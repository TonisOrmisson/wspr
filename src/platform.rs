//! The few things that differ per operating system: data folder, key storage, single
//! instance, installing and updating, paste, dialogs, overlay styling. Everything else is shared.

use std::path::PathBuf;
use std::sync::OnceLock;

pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    let d = base.unwrap_or_else(|| PathBuf::from(".")).join("wspr");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// With seconds, for the log.
pub fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// ---------- API keys: Windows DPAPI (readable only by this Windows user), Linux keyring ----------

#[cfg(windows)]
mod dpapi {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::*;

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        }
    }

    fn take_blob(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        unsafe {
            let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
            v
        }
    }

    pub fn protect(plain: &str) -> Option<Vec<u8>> {
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe { CryptProtectData(&blob(plain.as_bytes()), None, None, None, None, 0, &mut out) }
            .ok()?;
        Some(take_blob(out))
    }

    pub fn unprotect(enc: &[u8]) -> Option<String> {
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe { CryptUnprotectData(&blob(enc), None, None, None, None, 0, &mut out) }.ok()?;
        String::from_utf8(take_blob(out)).ok()
    }
}

/// Stores the key of `provider` and returns what settings.json keeps: the DPAPI-encrypted key
/// as hex on Windows, a marker on Linux (the key itself is in the keyring). "" for no key.
pub fn save_key(provider: &str, key: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        let _ = provider;
        if key.is_empty() {
            return Ok(String::new());
        }
        dpapi::protect(key)
            .map(|b| crate::settings::to_hex(&b))
            .ok_or_else(|| "Could not encrypt the API key".into())
    }
    #[cfg(target_os = "linux")]
    {
        let entry = keyring::Entry::new("wspr", provider).map_err(|e| format!("Keyring: {e}"))?;
        if key.is_empty() {
            let _ = entry.delete_credential();
            return Ok(String::new());
        }
        entry
            .set_password(key)
            .map_err(|e| format!("Could not store the API key in the keyring: {e}"))?;
        Ok("keyring".into())
    }
}

pub fn load_key(provider: &str, stored: &str) -> String {
    if stored.is_empty() {
        return String::new();
    }
    #[cfg(windows)]
    {
        let _ = provider;
        crate::settings::from_hex(stored)
            .and_then(|b| dpapi::unprotect(&b))
            .unwrap_or_default()
    }
    #[cfg(target_os = "linux")]
    keyring::Entry::new("wspr", provider)
        .and_then(|e| e.get_password())
        .unwrap_or_default()
}

// ---------- single instance ----------

/// False when another wspr is already running. The guard must live as long as the app.
#[cfg(windows)]
pub fn single_instance() -> Option<impl Sized> {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    let mutex =
        unsafe { CreateMutexW(None, true, windows::core::w!("Local\\wspr-single-instance")) }
            .ok()?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return None;
    }
    Some(mutex)
}

