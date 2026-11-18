#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
mod applog;
mod audio;
mod history;
mod hotkey;
#[cfg(windows)]
mod platform;
#[cfg(windows)]
mod recorder;
mod settings;
#[cfg(windows)]
mod settings_view;
mod stt;
mod update;

#[cfg(windows)]
fn main() {
    app::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("wspr runs on Windows only for now. On other systems only `cargo test` is useful.");
}
