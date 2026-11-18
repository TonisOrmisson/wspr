//! The app: tray icon, global hotkey, recording state machine, status overlay and settings
//! window. The overlay is eframe's root window: always on top, click-through, and moved off
//! screen when idle (hiding it would let Windows give it focus when it shows again). It is
//! opaque with rounded corners cut by the system: a transparent GL window showed empty on Windows.

use crate::history::{self, Entry};
use crate::platform::{self, Sound};
use crate::settings::Settings;
use crate::settings_view::{Action, Form};
use crate::{applog, audio, hotkey, recorder, stt, update};
use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_SECONDS: f32 = 600.0; // keeps the WAV under the APIs' 25 MB limit
const MIN_SECONDS: f32 = 0.3;
const UPDATE_FIRST: Duration = Duration::from_secs(15);
const UPDATE_EVERY: Duration = Duration::from_secs(60 * 60);
const UPDATE_RETRY: Duration = Duration::from_secs(10 * 60); // an update waits for dictation to finish
const PILL: egui::Vec2 = egui::vec2(240.0, 40.0);
const PILL_WIDE: egui::Vec2 = egui::vec2(600.0, 40.0);
const PILL_BG: egui::Color32 = egui::Color32::from_rgb(32, 32, 32);
/// Linux draws the pill rounded in a transparent, click-through window. On Windows that kind of
/// window is layered, which OpenGL cannot draw into, so there it is opaque with a system region.
const TRANSPARENT: bool = cfg!(target_os = "linux");

/// Result, the audio (kept on failure), request time in ms, and what was sent (for the log).
type DoneResult = (Result<String, String>, Vec<u8>, u128, String);
type UpdateResult = (bool, Result<Option<(String, Vec<u8>)>, String>);

enum Msg {
    Hotkey(u32),
    Menu(String),
    Done(DoneResult),
    Update(UpdateResult),
}

enum State {
    Idle,
    Recording(recorder::Recording),
    Transcribing,
}

#[derive(Clone, PartialEq)]
enum Overlay {
    Hidden,
    Recording(Instant),
    Transcribing,
    Done,
    Info(String),
    Error(String),
}

fn data_path(name: &str) -> PathBuf {
    platform::data_dir().join(name)
}

/// One line in wspr.log. Never contains dictated text or API keys.
fn log(msg: impl AsRef<str>) {
    let _ = applog::append(
        &data_path("wspr.log"),
        &format!("{} {}", platform::timestamp(), msg.as_ref()),
    );
}

fn window_icon() -> egui::IconData {
    egui::IconData {
        rgba: include_bytes!("../res/icon-64.rgba").to_vec(),
        width: 64,
        height: 64,
    }
}

pub fn run() {
    let Some(_instance) = platform::single_instance() else {
        platform::alert("wspr is already running (see the tray icon).");
        return;
    };
    let viewport = egui::ViewportBuilder::default()
        .with_title("wspr")
        .with_app_id("wspr")
        .with_icon(window_icon())
        .with_decorations(false)
        .with_transparent(TRANSPARENT)
        .with_mouse_passthrough(TRANSPARENT)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_taskbar(false)
        .with_active(false)
        .with_resizable(false)
        .with_inner_size(PILL)
        .with_position([-20000.0, -20000.0]);
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    if let Err(e) = eframe::run_native("wspr", options, Box::new(|cc| Ok(Box::new(App::new(cc))))) {
        platform::alert(&format!("wspr could not start: {e}"));
    }
}

