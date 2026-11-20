//! Wayland lets no program grab a key or type into another program's window. On GNOME the hotkey
//! is therefore a GNOME custom shortcut that runs `wspr toggle`, and Ctrl+V goes through GNOME's
//! remote control interface. On KDE the hotkey is registered with KDE's shortcut service and
//! Ctrl+V goes through the RemoteDesktop portal. The windows themselves run through XWayland,
//! where the overlay can still place itself and stay on top.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

static WAYLAND: OnceLock<bool> = OnceLock::new();

pub fn init() {
    let on = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland");
    let _ = WAYLAND.set(on);
}

/// Windows go through XWayland when it is there. WAYLAND_DISPLAY stays set, so the clipboard
/// is set through Wayland: KWin ignores clipboard changes from an X11 window without focus.
pub fn use_x11() -> bool {
    session() && std::env::var_os("DISPLAY").is_some()
}

pub fn session() -> bool {
    WAYLAND.get().copied().unwrap_or(false)
}

fn desktop_is(name: &str) -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|p| p == name))
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

/// Calls `on_toggle` for each `wspr toggle` (GNOME) or hotkey press (KDE). Only the single
/// running instance calls this.
pub fn listen(on_toggle: impl Fn() + Send + Sync + 'static) -> Result<(), String> {
    let on_toggle = Arc::new(on_toggle);
    if desktop_is("KDE") {
        let on_kde = on_toggle.clone();
        std::thread::spawn(move || kde_listen(&*on_kde));
        // Ask for the paste permission now rather than in the middle of the first dictation.
        std::thread::spawn(|| {
            // At login the portal may still be starting up: try again a little later.
            for attempt in 1..=3 {
                match paster() {
                    Err(e) if attempt < 3 && e.contains("activate") => {
                        std::thread::sleep(std::time::Duration::from_secs(10))
                    }
                    Err(e) => return crate::app::log(format!("ERROR {e}")),
                    Ok(_) => return,
                }
            }
        });
    }
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
        return Err(format!(
            "gsettings: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
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

/// Sets the desktop's shortcut for wspr; None turns it off (while Settings are open).
pub fn set_shortcut(hotkey: Option<&str>) -> Result<(), String> {
    if desktop_is("KDE") {
        return kde_shortcut(hotkey);
    }
    if !desktop_is("GNOME") {
        return Err("On Wayland the wspr hotkey works only on GNOME and KDE for now.".into());
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

// ---------- KDE shortcut ----------

const KGLOBALACCEL: &str = "org.kde.kglobalaccel";
/// Component and action, as unique names and as shown in KDE's shortcut settings.
const ACTION: [&str; 4] = ["wspr", "toggle", "wspr", "Start or stop dictation"];

fn kde_shortcut(hotkey: Option<&str>) -> Result<(), String> {
    let keys = match hotkey {
        Some(h) => vec![crate::hotkey::to_qt(h)?],
        None => vec![],
    };
    let err = |e: zbus::Error| format!("KDE shortcuts: {e}");
    let conn = Connection::session().map_err(err)?;
    let kga =
        Proxy::new(&conn, KGLOBALACCEL, "/kglobalaccel", "org.kde.KGlobalAccel").map_err(err)?;
    kga.call::<_, _, ()>("doRegister", &(&ACTION[..],))
        .map_err(err)?;
    // SetPresent | NoAutoloading: use these keys, not the ones KDE saved earlier.
    let got: Vec<i32> = kga
        .call("setShortcut", &(&ACTION[..], &keys, 6u32))
        .map_err(err)?;
    if got.first() != keys.first() {
        return Err(format!(
            "The hotkey {} is already used by another program. Choose another one in Settings.",
            hotkey.unwrap_or_default()
        ));
    }
    Ok(())
}

fn kde_listen(on_toggle: &dyn Fn()) {
    let messages = (|| -> zbus::Result<_> {
        let conn = Connection::session()?;
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.kde.kglobalaccel.Component")?
            .member("globalShortcutPressed")?
            .build();
        MessageIterator::for_match_rule(rule, &conn, None)
    })();
    let messages = match messages {
        Ok(m) => m,
        Err(e) => return crate::app::log(format!("ERROR KDE shortcuts: {e}")),
    };
    for msg in messages.flatten() {
        if let Ok((component, action, _)) = msg.body().deserialize::<(String, String, i64)>()
            && component == ACTION[0]
            && action == ACTION[1]
        {
            on_toggle();
        }
    }
}

// ---------- paste ----------

const MUTTER: &str = "org.gnome.Mutter.RemoteDesktop";
/// Left Ctrl and V as Linux evdev key codes, pressed and released.
const CTRL_V: [(u32, bool); 4] = [(29, true), (47, true), (47, false), (29, false)];

/// Presses Ctrl+V through GNOME's own remote control interface, the one GNOME Remote Desktop
/// uses. Unlike the RemoteDesktop portal it shows no permission dialog. Key codes are positions
/// (Linux evdev), so any keyboard layout works.
pub fn ctrl_v() -> Result<(), String> {
    if desktop_is("KDE") {
        return kde_ctrl_v();
    }
    let err = |e: zbus::Error| format!("Paste: {e}");
    // The session lives as long as this connection.
    let conn = Connection::session().map_err(err)?;
    let rd = Proxy::new(&conn, MUTTER, "/org/gnome/Mutter/RemoteDesktop", MUTTER).map_err(err)?;
    let path: OwnedObjectPath = rd.call("CreateSession", &()).map_err(err)?;
    let session = Proxy::new(
        &conn,
        MUTTER,
        path.as_str(),
        "org.gnome.Mutter.RemoteDesktop.Session",
    )
    .map_err(err)?;
    session.call::<_, _, ()>("Start", &()).map_err(err)?;
    let mut result = Ok(());
    for (key, down) in CTRL_V {
        // Keep going after an error, so Ctrl is never left held down.
        if let Err(e) = session.call::<_, _, ()>("NotifyKeyboardKeycode", &(key, down)) {
            result = Err(err(e));
        }
    }
    let _ = session.call::<_, _, ()>("Stop", &());
    result
}

// ---------- KDE paste through the RemoteDesktop portal ----------

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const REMOTE_DESKTOP: &str = "org.freedesktop.portal.RemoteDesktop";

struct Paster {
    conn: Connection,
    session: OwnedObjectPath,
}

type Options<'a> = HashMap<&'a str, Value<'a>>;

static PASTER: Mutex<Option<Paster>> = Mutex::new(None);

fn token_path() -> PathBuf {
    crate::platform::data_dir().join("remote-desktop-token")
}

/// Opens the portal session once and keeps it: KDE asks the user to allow it.
fn paster() -> Result<std::sync::MutexGuard<'static, Option<Paster>>, String> {
    let mut guard = PASTER.lock().map_err(|_| "Paste: lock")?;
    if guard.is_none() {
        *guard = Some(Paster::connect()?);
        crate::app::log("Remote control for pasting allowed");
    }
    Ok(guard)
}

fn kde_ctrl_v() -> Result<(), String> {
    let mut guard = paster()?;
    let result = guard.as_ref().map_or(Ok(()), |p| p.ctrl_v());
    if result.is_err() {
        *guard = None; // the session was closed: open a new one next time
    }
    result
}

impl Paster {
    fn connect() -> Result<Self, String> {
        let err = |e: zbus::Error| format!("Remote control: {e}");
        let conn = Connection::session().map_err(err)?;
        let portal = Proxy::new(&conn, PORTAL, PORTAL_PATH, REMOTE_DESKTOP).map_err(err)?;
        let res = request(&conn, &portal, "CreateSession", |t| {
            (Options::from([
                ("handle_token", t),
                ("session_handle_token", "wspr".into()),
            ]),)
        })?;
        let session = res
            .get("session_handle")
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .ok_or("Remote control: no session")?;
        let session = OwnedObjectPath::try_from(session).map_err(|e| e.to_string())?;
        let restore = std::fs::read_to_string(token_path()).unwrap_or_default();
        request(&conn, &portal, "SelectDevices", |t| {
            let mut o = Options::from([
                ("handle_token", t),
                ("types", 1u32.into()),        // keyboard
                ("persist_mode", 2u32.into()), // until revoked
            ]);
            if !restore.is_empty() {
                o.insert("restore_token", restore.as_str().into());
            }
            (ObjectPath::from(&session), o)
        })?;
        let res = request(&conn, &portal, "Start", |t| {
            (
                ObjectPath::from(&session),
                "",
                Options::from([("handle_token", t)]),
            )
        })?;
        if let Some(token) = res
            .get("restore_token")
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
        {
            let _ = std::fs::write(token_path(), token);
        }
        Ok(Self { conn, session })
    }

    fn ctrl_v(&self) -> Result<(), String> {
        let portal = Proxy::new(&self.conn, PORTAL, PORTAL_PATH, REMOTE_DESKTOP)
            .map_err(|e| e.to_string())?;
        let mut result = Ok(());
        for (key, down) in CTRL_V {
            let sent: zbus::Result<()> = portal.call(
                "NotifyKeyboardKeycode",
                &(
                    ObjectPath::from(&self.session),
                    Options::new(),
                    key as i32,
                    down as u32,
                ),
            );
            // Keep going after an error, so Ctrl is never left held down.
            if let Err(e) = sent {
                result = Err(format!("Remote control: {e}"));
            }
        }
        result
    }
}

/// Calls a portal method and waits for its answer, which comes as a signal on a request object.
fn request<B>(
    conn: &Connection,
    portal: &Proxy,
    method: &str,
    body: impl FnOnce(Value<'static>) -> B,
) -> Result<HashMap<String, OwnedValue>, String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let err = |e: zbus::Error| format!("Remote control: {e}");
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let token = format!("wspr{}_{n}", std::process::id());
    let sender = conn.unique_name().ok_or("Remote control: no D-Bus name")?;
    let sender = sender.trim_start_matches(':').replace('.', "_");
    let path = format!("{PORTAL_PATH}/request/{sender}/{token}");
    // Subscribe before calling, or a fast answer would be missed.
    let req = Proxy::new(
        conn,
        PORTAL,
        path.as_str(),
        "org.freedesktop.portal.Request",
    )
    .map_err(err)?;
    let mut answers = req.receive_signal("Response").map_err(err)?;
    let _: OwnedObjectPath = portal
        .call(method, &body(Value::from(token)))
        .map_err(err)?;
    let msg = answers.next().ok_or("Remote control: no answer")?;
    let (code, results): (u32, HashMap<String, OwnedValue>) =
        msg.body().deserialize().map_err(err)?;
    match code {
        0 => Ok(results),
        1 => Err("Remote control was not allowed. wspr needs it to paste.".into()),
        _ => Err(format!("Remote control: {method} failed")),
    }
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
