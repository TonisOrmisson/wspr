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
- History of the last transcriptions (10 to 500, or off), viewable from Settings. Useful when the
  text was pasted into the wrong place.
- If the API request fails, the recording is kept and can be retried from the tray menu.
- Settings window: OpenAI API key, model, input language, hotkey (press it to record it),
  history size.
- Your clipboard text is restored after pasting.
- Updates itself: checks GitHub Releases at start and every hour (or from the tray menu),
  verifies the download's SHA-256 and installs it silently. It never interrupts a dictation.

## Setup

1. Download `wspr-setup-x.y.z.exe` from [Releases](https://github.com/TonisOrmisson/wspr/releases)
   and run it. It installs for your user only (no admin rights) into
   `%LOCALAPPDATA%\Programs\wspr`, adds wspr to the Start menu and, if you leave the box
   checked, starts it with Windows. Uninstall from Settings > Apps.
   Windows SmartScreen may warn about an unsigned app: "More info" > "Run anyway".
2. The settings window opens on first start: paste your OpenAI API key.

A portable `wspr.exe` is attached to each release too; it updates itself through the installer.

Settings and history are in `%APPDATA%\wspr\`. The API key is encrypted with Windows DPAPI
(readable only by your Windows user).

## Models

Default is `gpt-transcribe`. The model field is editable. OpenAI shuts down `whisper-1`,
`gpt-4o-transcribe` and `gpt-4o-mini-transcribe` on 2027-02-26.

Pick the language you speak, or Auto-detect.

## Build

```bash
# on Windows
cargo build --release
# cross-compile from Linux (needs gcc-mingw-w64-x86-64)
cargo build --release --target x86_64-pc-windows-gnu
# unit tests of the platform-independent parts run anywhere
cargo test
# installer (NSIS 3)
makensis -DVERSION=0.2.0 -DEXE=path/to/wspr.exe installer/wspr.nsi
```

## License

MIT