#[cfg(target_os = "linux")]
pub fn single_instance() -> Option<impl Sized> {
    let file = std::fs::File::create(data_dir().join("wspr.lock")).ok()?;
    // The lock is released when the process ends, also after a crash. After an update the old
    // process may still be quitting, so wait for it a few seconds before giving up.
    for _ in 0..50 {
        if file.try_lock().is_ok() {
            return Some(file);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

// ---------- installing and updating ----------

#[cfg(target_os = "linux")]
fn linux_bin() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/bin/wspr"))
}

/// Linux has no installer: on first start wspr copies itself to ~/.local/bin and adds itself to
/// the app menu and to autostart. Returns a message when it did so. Windows: the installer did it.
pub fn ensure_installed() -> Result<Option<String>, String> {
    #[cfg(windows)]
    return Ok(None);
    #[cfg(target_os = "linux")]
    {
        let bin = linux_bin().ok_or("HOME is not set")?;
        let home = bin
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .ok_or("bad HOME")?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let fresh = std::fs::canonicalize(&exe).ok() != std::fs::canonicalize(&bin).ok();
        if fresh {
            replace_binary(
                &bin,
                &std::fs::read(&exe).map_err(|e| format!("Could not read wspr: {e}"))?,
            )?;
        }
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName=wspr\nComment=Dictation: press a hotkey, speak, the text is pasted\n\
             Exec={}\nIcon=wspr\nTerminal=false\nCategories=Utility;\nStartupWMClass=wspr\n",
            bin.display()
        );
        let files: [(PathBuf, &[u8]); 3] = [
            (
                home.join(".local/share/applications/wspr.desktop"),
                desktop.as_bytes(),
            ),
            (
                home.join(".config/autostart/wspr.desktop"),
                desktop.as_bytes(),
            ),
            (
                home.join(".local/share/icons/hicolor/256x256/apps/wspr.png"),
                include_bytes!("../res/wspr.png"),
            ),
        ];
        for (path, bytes) in files {
            if fresh || !path.exists() {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
        Ok(fresh.then(|| {
            "wspr is installed: it is in the app menu and starts with your session".into()
        }))
    }
}

/// Writes the new binary next to the old one and renames it over, so a failed write keeps the old.
#[cfg(target_os = "linux")]
fn replace_binary(bin: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = bin.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = bin.with_extension("new");
    std::fs::write(&tmp, bytes).map_err(|e| format!("Could not write {}: {e}", tmp.display()))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, bin).map_err(|e| format!("Could not replace {}: {e}", bin.display()))
}

/// Installs a downloaded, checksum-verified update and starts it. The caller then quits.
pub fn install_update(version: &str, bytes: &[u8]) -> Result<(), String> {
    #[cfg(windows)]
    {
        // The silent installer replaces wspr.exe and starts the new version.
        let path = std::env::temp_dir().join(format!("wspr-setup-{version}.exe"));
        std::fs::write(&path, bytes).map_err(|e| format!("Could not save the update: {e}"))?;
        std::process::Command::new(&path)
            .arg("/S")
            .spawn()
            .map_err(|e| format!("Could not start the update installer: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let _ = version;
        let bin = linux_bin().ok_or("HOME is not set")?;
        replace_binary(&bin, bytes)?;
        // Started after a second, once this process has quit and released the single-instance lock.
        std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 1; exec \"$0\"")
            .arg(&bin)
            .spawn()
            .map_err(|e| format!("Could not start the new version: {e}"))?;
        Ok(())
    }
}

// ---------- paste ----------

/// Sends Ctrl+V to the focused window. False when the system blocked it
/// (on Windows: the target window runs as administrator).
pub fn send_paste(enigo: &mut enigo::Enigo) -> bool {
    use enigo::{Direction, Key, Keyboard};
    // The V key by position: with a non-Latin layout a Unicode 'v' would type a different key.
    #[cfg(windows)]
    let v = Key::Other(0x56);
    #[cfg(not(windows))]
    let v = Key::Unicode('v');
    let ok =
        enigo.key(Key::Control, Direction::Press).is_ok() && enigo.key(v, Direction::Click).is_ok();
    // Always release Ctrl, or it would stay held down.
    enigo.key(Key::Control, Direction::Release).is_ok() && ok
}

// ---------- beeps ----------

#[derive(Clone, Copy)]
pub enum Sound {
    Start,
    Stop,
    Error,
}

/// Plays a short tone on the default output device without blocking the caller.
pub fn play(sound: Sound) {
    let (freq, ms) = match sound {
        Sound::Start => (880.0, 90),
        Sound::Stop => (587.0, 90),
        Sound::Error => (220.0, 300),
    };
    std::thread::spawn(move || {
        let _ = play_tone(freq, ms);
    });
}

fn play_tone(freq: f32, ms: u32) -> Option<()> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    // One output device lookup per process; it is slow on some systems.
    static DEVICE: OnceLock<Option<cpal::Device>> = OnceLock::new();
    let device = DEVICE
        .get_or_init(|| cpal::default_host().default_output_device())
        .as_ref()?;
    let cfg = device.default_output_config().ok()?;
    let (rate, channels) = (cfg.sample_rate(), cfg.channels() as usize);
    let samples = crate::audio::tone(freq, ms, rate);
    let mut pos = 0;
    let mut next = move || {
        let s = samples.get(pos / channels).copied().unwrap_or(0.0);
        pos += 1;
        s
    };
    let config: cpal::StreamConfig = cfg.into();
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                out.iter_mut().for_each(|o| *o = next())
            },
            |_| {},
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            config,
            move |out: &mut [i16], _: &cpal::OutputCallbackInfo| {
                out.iter_mut()
                    .for_each(|o| *o = (next() * i16::MAX as f32) as i16)
            },
            |_| {},
            None,
        ),
        _ => return None,
    }
    .ok()?;
    stream.play().ok()?;
    std::thread::sleep(std::time::Duration::from_millis(ms as u64 + 150));
    Some(())
}

// ---------- dialogs and window styling ----------

/// A plain message box, for the rare cases the overlay cannot show (e.g. a second instance).
pub fn alert(text: &str) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MessageBoxW};
        let t: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MessageBoxW(
                None,
                windows::core::PCWSTR(t.as_ptr()),
                windows::core::w!("wspr"),
                MB_OK | MB_ICONWARNING,
            );
        }
    }
    #[cfg(target_os = "linux")]
    {
        let shown = std::process::Command::new("zenity")
            .args(["--warning", "--title=wspr", "--no-markup", "--text", text])
            .status()
            .is_ok_and(|s| s.success());
        if !shown {
            let _ = std::process::Command::new("notify-send")
                .args(["wspr", text])
                .status();
        }
        eprintln!("wspr: {text}");
    }
}

/// Rounds the overlay into a pill of `w` x `h` physical pixels (Linux paints it rounded instead).
#[cfg_attr(not(windows), allow(unused_variables))]
pub fn round_corners(window: &winit::window::Window, w: i32, h: i32) {
    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Dwm::{DWMWA_BORDER_COLOR, DwmSetWindowAttribute};
        use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
        if let Some(hwnd) = hwnd(window) {
            unsafe {
                // Windows 11 draws a thin light border on top of borderless windows; turn it off.
                const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
                let none = DWMWA_COLOR_NONE;
                let _ =
                    DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &none as *const u32 as _, 4);
                // The region also leaves out the top pixel row, where that border sits on Windows 10.
                // The window owns the region afterwards.
                let rgn = CreateRoundRectRgn(0, 1, w + 1, h + 1, h, h);
                SetWindowRgn(hwnd, Some(rgn), true);
            }
        }
    }
}

#[cfg(windows)]
fn hwnd(window: &winit::window::Window) -> Option<windows::Win32::Foundation::HWND> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(windows::Win32::Foundation::HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// The overlay must never take focus (the paste would go to it) and must stay out of Alt+Tab.
/// It must not be a layered window either: OpenGL cannot draw into one, so it would show empty.
#[cfg_attr(not(windows), allow(unused_variables))]
pub fn style_overlay(window: &winit::window::Window) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        let Some(hwnd) = hwnd(window) else { return };
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let ex = (ex | (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) as isize)
                & !(WS_EX_LAYERED.0 as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        }
    }
}
