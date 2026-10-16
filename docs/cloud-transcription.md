# Cloud Transcription

HEX transcribes locally by default. Cloud transcription is an explicit opt-in
that uploads each finished dictation clip to a hosted speech-to-text API and
pastes the returned text. It is off unless `cloud_transcription.enabled` is
`true`. Meetings and the local transcription API always stay on the local model.

Supported providers:

| `provider` | Endpoint                                               | Default model            | Key variable     |
| ---------- | ------------------------------------------------------ | ------------------------ | ---------------- |
| `openai`   | `https://api.openai.com/v1/audio/transcriptions`       | `gpt-4o-mini-transcribe` | `OPENAI_API_KEY` |
| `xai`      | `https://api.x.ai/v1/stt` (`grok` is accepted as alias) | provider default         | `XAI_API_KEY`    |
| `groq`     | `https://api.groq.com/openai/v1/audio/transcriptions`  | `whisper-large-v3-turbo` | `GROQ_API_KEY`   |

## Configure

Add a `cloud_transcription` object to the platform's settings file:

- macOS: `~/Library/Application Support/voice-control/settings.json` (restart HEX)
- Linux: `linux-settings.json` in the HEX data directory
  (`~/.local/share/voice-control/` by default); restart with `hex restart`
- Windows: `%APPDATA%\voice-control\windows-settings.json` (`voice-control settings`
  prints it); restart dictation after editing

```json
{
  "cloud_transcription": {
    "enabled": true,
    "provider": "openai",
    "model": "gpt-4o-transcribe",
    "api_key": "sk-...",
    "language": "en",
    "endpoint": null,
    "timeout_seconds": 60
  }
}
```

Only `enabled` and `provider` are needed. Omit `model` for the provider default,
`language` for automatic detection, and `endpoint` unless you use a compatible
gateway (HTTPS, or plain HTTP to `localhost`/loopback only).

`api_key` is stored in plain text in the settings file. To keep it out of the
file, omit it and export the provider's key variable in the environment of the
process that transcribes. On Linux the per-user `hex.service` does not inherit
your shell environment; use `systemctl --user edit hex.service` with
`Environment=OPENAI_API_KEY=...`, or `systemctl --user import-environment`.

## Check a key before enabling

Linux and Windows can transcribe a WAV with the configured provider, even while
`enabled` is still `false`:

```sh
hex model cloud-transcribe sample.wav             # Linux
voice-control model cloud-transcribe sample.wav   # Windows
```

## Behavior

- Clips are sent as 16 kHz mono 16-bit WAV. Captures longer than ten minutes are
  uploaded in ten-minute pieces and joined.
- On macOS, incremental (pause-piece) transcription is skipped while cloud is
  enabled, so each dictation is one upload.
- A failed request, a timeout, or a missing key fails that dictation; nothing is
  pasted and HEX does not fall back to the local model.
- Linux dictation starts without an installed local model when cloud is enabled.
  macOS onboarding still requires the selected local model.
