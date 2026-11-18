# wspr

Minimal dictation for Windows. Press a hotkey, speak, press it again: the audio goes to the
OpenAI transcription API and the text is pasted where your cursor is. Nothing else.

No accounts, no cloud of its own, no telemetry, no meetings, no notes. One ~2 MB `.exe`.

The UI is in Estonian, because that is what its author uses.

## Features

- Toggle hotkey (default `Ctrl+Shift+Space`): one press starts, the next one stops.
- Short beeps when recording starts and stops.
- A small on-screen pill shows the state: recording (with a timer), transcribing, done, error.
  It never takes focus, so the paste goes to the window you were typing in.
- Tray menu with the last 10 transcriptions (click copies to the clipboard) and the full history
  of the last 50. Useful when the text was pasted into the wrong place.
- If the API request fails, the recording is kept and can be retried from the tray menu.
- Settings window: OpenAI API key, model, input language(s), hotkey, optional vocabulary hint.
- Your clipboard text is restored after pasting.

## Setup

1. Download `wspr.exe` from [Releases](https://github.com/TonisOrmisson/wspr/releases).
2. Run it. The settings window opens on first start: paste your OpenAI API key.
   Windows SmartScreen may warn about an unsigned app: "More info" > "Run anyway".
3. To start it with Windows, put a shortcut to `wspr.exe` in the folder that opens with
   `Win+R` > `shell:startup`.

Settings and history are in `%APPDATA%\wspr\`. The API key is encrypted with Windows DPAPI
(readable only by your Windows user).

## Models

Default is `gpt-transcribe`. The model field is editable. OpenAI shuts down `whisper-1`,
`gpt-4o-transcribe` and `gpt-4o-mini-transcribe` on 2027-02-26.

Languages are ISO-639-1 codes, e.g. `et`, or several: `et,en`. Leave empty for auto-detection.

## Build

```bash
# on Windows
cargo build --release
# cross-compile from Linux (needs gcc-mingw-w64-x86-64)
cargo build --release --target x86_64-pc-windows-gnu
# unit tests of the platform-independent parts run anywhere
cargo test
```

## License

MIT
