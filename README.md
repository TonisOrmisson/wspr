# wspr

Minimal dictation for Windows 10/11 and Linux (X11, and Wayland on GNOME). Press a hotkey, speak, press it again: the audio is
transcribed by OpenAI or by any speech-to-text model on OpenRouter, and the text is pasted where
your cursor is.

No account, no telemetry, one file of ~8-13 MB. You bring your own API key.

## Features

- Toggle hotkey (default `Ctrl+Shift+Space`): one press starts, the next one stops.
- Short beeps when recording starts and stops.
- A small on-screen pill shows the state: recording (with a timer), transcribing, done, error.
  It never takes focus, so the text lands in the window you were typing in.
- Your clipboard text is restored after pasting.
- History of recent transcriptions (10 to 500, or off), one click away in the tray menu. Useful
  when the text went to the wrong place.
- If a request fails, the recording is kept and can be retried from the tray menu.
- A log (tray menu > View log) records recordings, transcriptions, errors and updates.
- Updates itself: checks GitHub Releases at start and every hour, verifies the download's
  SHA-256 and installs it silently. It never interrupts a dictation.

## Setup on Windows

1. Download `wspr-setup-x.y.z.exe` from [Releases](https://github.com/TonisOrmisson/wspr/releases)
   and run it. It installs for your user only (no admin rights) into
   `%LOCALAPPDATA%\Programs\wspr`, adds wspr to the Start menu and, if the box stays checked,
   starts it with Windows. Uninstall from Settings > Apps.
   The installer is not code-signed, so SmartScreen may warn: "More info" > "Run anyway".
2. On first start the settings window opens. Pick a provider, paste its API key, choose your
   language (or Auto-detect) and, if you like, a different hotkey: click the field and press it.

## Setup on Linux

Download `wspr-linux-x86_64` from [Releases](https://github.com/TonisOrmisson/wspr/releases), make
it executable and run it:

```bash
chmod +x wspr-linux-x86_64 && ./wspr-linux-x86_64
```

On first start it copies itself to `~/.local/bin/wspr`, adds itself to the app menu and to
autostart, and opens the settings. Updates replace `~/.local/bin/wspr`. API keys go to the desktop
keyring (GNOME Keyring or KWallet). Built on Ubuntu 24.04.

On Wayland, programs may not grab keys or type into other windows, so wspr adds its hotkey to
GNOME's keyboard shortcuts (Settings > Keyboard > Custom Shortcuts) and pastes through GNOME's
remote control interface. Other Wayland desktops (KDE and others) are not supported yet.

## Using it

The tray icon menu has Check for updates, History, View log, Settings and Quit, plus Retry after
a failed request.

## Providers

| Provider | Default model | Key from |
|---|---|---|
| OpenAI | `gpt-transcribe` | [platform.openai.com](https://platform.openai.com/api-keys) |
| OpenRouter | none, type a model ID, e.g. `elevenlabs/scribe-v2` | [openrouter.ai](https://openrouter.ai/keys) |

OpenRouter's speech-to-text models are listed at
[openrouter.ai/models](https://openrouter.ai/models?output_modalities=transcription).
The OpenAI model field is editable too.
Each provider keeps its own key and model. OpenAI shuts down `whisper-1`, `gpt-4o-transcribe`
and `gpt-4o-mini-transcribe` on 2027-02-26.

## Privacy

- Audio goes only to the provider you choose, and only while you dictate.
- Settings and history live in `%APPDATA%\wspr\` (Windows) or `~/.config/wspr/` (Linux). API keys
  are encrypted with Windows DPAPI (readable only by your Windows user) or kept in the Linux keyring. The history is plain text; set it to Off to keep none.
- The log (`wspr.log`, at most ~512 KB) never contains dictated text or API keys.
- The update check is one request per hour to the GitHub API.

## Build

```bash
cargo test                                             # platform-independent parts, any OS
cargo build --release                                  # on Windows or Linux (Linux needs libasound2-dev, libxkbcommon-dev)
cargo build --release --target x86_64-pc-windows-gnu   # from Linux, needs gcc-mingw-w64-x86-64
makensis -DVERSION=X.Y.Z -DEXE=path/to/wspr.exe installer/wspr.nsi   # installer, NSIS 3
```

## License

MIT