struct App {
    ctx: egui::Context,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    settings: Settings,
    keys: [String; 2],
    history: Vec<Entry>,
    state: State,
    overlay: Overlay,
    overlay_until: Option<Instant>,
    /// What the native window currently shows: None = off screen, Some(wide).
    placed: Option<bool>,
    styled: bool,
    restore_clip: Option<(String, Instant)>,
    next_update: Instant,
    checking_update: bool,
    hotkeys: Option<GlobalHotKeyManager>,
    hotkey: Option<HotKey>,
    tray: Option<TrayIcon>,
    /// Idle, recording, transcribing; by index, so one that fails to load shifts nothing.
    tray_icons: [Option<Icon>; 3],
    menu_has_retry: Option<bool>,
    clipboard: Option<arboard::Clipboard>,
    enigo: Option<enigo::Enigo>,
    form: Option<Form>,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        let ctx = cc.egui_ctx.clone();
        let (tx, rx) = channel();
        let (t, c) = (tx.clone(), ctx.clone());
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            if e.state == HotKeyState::Pressed {
                let _ = t.send(Msg::Hotkey(e.id));
                c.request_repaint();
            }
        }));
        let (t, c) = (tx.clone(), ctx.clone());
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = t.send(Msg::Menu(e.id.0));
            c.request_repaint();
        }));

        let settings = Settings::load(&data_path("settings.json"));
        let keys = [0, 1].map(|p| platform::load_key(stt::PROVIDERS[p].id, settings.key_enc_of(p)));
        let icon = |rgba: Vec<u8>| Icon::from_rgba(rgba, 32, 32).ok();
        let tray_icons = [
            icon(include_bytes!("../res/icon-32.rgba").to_vec())
                .or_else(|| icon(circle_rgba([110, 130, 160]))),
            icon(circle_rgba([230, 60, 60])),
            icon(circle_rgba([230, 160, 20])),
        ];
        let mut app = Self {
            ctx,
            tx,
            rx,
            history: history::load(&data_path("history.json")),
            settings,
            keys,
            state: State::Idle,
            overlay: Overlay::Hidden,
            overlay_until: None,
            placed: None,
            styled: false,
            restore_clip: None,
            next_update: Instant::now() + UPDATE_FIRST,
            checking_update: false,
            hotkeys: GlobalHotKeyManager::new().ok(),
            hotkey: None,
            tray: None,
            tray_icons,
            menu_has_retry: None,
            clipboard: arboard::Clipboard::new().ok(),
            enigo: enigo::Enigo::new(&enigo::Settings::default()).ok(),
            form: None,
        };
        let mut builder = TrayIconBuilder::new()
            .with_tooltip(app.tooltip())
            .with_menu_on_left_click(true);
        if let Some(idle) = app.tray_icons[0].clone() {
            builder = builder.with_icon(idle);
        }
        app.tray = builder.build().ok();
        app.refresh_menu();
        log(format!("wspr {VERSION} started"));
        app.announce_update();
        match platform::ensure_installed() {
            Ok(Some(msg)) => {
                log(&msg);
                app.show(Overlay::Info(msg), Some(Duration::from_secs(6)));
            }
            Ok(None) => {}
            Err(e) => log(format!("ERROR Install: {e}")),
        }
        let hotkey_err = app.register_hotkey().err();
        if let Some(e) = &hotkey_err {
            app.fail(e.clone());
        }
        if hotkey_err.is_some() || app.active_key().is_empty() {
            app.open_settings();
        }
        app
    }

    fn active_key(&self) -> &str {
        &self.keys[self.settings.provider_index()]
    }

    fn tooltip(&self) -> String {
        format!(
            "wspr {VERSION}: {} starts and stops dictation",
            self.settings.hotkey
        )
    }

    fn set_tray_icon(&self, i: usize) {
        if let (Some(tray), Some(Some(icon))) = (&self.tray, self.tray_icons.get(i)) {
            let _ = tray.set_icon(Some(icon.clone()));
        }
    }

    /// The tray menu offers Retry only while a failed recording waits.
    fn refresh_menu(&mut self) {
        let has_retry = data_path("failed.wav").exists();
        if self.menu_has_retry == Some(has_retry) {
            return;
        }
        self.menu_has_retry = Some(has_retry);
        let menu = Menu::new();
        if has_retry {
            let _ = menu.append(&MenuItem::with_id(
                "retry",
                "Retry failed recording",
                true,
                None,
            ));
            let _ = menu.append(&PredefinedMenuItem::separator());
        }
        let _ = menu.append(&MenuItem::with_id(
            "update",
            "Check for updates",
            true,
            None,
        ));
        let _ = menu.append(&MenuItem::with_id("log", "View log", true, None));
        let _ = menu.append(&MenuItem::with_id("settings", "Settings…", true, None));
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&MenuItem::with_id("quit", "Quit", true, None));
        if let Some(tray) = &self.tray {
            tray.set_menu(Some(Box::new(menu)));
        }
    }

    fn show(&mut self, o: Overlay, hide_after: Option<Duration>) {
        self.overlay = o;
        self.overlay_until = hide_after.map(|d| Instant::now() + d);
        self.ctx.request_repaint();
    }

    fn fail(&mut self, e: String) {
        log(format!("ERROR {e}"));
        platform::play(Sound::Error);
        self.set_tray_icon(0);
        self.show(Overlay::Error(e), Some(Duration::from_secs(6)));
    }

    fn announce_update(&mut self) {
        let file = data_path("version.txt");
        let updated = match std::fs::read_to_string(&file) {
            Ok(prev) => prev.trim() != VERSION,
            // Versions before 0.2.2 did not write the file; existing settings mean it is not a first install.
            Err(_) => data_path("settings.json").exists(),
        };
        let _ = std::fs::write(&file, VERSION);
        if updated {
            log(format!("Updated to {VERSION}"));
            self.show(
                Overlay::Info(format!("wspr updated to {VERSION}")),
                Some(Duration::from_secs(5)),
            );
        }
    }

    // ---------- hotkey ----------

    fn register_hotkey(&mut self) -> Result<(), String> {
        self.suspend_hotkey();
        let manager = self
            .hotkeys
            .as_ref()
            .ok_or("Global hotkeys are not available on this system")?;
        let accel = hotkey::to_accelerator(&self.settings.hotkey)?;
        let hk: HotKey = accel
            .parse()
            .map_err(|e| format!("Hotkey {}: {e}", self.settings.hotkey))?;
        manager.register(hk).map_err(|_| {
            format!(
                "The hotkey {} is already used by another program. Choose another one in Settings.",
                self.settings.hotkey
            )
        })?;
        self.hotkey = Some(hk);
        Ok(())
    }

    fn suspend_hotkey(&mut self) {
        if let (Some(m), Some(hk)) = (&self.hotkeys, self.hotkey.take()) {
            let _ = m.unregister(hk);
        }
    }

    // ---------- dictation ----------

    fn toggle(&mut self) {
        match self.state {
            State::Idle if self.active_key().is_empty() => self.open_settings(),
            State::Idle => self.start(),
            State::Recording(_) => self.stop(),
            State::Transcribing => platform::play(Sound::Error),
        }
    }

    fn start(&mut self) {
        match recorder::start() {
            Ok(r) => {
                let now = Instant::now();
                self.state = State::Recording(r);
                log("Recording started");
                platform::play(Sound::Start);
                self.set_tray_icon(1);
                self.show(Overlay::Recording(now), None);
            }
            Err(e) => self.fail(e),
        }
    }

    fn stop(&mut self) {
        let State::Recording(rec) = std::mem::replace(&mut self.state, State::Idle) else {
            return;
        };
        platform::play(Sound::Stop);
        let (channels, rate) = (rec.channels, rec.rate);
        let samples = rec.finish();
        let seconds = samples.len() as f32 / (rate.max(1) as f32 * channels.max(1) as f32);
        log(format!(
            "Recording stopped after {seconds:.1} s ({rate} Hz, {channels} ch)"
        ));
        if seconds < MIN_SECONDS {
            log("Too short, discarded");
            self.set_tray_icon(0);
            self.show(Overlay::Hidden, None);
            return;
        }
        let wav = audio::wav(
            &audio::to_mono_16k(&samples, channels, rate),
            audio::TARGET_RATE,
        );
        self.transcribe(wav);
    }

    fn transcribe(&mut self, wav: Vec<u8>) {
        self.state = State::Transcribing;
        let p = self.settings.provider_index();
        let provider = &stt::PROVIDERS[p];
        let (url, key, model, langs) = (
            provider.url,
            self.keys[p].clone(),
            self.settings.model_of(p).to_string(),
            self.settings.language_list(),
        );
        let lang = if langs.is_empty() {
            "auto".into()
        } else {
            langs.join(",")
        };
        // 16 kHz mono 16-bit WAV: 32000 bytes per second after the 44-byte header.
        let audio_s = wav.len().saturating_sub(44) as f32 / 32000.0;
        let label = format!(
            "{audio_s:.1} s of audio, {} {model}, language {lang}",
            provider.id
        );
        self.set_tray_icon(2);
        self.show(Overlay::Transcribing, None);
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let started = Instant::now();
            let result = stt::transcribe(url, &key, &model, &langs, &wav);
            let _ = tx.send(Msg::Done((
                result,
                wav,
                started.elapsed().as_millis(),
                label,
            )));
            ctx.request_repaint();
        });
    }

    fn on_result(&mut self, (result, wav, ms, label): DoneResult) {
        self.state = State::Idle;
        self.set_tray_icon(0);
        // Request time right after the timestamp, so the log is easy to split into columns.
        match &result {
            Ok(text) => log(format!(
                "({ms} ms) Transcribed {label}, {} characters",
                text.chars().count()
            )),
            Err(_) => log(format!("({ms} ms) Transcription failed, {label}")),
        }
        match result {
            Ok(text) if text.is_empty() => self.show(
                Overlay::Info("No speech detected".into()),
                Some(Duration::from_millis(2500)),
            ),
            Ok(text) => {
                let _ = std::fs::remove_file(data_path("failed.wav"));
                let saved = self.add_history(&text);
                match (self.paste(&text), saved) {
                    (Err(e), _) => self.fail(e),
                    (Ok(false), Err(e)) => self.fail(format!(
                        "Could not paste or save the history ({e}). The text is on the clipboard"
                    )),
                    (Ok(false), Ok(())) => {
                        self.fail("Could not paste. The text is on the clipboard".into())
                    }
                    (Ok(true), Err(e)) => {
                        self.fail(format!("Pasted, but the history could not be saved: {e}"))
                    }
                    (Ok(true), Ok(())) => {
                        self.show(Overlay::Done, Some(Duration::from_millis(1200)))
                    }
                }
            }
            Err(e) => {
                // Keep the audio so the dictation is not lost; the tray menu offers a retry.
                let kept = history::write_atomic(&data_path("failed.wav"), &wav).is_ok();
                self.fail(if kept {
                    format!("{e}. You can retry from the tray menu.")
                } else {
                    e
                });
            }
        }
        self.refresh_menu();
    }

    fn add_history(&mut self, text: &str) -> std::io::Result<()> {
        if self.settings.history_size == 0 {
            return Ok(());
        }
        let entry = Entry {
            time: platform::now(),
            text: text.to_string(),
        };
        history::push(&mut self.history, entry, self.settings.history_size);
        history::save(&data_path("history.json"), &self.history)
    }

    /// Ok(false): the text is on the clipboard but Ctrl+V could not be sent.
    fn paste(&mut self, text: &str) -> Result<bool, String> {
        let clipboard = self
            .clipboard
            .as_mut()
            .ok_or("The clipboard is not available")?;
        let prev = clipboard.get_text().ok();
        clipboard
            .set_text(text)
            .map_err(|e| format!("Clipboard: {e}"))?;
        let Some(enigo) = self.enigo.as_mut() else {
            return Ok(false);
        };
        if !platform::send_paste(enigo) {
            return Ok(false);
        }
        // shortcut: only text clipboard content is restored; an image on the clipboard is replaced by the dictation.
        if let Some(p) = prev {
            self.restore_clip = Some((p, Instant::now() + Duration::from_millis(600)));
        }
        Ok(true)
    }

    fn retry(&mut self) {
        if !matches!(self.state, State::Idle) {
            return platform::play(Sound::Error);
        }
        if self.active_key().is_empty() {
            return self.open_settings();
        }
        match std::fs::read(data_path("failed.wav")) {
            Ok(wav) => {
                log("Retrying the failed recording");
                self.transcribe(wav)
            }
            Err(e) => self.fail(format!("Could not read the recording: {e}")),
        }
    }

    // ---------- settings ----------

    fn open_settings(&mut self) {
        if self.form.is_some() {
            return;
        }
        // The hotkey is off while settings are open, so a running recording could not be stopped.
        if matches!(self.state, State::Recording(_)) {
            self.stop();
        }
        self.suspend_hotkey();
        self.form = Some(Form::new(&self.settings, &self.keys));
    }

    fn close_settings(&mut self) {
        self.form = None;
        if let Err(e) = self.register_hotkey() {
            self.fail(e);
        }
    }

    fn save_settings(&mut self, form: &Form) -> Result<(), String> {
        let mut new = form.to_settings();
        hotkey::parse(&new.hotkey)?;
        let p = form.provider;
        if new.model_of(p).is_empty() {
            return Err("Enter a model name".into());
        }
        // OpenRouter also lists text-to-speech and chat models; catch those before the first dictation.
        // If the model list cannot be fetched (offline), the model is accepted as typed.
        if stt::PROVIDERS[p].id == "openrouter"
            && let Some(models) = stt::openrouter_stt_models()
            && !models.iter().any(|m| m == new.model_of(p))
        {
            let examples: Vec<&str> = models.iter().take(6).map(String::as_str).collect();
            return Err(format!(
                "\"{}\" is not a speech-to-text model on OpenRouter. Some that are: {}",
                new.model_of(p),
                examples.join(", ")
            ));
        }
        let keys = form.keys.clone().map(|k| k.trim().to_string());
        for (i, key) in keys.iter().enumerate() {
            let model = new.model_of(i).to_string();
            new.set_provider_values(i, model, platform::save_key(stt::PROVIDERS[i].id, key)?);
        }
        new.save(&data_path("settings.json"))
            .map_err(|e| format!("Could not save settings: {e}"))?;
        log(format!(
            "Settings saved: provider {}, model {}, language {}, hotkey {}, history {}",
            stt::PROVIDERS[p].id,
            new.model_of(p),
            if new.languages.is_empty() {
                "auto"
            } else {
                &new.languages
            },
            new.hotkey,
            new.history_size
        ));
        self.history.truncate(new.history_size);
        self.settings = new;
        self.keys = keys;
        self.trim_history()?;
        if let Some(tray) = &self.tray {
            let _ = tray.set_tooltip(Some(self.tooltip()));
        }
        self.register_hotkey()
    }

    /// Applies a smaller history size to the files; "Off" deletes them.
    fn trim_history(&self) -> Result<(), String> {
        let _ = std::fs::remove_file(data_path("history.txt")); // regenerated on the next "View"
        if self.settings.history_size == 0 {
            let _ = std::fs::remove_file(data_path("history.json"));
            return Ok(());
        }
        history::save(&data_path("history.json"), &self.history)
            .map_err(|e| format!("Could not save the history: {e}"))
    }

    fn open_history(&mut self) {
        if self.history.is_empty() {
            return platform::alert("No transcriptions in the history.");
        }
        let path = data_path("history.txt");
        match std::fs::write(&path, history::to_text(&self.history)) {
            Ok(()) => open_file(&path),
            Err(e) => self.fail(format!("Could not write the history file: {e}")),
        }
    }

    // ---------- updates ----------

    fn check_update(&mut self, manual: bool) {
        if self.checking_update {
            return;
        }
        self.checking_update = true;
        if manual {
            self.show(Overlay::Info("Checking for updates…".into()), None);
        }
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Update((manual, update::fetch_newer(VERSION))));
            ctx.request_repaint();
        });
    }

    fn on_update(&mut self, (manual, result): UpdateResult) {
        self.checking_update = false;
        let next = match result {
            Ok(None) => {
                log("Update check: up to date");
                if manual {
                    self.show(
                        Overlay::Info(format!("wspr {VERSION} is up to date")),
                        Some(Duration::from_millis(2500)),
                    );
                }
                UPDATE_EVERY
            }
            Err(e) => {
                if manual {
                    self.fail(e);
                } else {
                    log(format!("Update check failed: {e}"));
                }
                UPDATE_EVERY
            }
            // Never interrupt a dictation; try again a bit later.
            Ok(Some((version, _))) if !matches!(self.state, State::Idle) => {
                log(format!(
                    "Update {version} found, waiting until dictation is done"
                ));
                UPDATE_RETRY
            }
            Ok(Some((version, bytes))) => {
                log(format!("Installing update {version}"));
                match platform::install_update(&version, &bytes) {
                    Ok(()) => {
                        self.show(Overlay::Info(format!("Updating to wspr {version}…")), None);
                        self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(e) => self.fail(e),
                }
                UPDATE_EVERY
            }
        };
        self.next_update = Instant::now() + next;
    }

    // ---------- events and timers ----------

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Hotkey(id) if self.hotkey.is_some_and(|h| h.id() == id) => self.toggle(),
            Msg::Hotkey(_) => {}
            Msg::Menu(id) => match id.as_str() {
                "retry" => self.retry(),
                "update" => self.check_update(true),
                "log" => open_file(&data_path("wspr.log")),
                "settings" => self.open_settings(),
                "quit" => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                _ => {}
            },
            Msg::Done(d) => self.on_result(d),
            Msg::Update(u) => self.on_update(u),
        }
    }

    /// Runs due timers and asks egui to wake up for the next one.
    fn timers(&mut self) {
        let now = Instant::now();
        let mut next = self.next_update;
        if now >= self.next_update && !self.checking_update {
            self.next_update = now + UPDATE_EVERY; // replaced when the result arrives
            self.check_update(false);
        }
        if let Some(until) = self.overlay_until {
            if now >= until {
                self.show(Overlay::Hidden, None);
            } else {
                next = next.min(until);
            }
        }
        if let Some((_, at)) = &self.restore_clip {
            if now >= *at {
                if let (Some((text, _)), Some(c)) =
                    (self.restore_clip.take(), self.clipboard.as_mut())
                {
                    let _ = c.set_text(text);
                }
            } else {
                next = next.min(*at);
            }
        }
        if let State::Recording(r) = &self.state {
            if r.seconds() >= MAX_SECONDS {
                self.stop();
            } else {
                next = next.min(now + Duration::from_secs(1)); // the timer in the overlay
            }
        }
        self.ctx
            .request_repaint_after(next.saturating_duration_since(Instant::now()));
    }

    /// Moves the overlay window on screen (bottom centre of the primary monitor) or off it.
    fn place_overlay(&mut self, frame: &eframe::Frame) {
        let want = match &self.overlay {
            Overlay::Hidden => None,
            Overlay::Info(_) | Overlay::Error(_) => Some(true),
            _ => Some(false),
        };
        let Some(window) = frame.winit_window() else {
            return;
        };
        // Style on the first frame, before the window is ever shown or listed in Alt+Tab.
        if !self.styled {
            platform::style_overlay(window);
            self.styled = true;
        }
        if want == self.placed {
            return;
        }
        self.placed = want;
        use winit::dpi::{PhysicalPosition, PhysicalSize};
        let Some(wide) = want else {
            window.set_outer_position(PhysicalPosition::new(-20000, -20000));
            return;
        };
        let Some(monitor) = window
            .primary_monitor()
            .or_else(|| window.current_monitor())
        else {
            return;
        };
        let scale = monitor.scale_factor();
        let size = if wide { PILL_WIDE } else { PILL };
        let (w, h) = (
            (size.x as f64 * scale) as i32,
            (size.y as f64 * scale) as i32,
        );
        let (m, ms) = (monitor.position(), monitor.size());
        let x = m.x + (ms.width as i32 - w) / 2;
        let y = m.y + ms.height as i32 - h - (90.0 * scale) as i32; // above the taskbar
        let _ = window.request_inner_size(PhysicalSize::new(w as u32, h as u32));
        window.set_outer_position(PhysicalPosition::new(x, y));
        platform::round_corners(window, w, h);
        self.ctx.request_repaint(); // paint again at the new size
    }

    fn paint_overlay(&self, ui: &mut egui::Ui) {
        let (text, color) = match &self.overlay {
            Overlay::Hidden => return,
            Overlay::Recording(t) => {
                let s = t.elapsed().as_secs();
                (
                    format!("Recording   {}:{:02}", s / 60, s % 60),
                    egui::Color32::from_rgb(230, 60, 60),
                )
            }
            Overlay::Transcribing => (
                "Transcribing…".into(),
                egui::Color32::from_rgb(230, 160, 20),
            ),
            Overlay::Done => ("Done".into(), egui::Color32::from_rgb(80, 200, 80)),
            Overlay::Info(s) => (s.clone(), egui::Color32::from_gray(180)),
            Overlay::Error(s) => (format!("Error: {s}"), egui::Color32::from_rgb(230, 60, 60)),
        };
        let rect = ui.max_rect();
        let h = rect.height();
        let painter = ui.painter();
        painter.rect_filled(rect, if TRANSPARENT { h / 2.0 } else { 0.0 }, PILL_BG);
        painter.circle_filled(
            egui::pos2(rect.left() + h / 2.0, rect.center().y),
            h * 0.2,
            color,
        );
        let font = egui::FontId::proportional(h * 0.38);
        let max_w = rect.width() - h * 1.5;
        let mut shown = text.clone();
        // Long errors are cut to fit the pill; the full text is in the log.
        while painter
            .layout_no_wrap(shown.clone(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
            > max_w
            && shown.chars().count() > 4
        {
            shown = shown
                .chars()
                .take(shown.chars().count() - 4)
                .collect::<String>()
                + "…";
        }
        painter.text(
            egui::pos2(rect.left() + h, rect.center().y),
            egui::Align2::LEFT_CENTER,
            shown,
            font,
            egui::Color32::from_gray(240),
        );
    }

    fn settings_window(&mut self, ui: &mut egui::Ui) {
        let Some(mut form) = self.form.take() else {
            return;
        };
        let builder = egui::ViewportBuilder::default()
            .with_title("wspr settings")
            .with_app_id("wspr")
            .with_icon(window_icon())
            .with_inner_size([520.0, 260.0])
            .with_resizable(false);
        let action = ui.ctx().show_viewport_immediate(
            egui::ViewportId::from_hash_of("settings"),
            builder,
            |ui, _| {
                let mut action = egui::CentralPanel::default()
                    .show(ui, |ui| form.ui(ui))
                    .inner;
                if ui.input(|i| i.viewport().close_requested()) {
                    action = Action::Cancel;
                }
                action
            },
        );
        match action {
            Action::None => self.form = Some(form),
            Action::Cancel => self.close_settings(),
            Action::ViewHistory => {
                self.open_history();
                self.form = Some(form);
            }
            Action::Save => match self.save_settings(&form) {
                Ok(()) => self.form = None,
                Err(e) => {
                    form.error = Some(e);
                    self.form = Some(form);
                }
            },
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle(msg);
        }
        self.timers();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.place_overlay(frame);
        self.paint_overlay(ui);
        self.settings_window(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if TRANSPARENT {
            [0.0; 4]
        } else {
            PILL_BG.to_normalized_gamma_f32()
        }
    }
}

fn open_file(path: &std::path::Path) {
    if !path.exists() {
        return platform::alert(&format!("{} does not exist yet.", path.display()));
    }
    if let Err(e) = open::that_detached(path) {
        platform::alert(&format!("Could not open {}: {e}", path.display()));
    }
}

/// A filled anti-aliased circle as 32x32 RGBA, for the recording and busy tray icons.
fn circle_rgba([r, g, b]: [u8; 3]) -> Vec<u8> {
    (0..32 * 32)
        .flat_map(|i| {
            let (dx, dy) = ((i % 32) as f32 - 15.5, (i / 32) as f32 - 15.5);
            let a = (13.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            [r, g, b, (a * 255.0) as u8]
        })
        .collect()
}
