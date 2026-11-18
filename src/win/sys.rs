//! Small Win32 helpers: clipboard, paste, sounds, DPAPI, time, icons, wide strings.

use std::sync::OnceLock;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::Security::Cryptography::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn now() -> String {
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute
    )
}

// ---------- clipboard ----------

fn open_clipboard() -> Result<(), String> {
    // Another app may hold the clipboard for a moment.
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err("Lõikelauda ei saanud avada".into())
}

pub fn get_text() -> Option<String> {
    open_clipboard().ok()?;
    let text = unsafe {
        GetClipboardData(CF_UNICODETEXT.0 as u32)
            .ok()
            .and_then(|h| {
                let mem = HGLOBAL(h.0);
                let p = GlobalLock(mem) as *const u16;
                if p.is_null() {
                    return None;
                }
                let len = (0..GlobalSize(mem) / 2)
                    .take_while(|&i| *p.add(i) != 0)
                    .count();
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
                let _ = GlobalUnlock(mem);
                Some(s)
            })
    };
    let _ = unsafe { CloseClipboard() };
    text
}

pub fn set_text(s: &str) -> Result<(), String> {
    let w = wide(s);
    open_clipboard()?;
    let r = unsafe {
        (|| -> windows::core::Result<()> {
            EmptyClipboard()?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2)?;
            let p = GlobalLock(mem) as *mut u16;
            if p.is_null() {
                let _ = GlobalFree(Some(mem));
                return Err(windows::core::Error::from_thread());
            }
            std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
            let _ = GlobalUnlock(mem);
            // On success the clipboard owns the memory.
            if let Err(e) = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0))) {
                let _ = GlobalFree(Some(mem));
                return Err(e);
            }
            Ok(())
        })()
    };
    let _ = unsafe { CloseClipboard() };
    r.map_err(|e| format!("Lõikelaud: {e}"))
}

/// False when Windows blocked the input, e.g. the target window runs as administrator.
pub fn send_ctrl_v() -> bool {
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                ..Default::default()
            },
        },
    };
    let inputs = [
        key(VK_CONTROL, false),
        key(VK_V, false),
        key(VK_V, true),
        key(VK_CONTROL, true),
    ];
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) as usize == inputs.len() }
}

// ---------- sounds ----------

#[derive(Clone, Copy)]
pub enum Sound {
    Start = 0,
    Stop = 1,
    Error = 2,
}

pub fn play(s: Sound) {
    static SOUNDS: OnceLock<[Vec<u8>; 3]> = OnceLock::new();
    let sounds = SOUNDS.get_or_init(|| {
        [
            crate::audio::beep(880.0, 90),
            crate::audio::beep(587.0, 90),
            crate::audio::beep(220.0, 300),
        ]
    });
    // The bytes live in a static, so async playback never reads freed memory.
    let data = &sounds[s as usize];
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(data.as_ptr() as *const u16),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

// ---------- DPAPI ----------

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

// ---------- icons ----------

/// A filled anti-aliased circle, used as the tray icon in different colours.
pub fn circle_icon(r: u8, g: u8, b: u8) -> HICON {
    const S: i32 = 32;
    let px: Vec<u32> = (0..S * S)
        .map(|i| {
            let (dx, dy) = ((i % S) as f32 - 15.5, (i / S) as f32 - 15.5);
            let a = (13.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            let a8 = (a * 255.0) as u32;
            // Icons take straight (not premultiplied) alpha.
            (a8 << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
        })
        .collect();
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: S,
                biHeight: -S,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            return LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
        };
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u32, px.len());
        let mask_bits = vec![0u8; (S * S / 8) as usize];
        let mask = CreateBitmap(S, S, 1, 1, Some(mask_bits.as_ptr() as _));
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}
