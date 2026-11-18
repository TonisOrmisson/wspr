//! Small always-on-top status pill. It never takes focus and clicks pass through it,
//! so the paste still lands in the window the user was typing in.

use super::sys::wide;
use std::cell::RefCell;
use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

#[derive(Clone)]
pub enum View {
    Hidden,
    Recording(Instant),
    Transcribing,
    Done,
    Info(String),
    Error(String),
}

thread_local! {
    static VIEW: RefCell<View> = const { RefCell::new(View::Hidden) };
}

pub fn create(hinst: HINSTANCE) -> HWND {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: w!("wsprOverlay"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED,
            w!("wsprOverlay"),
            w!("wspr"),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(hinst),
            None,
        )
        .unwrap_or_default();
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 235, LWA_ALPHA);
        hwnd
    }
}

fn text_and_color(v: &View) -> (String, COLORREF) {
    // COLORREF is 0x00BBGGRR.
    match v {
        View::Hidden => (String::new(), COLORREF(0)),
        View::Recording(t) => {
            let s = t.elapsed().as_secs();
            (
                format!("Recording   {}:{:02}", s / 60, s % 60),
                COLORREF(0x003C3CE6),
            )
        }
        View::Transcribing => ("Transcribing…".into(), COLORREF(0x0014A0E6)),
        View::Done => ("Done".into(), COLORREF(0x0050C850)),
        View::Info(s) => (s.clone(), COLORREF(0x00B4B4B4)),
        View::Error(s) => (format!("Error: {s}"), COLORREF(0x003C3CE6)),
    }
}

pub fn show(hwnd: HWND, v: View, scale: f32) {
    let hidden = matches!(v, View::Hidden);
    let wide_pill = matches!(v, View::Error(_) | View::Info(_));
    VIEW.with(|cell| *cell.borrow_mut() = v);
    unsafe {
        if hidden {
            let _ = ShowWindow(hwnd, SW_HIDE);
            return;
        }
        let (w, h) = (
            ((if wide_pill { 560.0 } else { 230.0 }) * scale) as i32,
            (40.0 * scale) as i32,
        );
        let mut area = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut area as *mut _ as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let x = area.left + (area.right - area.left - w) / 2;
        let y = area.bottom - h - (32.0 * scale) as i32;
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            w,
            h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, h, h);
        SetWindowRgn(hwnd, Some(rgn), true); // the window owns the region now
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

pub fn repaint(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg != WM_PAINT {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let (text, color) = VIEW.with(|cell| text_and_color(&cell.borrow()));
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let h = rc.bottom;

        let bg = CreateSolidBrush(COLORREF(0x00202020));
        FillRect(hdc, &rc, bg);
        let _ = DeleteObject(bg.into());

        let dot = CreateSolidBrush(color);
        let old_brush = SelectObject(hdc, dot.into());
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let d = h * 2 / 5;
        let _ = Ellipse(hdc, h / 2 - d / 2, (h - d) / 2, h / 2 + d / 2, (h + d) / 2);
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        let _ = DeleteObject(dot.into());

        let font = CreateFontW(
            -(h * 2 / 5),
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
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
        let old_font = SelectObject(hdc, font.into());
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0x00F0F0F0));
        let mut text_rc = RECT {
            left: h,
            top: 0,
            right: rc.right - h / 2,
            bottom: h,
        };
        let mut wtext = wide(&text);
        let len = wtext.len() - 1;
        DrawTextW(
            hdc,
            &mut wtext[..len],
            &mut text_rc,
            DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
        let _ = EndPaint(hwnd, &ps);
        LRESULT(0)
    }
}
