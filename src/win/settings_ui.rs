//! Small settings window: API key, model, language, hotkey, history size.

use super::sys::wide;
use crate::hotkey;
use crate::openai::MODELS;
use crate::settings::{HISTORY_SIZES, LANGUAGES, Settings, key_hint, language_index};
use std::cell::{Cell, RefCell};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, GetKeyState, SetFocus, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const ID_KEY: i32 = 101;
const ID_MODEL: i32 = 102;
const ID_LANG: i32 = 103;
const ID_HOTKEY: i32 = 104;
const ID_HISTORY: i32 = 105;
const ID_KEY_HINT: i32 = 106;
const ID_VIEW_HISTORY: i32 = 107;

thread_local! {
    static WINDOW: Cell<Option<HWND>> = const { Cell::new(None) };
    static FONT: Cell<HFONT> = Cell::new(HFONT::default());
    static LAST_FOCUS: Cell<HWND> = Cell::new(HWND::default());
    /// Last complete hotkey in the field; restored when only modifiers were pressed.
    static HOTKEY_TEXT: RefCell<String> = const { RefCell::new(String::new()) };
    static HISTORY_SIZE: Cell<usize> = const { Cell::new(0) };
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
            ("Hotkey (click, then press)", ID_HOTKEY, &s.hotkey, edit),
            (
                "Keep transcriptions",
                ID_HISTORY,
                "",
                WS_TABSTOP.0 | CBS_DROPDOWNLIST as u32,
            ),
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
                HOTKEY_TEXT.with(|t| *t.borrow_mut() = value.to_string());
                let _ = SetWindowSubclass(hk, Some(hotkey_field_proc), 1, 0);
            } else if *id == ID_HISTORY {
                let combo = child(
                    w!("COMBOBOX"),
                    "",
                    *style,
                    WINDOW_EX_STYLE(0),
                    210.0,
                    y,
                    170.0,
                    200.0,
                    *id,
                );
                let mut labels: Vec<String> =
                    HISTORY_SIZES.iter().map(|h| h.0.to_string()).collect();
                // A size from a hand-edited settings file stays selectable.
                let selected = HISTORY_SIZES
                    .iter()
                    .position(|h| h.1 == s.history_size)
                    .unwrap_or_else(|| {
                        labels.push(format!("Last {}", s.history_size));
                        labels.len() - 1
                    });
                HISTORY_SIZE.with(|h| h.set(s.history_size));
                for l in labels {
                    let wl = wide(&l);
                    SendMessageW(
                        combo,
                        CB_ADDSTRING,
                        Some(WPARAM(0)),
                        Some(LPARAM(wl.as_ptr() as isize)),
                    );
                }
                SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(selected)), None);
                child(
                    w!("BUTTON"),
                    "View…",
                    WS_TABSTOP.0,
                    WINDOW_EX_STYLE(0),
                    390.0,
                    y - 1.0,
                    100.0,
                    28.0,
                    ID_VIEW_HISTORY,
                );
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

fn held_modifiers() -> u32 {
    let down = |vk: u16| unsafe { GetKeyState(vk as i32) } < 0;
    let mut mods = 0;
    if down(VK_CONTROL.0) {
        mods |= hotkey::MOD_CONTROL;
    }
    if down(VK_MENU.0) {
        mods |= hotkey::MOD_ALT;
    }
    if down(VK_SHIFT.0) {
        mods |= hotkey::MOD_SHIFT;
    }
    if down(VK_LWIN.0) || down(VK_RWIN.0) {
        mods |= hotkey::MOD_WIN;
    }
    mods
}

fn set_text(h: HWND, s: &str) {
    let w = wide(s);
    unsafe {
        let _ = SetWindowTextW(h, PCWSTR(w.as_ptr()));
    }
}

/// The hotkey field records key presses instead of accepting typed text.
unsafe extern "system" fn hotkey_field_proc(
    h: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    unsafe {
        match msg {
            // Take every key, Space and Alt combinations included; only Tab still moves focus.
            WM_GETDLGCODE => {
                let m = lp.0 as *const MSG;
                if !m.is_null() && (*m).wParam.0 == VK_TAB.0 as usize && held_modifiers() == 0 {
                    return DefSubclassProc(h, msg, wp, lp);
                }
                LRESULT(DLGC_WANTALLKEYS as isize)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let vk = wp.0 as u32;
                let mods = held_modifiers();
                let is_modifier = matches!(vk, 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5);
                if is_modifier {
                    set_text(h, &hotkey::modifier_prefix(mods));
                } else if let Some(s) = hotkey::format(mods, vk) {
                    set_text(h, &s);
                    HOTKEY_TEXT.with(|t| *t.borrow_mut() = s);
                }
                LRESULT(0)
            }
            WM_KEYUP | WM_SYSKEYUP => {
                // Only modifiers were pressed and released: show the last full hotkey again.
                if held_modifiers() == 0 {
                    set_text(h, &HOTKEY_TEXT.with(|t| t.borrow().clone()));
                }
                LRESULT(0)
            }
            WM_CHAR | WM_SYSCHAR | WM_PASTE | WM_CUT | WM_CLEAR | WM_UNDO | WM_CONTEXTMENU => {
                LRESULT(0)
            }
            _ => DefSubclassProc(h, msg, wp, lp),
        }
    }
}

fn selected_history_size(win: HWND) -> usize {
    let i = unsafe {
        GetDlgItem(Some(win), ID_HISTORY)
            .map(|c| SendMessageW(c, CB_GETCURSEL, None, None).0)
            .unwrap_or(-1)
    };
    match usize::try_from(i).ok().and_then(|i| HISTORY_SIZES.get(i)) {
        Some(h) => h.1,
        None => HISTORY_SIZE.with(|h| h.get()),
    }
}

fn save(win: HWND) -> Result<(), String> {
    let new = Settings {
        api_key_enc: String::new(), // filled in by apply_settings
        model: text_of(win, ID_MODEL),
        languages: selected_language(win),
        hotkey: HOTKEY_TEXT.with(|t| t.borrow().clone()),
        history_size: selected_history_size(win),
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
                    ID_VIEW_HISTORY => super::open_history(),
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
