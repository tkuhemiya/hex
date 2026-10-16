# Windows Plan

**Status:** Fork-only preview. Phase 1 is implemented and cross-checked; it has
not yet been run on a physical Windows machine. Windows CI builds, tests, and
transcribes a known recording. The hotkey, microphone, and paste path still need
a manual run.

## Approach

Follow the Linux beta: concrete `windows_*` modules over the shared core, one
capability slice at a time, no platform abstraction layer. Windows reuses:

- `audio` (CPAL over WASAPI), `dictation`, `pause_segmenter`, `events`,
  `feedback` (rodio), `instance` (`fs2` lock), and `app_paths` (`%APPDATA%`).
- `transcription_models` and `gguf_session`, plus the offline
  `linux_transcriber` adapter, which has no Linux-specific code. Its name is
  kept to avoid conflicts with upstream; rename it if this fork diverges.
- `transcribe.cpp`, which supports MSVC directly (CPU by default).

## Phase 1: Console Dictation (implemented)

```ts
voice-control.exe dictate                 // the default command
├── windows_input -> WH_KEYBOARD_LL hook thread -> windows_gesture
│   ├── Hold / release -> Start / Finish
│   ├── Second tap < 300 ms -> Lock; next press finishes
│   └── Escape while capturing -> Cancel (swallowed)
├── windows_dictation -> DictationCapture -> bounded job queue -> transcriber
└── windows_paste -> arboard clipboard -> wait for modifiers -> SendInput Ctrl-V
```

- `windows_gesture` is pure, so its tests also run on Linux.
- `windows_settings` persists `windows-settings.json`; the default shortcut is
  `Alt+Space`.
- `windows_dictation` duplicates the Linux loop minus the HUD. Converge them once
  a third change would touch both.
- Cross-check from Linux without the C++ build:
  `TRANSCRIBE_DIR=<prefix with lib/transcribe-link.json> cargo clippy --target x86_64-pc-windows-msvc --bin voice-control --tests`.

### Validate On Windows

1. Build and run `model install`, `model check`, and `devices`.
2. Hold `Alt+Space` in Notepad: check the window menu does not open, the text
   pastes, and releasing Alt does not focus the menu bar.
3. Double-tap lock, then finish; Escape-cancel while held and while locked.
4. Paste into Windows Terminal, a browser, and an elevated window (expect
   failure, transcript on the clipboard).
5. Try `Win+…`, `Ctrl+Alt+…` (AltGr layouts), and an F-key binding.
6. Unplug the microphone while idle and while capturing.

## Phase 2: Background App

- Windows subsystem binary with a tray icon (Start/Stop, Quit, open logs),
  single instance, and Ctrl-C replaced by tray Quit.
- Autostart through the `HKCU\...\Run` key, with Windows as the source of truth.
- Microphone recovery: reuse the macOS bounded retry instead of exiting.
- Restore the previous clipboard when no newer change superseded it, using
  `GetClipboardSequenceNumber`.
- Vulkan inference behind a cargo feature, with CPU fallback.

## Phase 3: Settings And HUD

- Check whether pinned `gpui 0.2.2` has a working Windows backend. If it does,
  host the shared `desktop_ui` panes and the transcription picker; the Linux
  root shows the shape. If not, use a minimal native settings window.
- Shortcut capture in Settings and model selection with download progress.
- Focus-free, click-through recording HUD (`WS_EX_LAYERED | WS_EX_TRANSPARENT |
  WS_EX_NOACTIVATE | WS_EX_TOPMOST`).

## Phase 4: Distribution

Code signing, an installer (MSIX, WiX, or Velopack), and signed updates. Keep
updates owned by one mechanism, as the Linux direct-install layout does.

## Deferred

Voice commands (Moonshine as a DLL plus Windows actions), application/browser
context (UI Automation), Voice Action, meetings (WASAPI loopback), and History.
Each is its own slice.
