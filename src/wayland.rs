//! Wayland lets no program grab a key or type into another program's window. On GNOME the hotkey
//! is therefore a GNOME custom shortcut that runs `wspr toggle`, and Ctrl+V goes through GNOME's
//! remote control interface. The windows themselves run through XWayland,
//! where the overlay can still place itself and stay on top.

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::OnceLock;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

static WAYLAND: OnceLock<bool> = OnceLock::new();

/// Call before any other thread starts: it changes the environment.
pub fn init() {
    let on = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland");
    if on && std::env::var_os("DISPLAY").is_some() {
        // SAFETY: called first thing in main, while the process has a single thread.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
    let _ = WAYLAND.set(on);
}

pub fn session() -> bool {
    WAYLAND.get().copied().unwrap_or(false)
}

// ---------- `wspr toggle` ----------

fn socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(crate::platform::data_dir)
        .join("wspr.sock")
}

/// Run by the GNOME shortcut: tells the running wspr to start or stop recording.
pub fn send_toggle() {
    if let Ok(mut s) = UnixStream::connect(socket_path()) {
        let _ = s.write_all(b"t");
    }
}

/// Calls `on_toggle` for each `wspr toggle`. Only the single running instance calls this.
pub fn listen(on_toggle: impl Fn() + Send + 'static) -> Result<(), String> {
    let path = socket_path();
    let _ = std::fs::remove_file(&path); // left over from a crash
    let listener = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            if conn.is_ok() {
                on_toggle();
            }
        }
    });
    Ok(())
}

// ---------- GNOME shortcut ----------

const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const KEY_PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/wspr/";

fn gsettings(args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("gsettings")
        .args(args)
        .output()
        .map_err(|e| format!("gsettings: {e}"))?;
    if !out.status.success() {
        return Err(format!("gsettings: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A string in GVariant text format, as gsettings expects it.
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The GNOME shortcut list with ours added, or None when it is already there.
fn with_ours(list: &str) -> Option<String> {
    if list.contains(KEY_PATH) {
        return None;
    }
    let items = list.trim_start_matches("@as").trim();
    let items = items.trim_start_matches('[').trim_end_matches(']').trim();
    Some(if items.is_empty() {
        format!("['{KEY_PATH}']")
    } else {
        format!("[{items}, '{KEY_PATH}']")
    })
}

/// Sets the GNOME shortcut that runs `wspr toggle`; None turns it off (while Settings are open).
pub fn set_shortcut(hotkey: Option<&str>) -> Result<(), String> {
    if !std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.contains("GNOME")) {
        return Err("On Wayland the wspr hotkey works only on GNOME for now.".into());
    }
    let binding = match hotkey {
        Some(h) => crate::hotkey::to_gtk(h)?,
        None => String::new(),
    };
    if let Some(list) = with_ours(&gsettings(&["get", SCHEMA, "custom-keybindings"])?) {
        gsettings(&["set", SCHEMA, "custom-keybindings", &list])?;
    }
    // The installed path, not current_exe(): an update replaces the file under the running process.
    let exe = crate::platform::linux_bin().ok_or("HOME is not set")?;
    let schema = format!("{SCHEMA}.custom-keybinding:{KEY_PATH}");
    gsettings(&["set", &schema, "name", "'wspr'"])?;
    let command = format!("\"{}\" toggle", exe.display());
    gsettings(&["set", &schema, "command", &quoted(&command)])?;
    gsettings(&["set", &schema, "binding", &quoted(&binding)])?;
    Ok(())
}

// ---------- paste ----------

const MUTTER: &str = "org.gnome.Mutter.RemoteDesktop";

/// Presses Ctrl+V through GNOME's own remote control interface, the one GNOME Remote Desktop
/// uses. Unlike the RemoteDesktop portal it shows no permission dialog. Key codes are positions
/// (Linux evdev), so any keyboard layout works.
pub fn ctrl_v() -> Result<(), String> {
    const CTRL: u32 = 29;
    const V: u32 = 47;
    let err = |e: zbus::Error| format!("Paste: {e}");
    // The session lives as long as this connection.
    let conn = Connection::session().map_err(err)?;
    let rd = Proxy::new(&conn, MUTTER, "/org/gnome/Mutter/RemoteDesktop", MUTTER).map_err(err)?;
    let path: OwnedObjectPath = rd.call("CreateSession", &()).map_err(err)?;
    let session = Proxy::new(&conn, MUTTER, path.as_str(), "org.gnome.Mutter.RemoteDesktop.Session")
        .map_err(err)?;
    session.call::<_, _, ()>("Start", &()).map_err(err)?;
    let mut result = Ok(());
    for (key, down) in [(CTRL, true), (V, true), (V, false), (CTRL, false)] {
        // Keep going after an error, so Ctrl is never left held down.
        if let Err(e) = session.call::<_, _, ()>("NotifyKeyboardKeycode", &(key, down)) {
            result = Err(err(e));
        }
    }
    let _ = session.call::<_, _, ()>("Stop", &());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_our_shortcut_once() {
        assert_eq!(with_ours("@as []").unwrap(), format!("['{KEY_PATH}']"));
        assert_eq!(
            with_ours("['/a/']").unwrap(),
            format!("['/a/', '{KEY_PATH}']")
        );
        assert_eq!(with_ours(&format!("['/a/', '{KEY_PATH}']")), None);
        assert_eq!(quoted("it's"), "'it\\'s'");
    }
}
