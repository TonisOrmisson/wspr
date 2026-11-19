#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(any(windows, target_os = "linux"))]
mod app;
mod applog;
mod audio;
mod history;
mod hotkey;
#[cfg(any(windows, target_os = "linux"))]
mod platform;
#[cfg(any(windows, target_os = "linux"))]
mod recorder;
mod settings;
#[cfg(any(windows, target_os = "linux"))]
mod settings_view;
mod stt;
mod update;
#[cfg(target_os = "linux")]
mod wayland;

#[cfg(any(windows, target_os = "linux"))]
fn main() {
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("toggle") {
        return wayland::send_toggle();
    }
    app::run();
}

#[cfg(not(any(windows, target_os = "linux")))]
fn main() {
    eprintln!("wspr runs on Windows and Linux.");
}
