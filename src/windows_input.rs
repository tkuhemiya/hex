//! The global dictation shortcut on Windows: a low-level keyboard hook
//! (`WH_KEYBOARD_LL`) on its own message-loop thread. It swallows the trigger
//! while it belongs to HEX, and Escape only while a capture is active, so the
//! chord never reaches the foreground application.

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, PM_NOREMOVE,
    PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_APP, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

pub use crate::windows_gesture::HotkeyEvent;
use crate::windows_gesture::{Decision, Gesture};
use crate::windows_settings::WindowsHotkey;

/// An unassigned virtual key. Tapping it while Alt or Win is held keeps their
/// release from opening a menu bar or the Start menu after HEX swallowed the
/// trigger.
const MENU_MASK_KEY: u16 = 0xe8;
const WM_SEND_MENU_MASK: u32 = WM_APP + 1;

pub struct WindowsHotkeyMonitor {
    pub events: Receiver<HotkeyEvent>,
    pub errors: Receiver<String>,
    thread_id: u32,
    worker: Option<JoinHandle<()>>,
}

impl WindowsHotkeyMonitor {
    pub fn start(binding: &WindowsHotkey, double_tap_enabled: bool) -> Result<Self> {
        let trigger = binding.virtual_key()?;
        let modifiers = Modifiers {
            control: binding.control,
            alt: binding.alt,
            shift: binding.shift,
            win: binding.win,
        };
        let (events_sender, events) = mpsc::sync_channel(16);
        let (error_sender, errors) = mpsc::channel();
        let (ready_sender, ready) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("hex-keyboard-hook".into())
            .spawn(move || {
                run_hook_thread(
                    HookState {
                        trigger,
                        modifiers,
                        gesture: Gesture::new(double_tap_enabled),
                        events: events_sender,
                        errors: error_sender,
                    },
                    ready_sender,
                )
            })?;
        let thread_id = match ready.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(thread_id)) => thread_id,
            Ok(Err(error)) => {
                let _ = worker.join();
                return Err(eyre!(error));
            }
            Err(_) => return Err(eyre!("timed out installing the Windows keyboard hook")),
        };
        Ok(Self {
            events,
            errors,
            thread_id,
            worker: Some(worker),
        })
    }
}

impl Drop for WindowsHotkeyMonitor {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Modifiers {
    control: bool,
    alt: bool,
    shift: bool,
    win: bool,
}

impl Modifiers {
    fn held() -> Self {
        Self {
            control: key_is_down(VK_CONTROL),
            alt: key_is_down(VK_MENU),
            shift: key_is_down(VK_SHIFT),
            win: key_is_down(VK_LWIN) || key_is_down(VK_RWIN),
        }
    }
}

pub(crate) fn key_is_down(key: u16) -> bool {
    // The high bit reports the current physical state. The hook sees an event
    // before Windows updates this state, so it reflects earlier keys only.
    unsafe { GetAsyncKeyState(i32::from(key)) < 0 }
}

struct HookState {
    trigger: u32,
    modifiers: Modifiers,
    gesture: Gesture,
    events: SyncSender<HotkeyEvent>,
    errors: Sender<String>,
}

thread_local! {
    static HOOK: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

fn run_hook_thread(state: HookState, ready: SyncSender<Result<u32, String>>) {
    HOOK.with(|hook| *hook.borrow_mut() = Some(state));
    let mut message: MSG = unsafe { std::mem::zeroed() };
    // Create this thread's message queue before anyone posts WM_QUIT to it.
    unsafe {
        PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }
    let hook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook),
            GetModuleHandleW(std::ptr::null()),
            0,
        )
    };
    if hook.is_null() {
        let error = std::io::Error::last_os_error();
        let _ = ready.send(Err(format!(
            "could not install the Windows keyboard hook: {error}"
        )));
        return;
    }
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
    // GetMessageW returns 0 for WM_QUIT and -1 on failure.
    while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
        if message.message == WM_SEND_MENU_MASK {
            send_menu_mask();
        }
    }
    unsafe {
        UnhookWindowsHookEx(hook);
    }
    HOOK.with(|hook| hook.borrow_mut().take());
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        // HEX's own Ctrl-V and menu-mask input arrives here as injected input.
        if event.flags & LLKHF_INJECTED == 0
            && let Some(decision) = decide(event.vkCode, wparam as u32)
            && decision.suppress
        {
            return 1;
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn decide(key: u32, message: u32) -> Option<Decision> {
    let down = match message {
        WM_KEYDOWN | WM_SYSKEYDOWN => true,
        WM_KEYUP | WM_SYSKEYUP => false,
        _ => return None,
    };
    HOOK.with(|hook| {
        // A reentrant call while the state is borrowed passes the key through.
        let mut hook = hook.try_borrow_mut().ok()?;
        let state = hook.as_mut()?;
        let now = Instant::now();
        let decision = if key == state.trigger {
            if down {
                let held = Modifiers::held();
                let decision = state.gesture.trigger_down(now, held == state.modifiers);
                if decision.event.is_some() && (held.alt || held.win) {
                    // Send after returning: SendInput inside the hook would
                    // reenter it before this event is decided.
                    unsafe {
                        PostThreadMessageW(GetCurrentThreadId(), WM_SEND_MENU_MASK, 0, 0);
                    }
                }
                decision
            } else {
                state.gesture.trigger_up(now)
            }
        } else if key == u32::from(VK_ESCAPE) {
            if down {
                state.gesture.escape_down()
            } else {
                state.gesture.escape_up()
            }
        } else {
            return None;
        };
        if let Some(event) = decision.event {
            match state.events.try_send(event) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => {}
                Err(TrySendError::Full(_)) => {
                    let _ = state
                        .errors
                        .send("Windows hotkey event queue overflow".into());
                }
            }
        }
        Some(decision)
    })
}

fn send_menu_mask() {
    let inputs = [
        keyboard_input(MENU_MASK_KEY, false),
        keyboard_input(MENU_MASK_KEY, true),
    ];
    send_inputs(&inputs);
}

pub(crate) fn keyboard_input(key: u16, release: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if release { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Returns how many events Windows accepted. User Interface Privilege
/// Isolation can still drop accepted input aimed at an elevated window.
pub(crate) fn send_inputs(inputs: &[INPUT]) -> usize {
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        ) as usize
    }
}
