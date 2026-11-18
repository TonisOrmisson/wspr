//! Small settings window: API key, model, languages, hotkey, vocabulary hint.

use super::sys::wide;
use crate::hotkey;
use crate::openai::MODELS;
use crate::settings::{LANGUAGES, Settings, key_hint, language_index};
use std::cell::Cell;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const ID_KEY: i32 = 101;
const ID_MODEL: i32 = 102;
const ID_LANG: i32 = 103;
const ID_HOTKEY: i32 = 104;
const ID_PROMPT: i32 = 105;
const ID_KEY_HINT: i32 = 106;

thread_local! {
    static WINDOW: Cell<Option<HWND>> = const { Cell::new(None) };
    static FONT: Cell<HFONT> = Cell::new(HFONT::default());
    static LAST_FOCUS: Cell<HWND> = Cell::new(HWND::default());
    static SAVED_HOTKEY: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
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
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_HOTKEY_CLASS,
        };
        let _ = InitCommonControlsEx(&icc);

        // Size the client area; the frame and title bar come on top of it.
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: px(510.0),
            bottom: px(342.0),
        };
        let _ = AdjustWindowRectEx(
            &mut rc,
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            false,
            WS_EX_DLGMODALFRAME,
        );
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;
        let Ok(win) = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            w!("wsprSettings"),
            PCWSTR(wide(concat!("wspr ", env!("CARGO_PKG_VERSION"), " settings")).as_ptr()),
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
            ("OpenAI API key", ID_KEY, api_key, edit | ES_PASSWORD as u32),
            (
                "Model",
                ID_MODEL,
                &s.model,
                WS_TABSTOP.0 | WS_VSCROLL.0 | CBS_DROPDOWN as u32,
            ),
            (
                "Language",
                ID_LANG,
                &s.languages,
                WS_TABSTOP.0 | WS_VSCROLL.0 | CBS_DROPDOWNLIST as u32,
            ),
            (
                "Hotkey (click, then press keys)",
                ID_HOTKEY,
                &s.hotkey,
                WS_TABSTOP.0,
            ),
            ("Vocabulary hint (optional)", ID_PROMPT, &s.prompt, edit),
        ];
        for (i, (label, id, value, style)) in rows.iter().enumerate() {
            // The rows below the key leave room for the key hint line.
            let y = 18.0 + i as f32 * 38.0 + if i > 0 { 18.0 } else { 0.0 };
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
            if *id == ID_KEY {
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
                child(
                    w!("STATIC"),
                    &format!("Key: {}", key_hint(value)),
                    0,
                    WINDOW_EX_STYLE(0),
                    212.0,
                    y + 30.0,
                    278.0,
                    20.0,
                    ID_KEY_HINT,
                );
            } else if *id == ID_HOTKEY {
                let hk = child(
                    HOTKEY_CLASSW,
                    "",
                    *style,
                    WS_EX_CLIENTEDGE,
                    210.0,
                    y,
                    280.0,
                    26.0,
                    *id,
                );
                if let Ok((m, k)) = hotkey::parse(value) {
                    SendMessageW(
                        hk,
                        HKM_SETHOTKEY,
                        Some(WPARAM(hotkey::to_control(m, k) as usize)),
                        None,
                    );
                }
            } else if *id == ID_LANG {
                let combo = child(
                    w!("COMBOBOX"),
                    "",
                    *style,
                    WINDOW_EX_STYLE(0),
                    210.0,
                    y,
                    280.0,
                    300.0,
                    *id,
                );
                let mut labels: Vec<&str> = LANGUAGES.iter().map(|l| l.0).collect();
                // A value typed into an older settings file (e.g. "et,en") stays selectable.
                let selected = language_index(value).unwrap_or_else(|| {
                    labels.push(value);
                    labels.len() - 1
                });
                for l in labels {
                    let wl = wide(l);
                    SendMessageW(
                        combo,
                        CB_ADDSTRING,
                        Some(WPARAM(0)),
                        Some(LPARAM(wl.as_ptr() as isize)),
                    );
                }
                SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(selected)), None);
            } else if *id == ID_MODEL {
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
            "The hotkey is off while this window is open. Press it once to start dictation, again to stop.\n\
             OpenAI shuts down all models except gpt-transcribe on 2027-02-26.",
            0,
            WINDOW_EX_STYLE(0),
            16.0,
            228.0,
            480.0,
            64.0,
            0,
        );
        child(
            w!("BUTTON"),
            "Save",
            WS_TABSTOP.0 | BS_DEFPUSHBUTTON as u32,
            WINDOW_EX_STYLE(0),
            290.0,
            298.0,
            96.0,
            30.0,
            IDOK.0,
        );
        child(
            w!("BUTTON"),
            "Cancel",
            WS_TABSTOP.0,
            WINDOW_EX_STYLE(0),
            394.0,
            298.0,
            96.0,
            30.0,
            IDCANCEL.0,
        );

        // While the hotkey is being edited, pressing it must not start a dictation.
        super::suspend_hotkey();
        SAVED_HOTKEY.with(|h| *h.borrow_mut() = s.hotkey.clone());
        let _ = ShowWindow(win, SW_SHOW);
        let _ = SetForegroundWindow(win);
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

