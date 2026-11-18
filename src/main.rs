#![cfg_attr(windows, windows_subsystem = "windows")]

mod audio;
mod history;
mod hotkey;
mod openai;
mod settings;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("wspr runs on Windows only. On other systems only `cargo test` is useful.");
}
