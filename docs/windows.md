# Run The Windows Preview

The Windows preview is a foreground console dictation loop: hold the shortcut,
speak, release, and HEX pastes the local transcript into the focused window. It
targets x86_64 Windows 10/11. Inference runs on the CPU. There is no Settings
window, tray icon, recording HUD, autostart, or updater yet; see the
[plan](plans/windows.md).

## Build

Install:

- [Rust](https://rustup.rs) with the default `x86_64-pc-windows-msvc` toolchain.
- Visual Studio 2022 Build Tools with **Desktop development with C++**.
- [CMake](https://cmake.org/download/), on `PATH`.

From a **Developer PowerShell for VS 2022**:

```powershell
cargo build --release --bin voice-control
$hex = ".\target\release\voice-control.exe"
& $hex model install      # downloads and checksum-verifies the default model
& $hex model check        # loads and prewarms it
& $hex devices            # lists microphones
& $hex                    # starts dictation; Ctrl-C stops
```

`transcribe.cpp` builds its C++ sources through CMake the first time. Its
Windows build uses a short junction under the output directory, so a deep
checkout path is fine.

## Use

| Action | Result |
| --- | --- |
| Hold `Alt+Space`, release | Capture, transcribe, paste |
| Tap twice within 300 ms | Lock capture; press again to finish |
| `Escape` while capturing | Cancel; Escape is not delivered to the app |
| `--device <fragment>` | Use the first microphone whose name contains it |

Captures under 300 ms are discarded. HEX swallows the shortcut so `Alt+Space`
does not open the window menu, and taps an unassigned key while Alt or Win is
held so releasing them does not open a menu bar or the Start menu.

Paste places the transcript on the clipboard, waits until you release Ctrl,
Alt, Shift, and Win, then sends Ctrl-V. The previous clipboard contents are not
restored. Windows blocks synthetic input into windows running as administrator
unless HEX runs elevated too; the transcript then remains on the clipboard.

## Settings And Logs

`voice-control settings` writes defaults if needed, then prints the settings
path and contents: `%APPDATA%\voice-control\windows-settings.json`.

```json
{
  "schema_version": 1,
  "dictation_hotkey": { "control": false, "alt": true, "shift": false, "win": false, "key": "space" },
  "double_tap_lock": true,
  "sound_effect_volume": 0.5
}
```

The file also records the `transcription` model and language; missing fields keep
their defaults.

`key` accepts `space`, `enter`, `tab`, `backspace`, `insert`, `pause`,
`scrolllock`, `capslock`, a letter or digit, or `f1`-`f24`. Modifiers must match
exactly. Restart dictation after editing.

Logs live in `%APPDATA%\voice-control\logs\`: `process.log` for diagnostics and
`live.ndjson` for dictation observations (`voice-control status --lines 20`).
Set `HEX_APPLICATION_SUPPORT_DIR` to isolate a test run.
