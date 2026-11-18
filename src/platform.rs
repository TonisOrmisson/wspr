//! The few things that differ per operating system: data folder, key encryption, single
//! instance, paste, beeps, time. Everything else is shared.

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

// ---------- key encryption (Windows DPAPI: readable only by this Windows user) ----------

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

/// Encrypted key as stored in settings.json (hex), or "" for no key.
pub fn encrypt_key(key: &str) -> Result<String, String> {
    if key.is_empty() {
        return Ok(String::new());
    }
    #[cfg(windows)]
    return dpapi::protect(key)
        .map(|b| crate::settings::to_hex(&b))
        .ok_or_else(|| "Could not encrypt the API key".into());
}

pub fn decrypt_key(stored: &str) -> String {
    #[cfg(windows)]
    return crate::settings::from_hex(stored)
        .and_then(|b| dpapi::unprotect(&b))
        .unwrap_or_default();
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
}

/// Rounds the overlay into a pill of `w` x `h` physical pixels.
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
