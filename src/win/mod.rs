//! Windows app: hidden main window, tray icon, hotkey, recording state machine.
//!
//! All UI state lives on the main thread in `APP`. Only the HTTP request runs on a worker
//! thread, which posts its result back with WM_DONE. Never call Win32 functions that send
//! messages (menus, message boxes, window creation) inside `with`, or the RefCell double-borrows.

mod overlay;
mod recorder;
mod settings_ui;
mod sys;

use crate::history::{self, Entry};
use crate::settings::{self, Settings};
use crate::{audio, hotkey, openai};
use overlay::View;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Instant;
use sys::Sound;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const WM_TRAY: u32 = WM_APP + 1;
const WM_DONE: u32 = WM_APP + 2;
const HOTKEY_ID: i32 = 1;
const T_TICK: usize = 1;
const T_HIDE: usize = 2;
const T_RESTORE: usize = 3;
const MAX_SECONDS: f32 = 600.0; // keeps the WAV under the API's 25 MB limit
const MIN_SECONDS: f32 = 0.3;
const ID_RETRY: usize = 10;
const ID_OPEN_HISTORY: usize = 11;
const ID_SETTINGS: usize = 12;
const ID_QUIT: usize = 13;
const ID_HISTORY: usize = 1000;
const ICON_IDLE: usize = 0;
const ICON_REC: usize = 1;
const ICON_BUSY: usize = 2;

enum State {
    Idle,
    Recording(recorder::Recording),
    Transcribing,
}

struct App {
    hwnd: HWND,
    overlay: HWND,
    hinst: HINSTANCE,
    scale: f32,
    settings: Settings,
    api_key: String,
    history: Vec<Entry>,
    state: State,
    restore_clip: Option<String>,
    icons: [HICON; 3],
    icon: usize,
    taskbar_created: u32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|a| f(a.borrow_mut().as_mut().expect("app not initialised")))
}

fn data_dir() -> PathBuf {
    let d = PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join("wspr");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}
fn history_path() -> PathBuf {
    data_dir().join("history.json")
}
fn failed_path() -> PathBuf {
    data_dir().join("failed.wav")
}

fn message(text: &str) {
    let t = sys::wide(text);
    unsafe {
        MessageBoxW(None, PCWSTR(t.as_ptr()), w!("wspr"), MB_OK | MB_ICONWARNING);
    }
}

pub fn run() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _mutex = CreateMutexW(None, true, w!("Local\\wspr-single-instance"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            message("wspr juba töötab (vaata salveikooni).");
            return;
        }
        let hinst: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: w!("wsprMain"),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("wsprMain"),
            w!("wspr"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst),
            None,
        ) {
            Ok(h) => h,
            Err(e) => return message(&format!("Akna loomine ebaõnnestus: {e}")),
        };
        let settings = Settings::load(&settings_path());
        let api_key = settings::from_hex(&settings.api_key_enc)
            .and_then(|b| sys::unprotect(&b))
            .unwrap_or_default();
        let app = App {
            hwnd,
            overlay: overlay::create(hinst),
            hinst,
            scale: GetDpiForSystem() as f32 / 96.0,
            settings,
            api_key,
            history: history::load(&history_path()),
            state: State::Idle,
            restore_clip: None,
            icons: [
                sys::circle_icon(110, 130, 160),
                sys::circle_icon(230, 60, 60),
                sys::circle_icon(230, 160, 20),
            ],
            icon: ICON_IDLE,
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
        };
        APP.with(|a| *a.borrow_mut() = Some(app));
        tray(NIM_ADD, None);

        let hotkey_err = register_hotkey().err();
        if let Some(e) = &hotkey_err {
            message(e);
        }
        if hotkey_err.is_some() || with(|a| a.api_key.is_empty()) {
            open_settings();
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if let Some(s) = settings_ui::hwnd()
                && IsDialogMessageW(s, &msg).as_bool()
            {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn tray(op: NOTIFY_ICON_MESSAGE, icon: Option<usize>) {
    let (hwnd, hicon, tip) = with(|a| {
        if let Some(i) = icon {
            a.icon = i;
        }
        (
            a.hwnd,
            a.icons[a.icon],
            format!("wspr: {} alustab ja lõpetab", a.settings.hotkey),
        )
    });
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: hicon,
        ..Default::default()
    };
    let t = sys::wide(&tip);
    let n = t.len().min(nid.szTip.len() - 1);
    nid.szTip[..n].copy_from_slice(&t[..n]);
    unsafe {
        let _ = Shell_NotifyIconW(op, &nid);
    }
}

fn register_hotkey() -> Result<(), String> {
    let (hwnd, hk) = with(|a| (a.hwnd, a.settings.hotkey.clone()));
    let (mods, vk) = hotkey::parse(&hk)?;
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
        RegisterHotKey(
            Some(hwnd),
            HOTKEY_ID,
            HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT,
            vk,
        )
        .map_err(|_| {
            format!("Kiirklahv {hk} on juba mõne teise programmi kasutuses. Vali seadetes teine.")
        })
    }
}