/// The code of the selected language, or the raw value of a custom entry.
fn selected_language(win: HWND) -> String {
    let i = unsafe {
        GetDlgItem(Some(win), ID_LANG)
            .map(|c| SendMessageW(c, CB_GETCURSEL, None, None).0)
            .unwrap_or(-1)
    };
    match usize::try_from(i).ok().and_then(|i| LANGUAGES.get(i)) {
        Some(l) => l.1.to_string(),
        None => text_of(win, ID_LANG),
    }
}

fn captured_hotkey(win: HWND) -> Result<String, String> {
    let word = unsafe {
        GetDlgItem(Some(win), ID_HOTKEY)
            .map(|c| SendMessageW(c, HKM_GETHOTKEY, None, None).0 as u32 & 0xFFFF)
            .unwrap_or(0)
    };
    // The control cannot show Win, so an unchanged hotkey keeps its saved text (e.g. Win+F9).
    let saved = SAVED_HOTKEY.with(|h| h.borrow().clone());
    if hotkey::parse(&saved).is_ok_and(|(m, k)| hotkey::to_control(m, k) == word) {
        return Ok(saved);
    }
    let (mods, vk) = hotkey::from_control(word);
    if vk == 0 {
        return Err("Click the Hotkey field and press the key combination you want".into());
    }
    hotkey::format(mods, vk)
        .ok_or_else(|| "That key cannot be used as a hotkey, choose another".into())
}

fn save(win: HWND) -> Result<(), String> {
    let new = Settings {
        api_key_enc: String::new(), // filled in by apply_settings
        model: text_of(win, ID_MODEL),
        languages: selected_language(win),
        hotkey: captured_hotkey(win)?,
        prompt: text_of(win, ID_PROMPT),
    };
    if new.model.is_empty() {
        return Err("Model is empty".into());
    }
    super::apply_settings(new, text_of(win, ID_KEY))
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                // Typing a new key updates the "sk-proj-…wxyz" hint below the field.
                if (wp.0 >> 16) as u32 == EN_CHANGE && (wp.0 & 0xFFFF) as i32 == ID_KEY {
                    let hint = wide(&format!("Key: {}", key_hint(&text_of(hwnd, ID_KEY))));
                    if let Ok(h) = GetDlgItem(Some(hwnd), ID_KEY_HINT) {
                        let _ = SetWindowTextW(h, PCWSTR(hint.as_ptr()));
                    }
                    return LRESULT(0);
                }
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
            // A plain window does not restore keyboard focus like a dialog does
            // (after a message box or Alt+Tab), so remember and restore it here.
            WM_ACTIVATE => {
                if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                    LAST_FOCUS.with(|f| f.set(GetFocus()));
                } else {
                    let last = LAST_FOCUS.with(|f| f.get());
                    let key = GetDlgItem(Some(hwnd), ID_KEY).unwrap_or_default();
                    let target = if IsChild(hwnd, last).as_bool() {
                        last
                    } else if GetWindowTextLengthW(key) == 0 {
                        key
                    } else {
                        // A saved key should not be overwritten by accidental typing.
                        GetDlgItem(Some(hwnd), ID_LANG).unwrap_or_default()
                    };
                    let _ = SetFocus(Some(target));
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                WINDOW.with(|c| c.set(None));
                super::resume_hotkey();
                let _ = DeleteObject(FONT.with(|f| f.replace(HFONT::default())).into());
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
