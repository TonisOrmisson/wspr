//! Small settings window: API key, model, languages, hotkey, vocabulary hint.

use super::sys::wide;
use crate::openai::MODELS;
use crate::settings::Settings;
use std::cell::Cell;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const ID_KEY: i32 = 101;
const ID_MODEL: i32 = 102;
const ID_LANG: i32 = 103;
const ID_HOTKEY: i32 = 104;
const ID_PROMPT: i32 = 105;

thread_local! {
    static WINDOW: Cell<Option<HWND>> = const { Cell::new(None) };
    static FONT: Cell<HFONT> = Cell::new(HFONT::default());
}

pub fn hwnd() -> Option<HWND> {
    WINDOW.with(|w| w.get())
}

pub fn open(hinst: HINSTANCE, s: &Settings, api_key: &str, scale: f32) {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = SetForegroundWindow(h);
        }
        return;
    }
    let px = |v: f32| (v * scale) as i32;
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: w!("wsprSettings"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as usize as *mut _),
            ..Default::default()
        };
        RegisterClassW(&wc); // fails harmlessly when already registered

        let (w, h) = (px(520.0), px(330.0));
        let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;
        let Ok(win) = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            w!("wsprSettings"),
            w!("wspr seaded"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(hinst),
            None,
        ) else {
            return;
        };
        WINDOW.with(|c| c.set(Some(win)));
        let font = CreateFontW(
            -px(15.0),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI"),
        );
        FONT.with(|f| f.set(font));

        let child = |class: PCWSTR,
                     text: &str,
                     style: u32,
                     ex: WINDOW_EX_STYLE,
                     x: f32,
                     y: f32,
                     w: f32,
                     h: f32,
                     id: i32| {
            let t = wide(text);
            let c = CreateWindowExW(
                ex,
                class,
                PCWSTR(t.as_ptr()),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                px(x),
                px(y),
                px(w),
                px(h),
                Some(win),
                Some(HMENU(id as usize as *mut _)),
                Some(hinst),
                None,
            )
            .unwrap_or_default();
            SendMessageW(
                c,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
            c
        };
        let edit = WS_TABSTOP.0 | ES_AUTOHSCROLL as u32;
        let rows: [(&str, i32, &str, u32); 5] = [
            (
                "OpenAI API-võti",
                ID_KEY,
                api_key,
                edit | ES_PASSWORD as u32,
            ),
            (
                "Mudel",
                ID_MODEL,
                &s.model,
                WS_TABSTOP.0 | WS_VSCROLL.0 | CBS_DROPDOWN as u32,
            ),
            ("Keel (nt et või et,en)", ID_LANG, &s.languages, edit),
            ("Kiirklahv", ID_HOTKEY, &s.hotkey, edit),
            ("Sõnavara vihje (valikuline)", ID_PROMPT, &s.prompt, edit),
        ];
        for (i, (label, id, value, style)) in rows.iter().enumerate() {
            let y = 18.0 + i as f32 * 38.0;
            child(
                w!("STATIC"),
                label,
                0,
                WINDOW_EX_STYLE(0),
                16.0,
                y + 4.0,
                190.0,
                22.0,
                0,
            );
            if *id == ID_MODEL {
                let combo = child(
                    w!("COMBOBOX"),
                    "",
                    *style,
                    WINDOW_EX_STYLE(0),
                    210.0,
                    y,
                    280.0,
                    200.0,
                    *id,
                );
                for m in MODELS {
                    let wm = wide(m);
                    SendMessageW(
                        combo,
                        CB_ADDSTRING,
                        Some(WPARAM(0)),
                        Some(LPARAM(wm.as_ptr() as isize)),
                    );
                }
                let wv = wide(value);
                let _ = SetWindowTextW(combo, PCWSTR(wv.as_ptr()));
            } else {
                child(
                    w!("EDIT"),
                    value,
                    *style,
                    WS_EX_CLIENTEDGE,
                    210.0,
                    y,
                    280.0,
                    26.0,
                    *id,
                );
            }
        }
        child(
            w!("STATIC"),
            "Kiirklahv nt Ctrl+Shift+Space, Alt+D või F9. Üks vajutus alustab, teine lõpetab.\n\
             Mudelid peale gpt-transcribe suletakse OpenAI poolt 26.02.2027.",
            0,
            WINDOW_EX_STYLE(0),
            16.0,
            210.0,
            480.0,
            44.0,
            0,
        );
        child(
            w!("BUTTON"),
            "Salvesta",
            WS_TABSTOP.0 | BS_DEFPUSHBUTTON as u32,
            WINDOW_EX_STYLE(0),
            290.0,
            262.0,
            96.0,
            30.0,
            IDOK.0,
        );
        child(
            w!("BUTTON"),
            "Tühista",
            WS_TABSTOP.0,
            WINDOW_EX_STYLE(0),
            394.0,
            262.0,
            96.0,
            30.0,
            IDCANCEL.0,
        );

        let _ = ShowWindow(win, SW_SHOW);
        let _ = SetForegroundWindow(win);
        if api_key.is_empty()
            && let Ok(k) = GetDlgItem(Some(win), ID_KEY)
        {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(k));
        }
    }
}

fn text_of(win: HWND, id: i32) -> String {
    unsafe {
        let Ok(c) = GetDlgItem(Some(win), id) else {
            return String::new();
        };
        let mut buf = vec![0u16; GetWindowTextLengthW(c) as usize + 1];
        let n = GetWindowTextW(c, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
            .trim()
            .to_string()
    }
}

fn save(win: HWND) -> Result<(), String> {
    let new = Settings {
        api_key_enc: String::new(), // filled in by apply_settings
        model: text_of(win, ID_MODEL),
        languages: text_of(win, ID_LANG),
        hotkey: text_of(win, ID_HOTKEY),
        prompt: text_of(win, ID_PROMPT),
    };
    if new.model.is_empty() {
        return Err("Mudel puudub".into());
    }
    super::apply_settings(new, text_of(win, ID_KEY))
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                match (wp.0 & 0xFFFF) as i32 {
                    id if id == IDOK.0 => match save(hwnd) {
                        Ok(()) => {
                            let _ = DestroyWindow(hwnd);
                        }
                        Err(e) => {
                            let t = wide(&e);
                            MessageBoxW(
                                Some(hwnd),
                                PCWSTR(t.as_ptr()),
                                w!("wspr"),
                                MB_OK | MB_ICONWARNING,
                            );
                        }
                    },
                    id if id == IDCANCEL.0 => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                WINDOW.with(|c| c.set(None));
                let _ = DeleteObject(FONT.with(|f| f.replace(HFONT::default())).into());
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