/// Called by the settings window on Save.
pub(crate) fn apply_settings(mut new: Settings, api_key: String) -> Result<(), String> {
    hotkey::parse(&new.hotkey)?;
    let key = api_key.trim().to_string();
    new.api_key_enc = if key.is_empty() {
        String::new()
    } else {
        settings::to_hex(&sys::protect(&key).ok_or("API-võtme krüpteerimine ebaõnnestus")?)
    };
    new.save(&settings_path())
        .map_err(|e| format!("Seadete salvestamine ebaõnnestus: {e}"))?;
    with(|a| {
        a.settings = new;
        a.api_key = key;
    });
    tray(NIM_MODIFY, None);
    register_hotkey()
}

fn open_settings() {
    let (hinst, s, key, scale) =
        with(|a| (a.hinst, a.settings.clone(), a.api_key.clone(), a.scale));
    settings_ui::open(hinst, &s, &key, scale);
}

fn show(v: View, hide_ms: u32) {
    let (hwnd, ov, scale) = with(|a| (a.hwnd, a.overlay, a.scale));
    unsafe {
        let _ = KillTimer(Some(hwnd), T_HIDE);
        overlay::show(ov, v, scale);
        if hide_ms > 0 {
            SetTimer(Some(hwnd), T_HIDE, hide_ms, None);
        }
    }
}

fn fail(e: String) {
    sys::play(Sound::Error);
    tray(NIM_MODIFY, Some(ICON_IDLE));
    show(View::Error(e), 6000);
}

fn toggle() {
    enum Act {
        Start,
        Stop,
        Busy,
        NeedKey,
    }
    let act = with(|a| match a.state {
        State::Idle if a.api_key.is_empty() => Act::NeedKey,
        State::Idle => Act::Start,
        State::Recording(_) => Act::Stop,
        State::Transcribing => Act::Busy,
    });
    match act {
        Act::NeedKey => open_settings(),
        Act::Start => start(),
        Act::Stop => stop(),
        Act::Busy => sys::play(Sound::Error),
    }
}

fn start() {
    match recorder::start() {
        Ok(r) => {
            let hwnd = with(|a| {
                a.state = State::Recording(r);
                a.hwnd
            });
            sys::play(Sound::Start);
            tray(NIM_MODIFY, Some(ICON_REC));
            show(View::Recording(Instant::now()), 0);
            unsafe {
                SetTimer(Some(hwnd), T_TICK, 1000, None);
            }
        }
        Err(e) => fail(e),
    }
}

fn stop() {
    let (hwnd, rec) = with(|a| match std::mem::replace(&mut a.state, State::Idle) {
        State::Recording(r) => (a.hwnd, Some(r)),
        other => {
            a.state = other;
            (a.hwnd, None)
        }
    });
    unsafe {
        let _ = KillTimer(Some(hwnd), T_TICK);
    }
    let Some(rec) = rec else { return };
    sys::play(Sound::Stop);
    let (channels, rate) = (rec.channels, rec.rate);
    let samples = rec.finish();
    if (samples.len() as f32) < MIN_SECONDS * rate as f32 * channels as f32 {
        tray(NIM_MODIFY, Some(ICON_IDLE));
        show(View::Hidden, 0);
        return;
    }
    let wav = audio::wav(
        &audio::to_mono_16k(&samples, channels, rate),
        audio::TARGET_RATE,
    );
    transcribe(wav);
}

