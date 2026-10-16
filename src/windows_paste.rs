use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};

use crate::windows_input::{key_is_down, keyboard_input, send_inputs};

const MODIFIER_POLL: Duration = Duration::from_millis(10);
/// Gives the clipboard owner change time to reach applications that read the
/// clipboard asynchronously after a paste keystroke.
const PASTE_SETTLE: Duration = Duration::from_millis(30);

/// Inserts text through the clipboard and a synthetic Ctrl-V. The transcript
/// remains on the clipboard; the previous contents are not restored yet.
pub struct WindowsPaster {
    stop: Arc<AtomicBool>,
}

impl WindowsPaster {
    pub fn new(stop: Arc<AtomicBool>) -> Self {
        Self { stop }
    }

    pub fn paste(&mut self, text: &str) -> Result<()> {
        set_clipboard_text(text)?;
        // A held Alt, Win, or Shift would turn Ctrl-V into another shortcut,
        // such as Win-V clipboard history. Wait without dropping the output.
        while [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
            .into_iter()
            .any(key_is_down)
        {
            if self.stop.load(Ordering::Acquire) {
                return Err(eyre!("paste cancelled during shutdown"));
            }
            thread::sleep(MODIFIER_POLL);
        }
        thread::sleep(PASTE_SETTLE);
        let inputs = [
            keyboard_input(VK_CONTROL, false),
            keyboard_input(VK_V, false),
            keyboard_input(VK_V, true),
            keyboard_input(VK_CONTROL, true),
        ];
        let sent = send_inputs(&inputs);
        if sent != inputs.len() {
            let error = std::io::Error::last_os_error();
            // Never leave a synthetic Ctrl held down.
            send_inputs(&[keyboard_input(VK_CONTROL, true)]);
            return Err(eyre!("Windows rejected the paste keystroke: {error}"));
        }
        Ok(())
    }
}

fn set_clipboard_text(text: &str) -> Result<()> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(|error| eyre!("could not set clipboard text: {error}"))
}
