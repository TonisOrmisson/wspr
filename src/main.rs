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

#[cfg(any(windows, target_os = "linux"))]
fn main() {
    app::run();
}

#[cfg(not(any(windows, target_os = "linux")))]
fn main() {
    eprintln!("wspr runs on Windows and Linux.");
}