fn transcribe(wav: Vec<u8>) {
    let (hwnd, key, model, langs, prompt) = with(|a| {
        a.state = State::Transcribing;
        let s = &a.settings;
        (
            a.hwnd.0 as isize,
            a.api_key.clone(),
            s.model.clone(),
            s.language_list(),
            s.prompt.clone(),
        )
    });
    tray(NIM_MODIFY, Some(ICON_BUSY));
    show(View::Transcribing, 0);
    std::thread::spawn(move || {
        let result = openai::transcribe(&key, &model, &langs, &prompt, &wav);
        let p = Box::into_raw(Box::new((result, wav)));
        unsafe {
            if PostMessageW(
                Some(HWND(hwnd as *mut _)),
                WM_DONE,
                WPARAM(0),
                LPARAM(p as isize),
            )
            .is_err()
            {
                drop(Box::from_raw(p));
            }
        }
    });
}

fn on_result(result: Result<String, String>, wav: Vec<u8>) {
    with(|a| a.state = State::Idle);
    tray(NIM_MODIFY, Some(ICON_IDLE));
    match result {
        Ok(text) if text.is_empty() => show(View::Info("Kõnet ei tuvastatud".into()), 2500),
        Ok(text) => {
            let _ = std::fs::remove_file(failed_path());
            let saved = with(|a| {
                history::push(
                    &mut a.history,
                    Entry {
                        time: sys::now(),
                        text: text.clone(),
                    },
                );
                history::save(&history_path(), &a.history)
            });
            match (paste(&text), saved) {
                (Err(e), _) => fail(format!("{e}. Tekst on ajaloos.")),
                (Ok(false), Err(e)) => fail(format!(
                    "Kleepida ei saanud ja ajalugu ei salvestunud ({e}). Tekst on lõikelaual"
                )),
                (Ok(false), Ok(())) => show(
                    View::Info("Kleepida ei saanud, tekst on lõikelaual (Ctrl+V)".into()),
                    5000,
                ),
                (Ok(true), Err(e)) => fail(format!(
                    "Kleebitud, aga ajaloo salvestamine ebaõnnestus: {e}"
                )),
                (Ok(true), Ok(())) => show(View::Done, 1200),
            }
        }
        Err(e) => {
            // Keep the audio so the dictation is not lost; the tray menu offers a retry.
            let kept = history::write_atomic(&failed_path(), &wav).is_ok();
            fail(if kept {
                format!("{e}. Salveikooni menüüst saab uuesti proovida.")
            } else {
                e
            });
        }
    }
}

/// Ok(false): the text is on the clipboard but Ctrl+V could not be sent.
fn paste(text: &str) -> Result<bool, String> {
    let prev = sys::get_text();
    sys::set_text(text)?;
    if !sys::send_ctrl_v() {
        return Ok(false); // leave the dictation on the clipboard for a manual paste
    }
    // shortcut: only text clipboard content is restored; an image on the clipboard is replaced by the dictation.
    if let Some(p) = prev {
        let hwnd = with(|a| {
            a.restore_clip = Some(p);
            a.hwnd
        });
        unsafe {
            SetTimer(Some(hwnd), T_RESTORE, 600, None);
        }
    }
    Ok(true)
}

fn tick() {
    let (hwnd, ov, secs) = with(|a| {
        let s = match &a.state {
            State::Recording(r) => Some(r.seconds()),
            _ => None,
        };
        (a.hwnd, a.overlay, s)
    });
    match secs {
        Some(s) if s >= MAX_SECONDS => stop(),
        Some(_) => overlay::repaint(ov),
        None => unsafe {
            let _ = KillTimer(Some(hwnd), T_TICK);
        },
    }
}

fn menu() {
    let (hwnd, labels) = with(|a| {
        (
            a.hwnd,
            a.history
                .iter()
                .take(history::IN_MENU)
                .map(history::menu_label)
                .collect::<Vec<_>>(),
        )
    });
    let has_failed = failed_path().exists();
    let cmd = unsafe {
        let Ok(m) = CreatePopupMenu() else { return };
        if labels.is_empty() {
            let _ = AppendMenuW(m, MF_STRING | MF_GRAYED, 0, w!("Ajalugu on tühi"));
        } else {
            let _ = AppendMenuW(
                m,
                MF_STRING | MF_GRAYED,
                0,
                w!("Viimased (klõps kopeerib lõikelauale):"),
            );
            for (i, l) in labels.iter().enumerate() {
                let wl = sys::wide(l);
                let _ = AppendMenuW(m, MF_STRING, ID_HISTORY + i, PCWSTR(wl.as_ptr()));
            }
        }
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
        if has_failed {
            let _ = AppendMenuW(
                m,
                MF_STRING,
                ID_RETRY,
                w!("Proovi ebaõnnestunud salvestust uuesti"),
            );
        }
        let _ = AppendMenuW(m, MF_STRING, ID_OPEN_HISTORY, w!("Ava kogu ajalugu"));
        let _ = AppendMenuW(m, MF_STRING, ID_SETTINGS, w!("Seaded…"));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(m, MF_STRING, ID_QUIT, w!("Välju"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Without this the menu does not close when clicking elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            m,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        )
        .0 as usize;
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(m);
        cmd
    };
    match cmd {
        ID_RETRY => retry(),
        ID_OPEN_HISTORY => open_history(),
        ID_SETTINGS => open_settings(),
        ID_QUIT => quit(),
        c if c >= ID_HISTORY => copy_history(c - ID_HISTORY),
        _ => {}
    }
}

fn copy_history(i: usize) {
    let Some(text) = with(|a| a.history.get(i).map(|e| e.text.clone())) else {
        return;
    };
    match sys::set_text(&text) {
        Ok(()) => show(View::Info("Kopeeritud lõikelauale".into()), 1500),
        Err(e) => fail(e),
    }
}

fn retry() {
    if !with(|a| matches!(a.state, State::Idle)) {
        return sys::play(Sound::Error);
    }
    if with(|a| a.api_key.is_empty()) {
        return open_settings();
    }
    match std::fs::read(failed_path()) {
        Ok(wav) => transcribe(wav),
        Err(e) => fail(format!("Salvestust ei saanud lugeda: {e}")),
    }
}

fn open_history() {
    let text = with(|a| history::to_text(&a.history));
    let path = data_dir().join("history.txt");
    if let Err(e) = std::fs::write(&path, text) {
        return fail(format!("Ajaloo faili ei saanud kirjutada: {e}"));
    }
    let p = sys::wide(&path.to_string_lossy());
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(p.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

fn quit() {
    tray(NIM_DELETE, None);
    let hwnd = with(|a| a.hwnd);
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_HOTKEY => toggle(),
            WM_TRAY => {
                let ev = (lp.0 & 0xFFFF) as u32;
                if ev == WM_RBUTTONUP || ev == WM_LBUTTONUP {
                    menu();
                }
            }
            WM_DONE => {
                let b = Box::from_raw(lp.0 as *mut (Result<String, String>, Vec<u8>));
                let (result, wav) = *b;
                on_result(result, wav);
            }
            WM_TIMER => match wp.0 {
                T_TICK => tick(),
                T_HIDE => {
                    let _ = KillTimer(Some(hwnd), T_HIDE);
                    show(View::Hidden, 0);
                }
                T_RESTORE => {
                    let _ = KillTimer(Some(hwnd), T_RESTORE);
                    if let Some(p) = with(|a| a.restore_clip.take()) {
                        let _ = sys::set_text(&p);
                    }
                }
                _ => {}
            },
            WM_DESTROY => PostQuitMessage(0),
            _ => {
                // Explorer restarted: the tray icon is gone and must be added again.
                let tc = APP.with(|a| {
                    a.try_borrow()
                        .ok()
                        .and_then(|a| a.as_ref().map(|a| a.taskbar_created))
                });
                if tc == Some(msg) && msg != 0 {
                    tray(NIM_ADD, None);
                } else {
                    return DefWindowProcW(hwnd, msg, wp, lp);
                }
            }
        }
        LRESULT(0)
    }
}
