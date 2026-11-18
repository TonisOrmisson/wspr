//! The settings window (egui): provider, API key, model, language, hotkey, history size.

use crate::hotkey;
use crate::settings::{HISTORY_SIZES, LANGUAGES, Settings, key_hint, language_index};
use crate::stt::PROVIDERS;
use eframe::egui;

pub enum Action {
    None,
    Save,
    Cancel,
    ViewHistory,
}

/// What the window edits. Keys are plain text here; they are encrypted on save.
pub struct Form {
    pub provider: usize,
    pub keys: [String; 2],
    pub models: [String; 2],
    pub language: String,
    pub hotkey: String,
    pub history_size: usize,
    pub error: Option<String>,
    capturing: bool,
}

impl Form {
    pub fn new(s: &Settings, keys: &[String; 2]) -> Self {
        Self {
            provider: s.provider_index(),
            keys: keys.clone(),
            models: [0, 1].map(|p| s.model_of(p).to_string()),
            language: s.languages.clone(),
            hotkey: s.hotkey.clone(),
            history_size: s.history_size,
            error: None,
            capturing: false,
        }
    }

    /// The settings to save, without keys (those are encrypted separately).
    pub fn to_settings(&self) -> Settings {
        let mut s = Settings {
            provider: PROVIDERS[self.provider].id.into(),
            languages: self.language.clone(),
            hotkey: self.hotkey.clone(),
            history_size: self.history_size,
            ..Default::default()
        };
        for (p, provider) in PROVIDERS.iter().enumerate() {
            let model = self.models[p].trim();
            let model = match provider.models.first() {
                Some(default) if model.is_empty() => default.to_string(),
                _ => model.to_string(),
            };
            s.set_provider_values(p, model, String::new());
        }
        s
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Action {
        let mut action = Action::None;
        self.capture_keys(ui);
        let p = self.provider;
        let width = 300.0;
        egui::Grid::new("settings")
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                ui.label("Provider");
                egui::ComboBox::from_id_salt("provider")
                    .width(width)
                    .selected_text(PROVIDERS[p].label)
                    .show_ui(ui, |ui| {
                        for (i, prov) in PROVIDERS.iter().enumerate() {
                            ui.selectable_value(&mut self.provider, i, prov.label);
                        }
                    });
                ui.end_row();

                ui.label(PROVIDERS[p].key_label);
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.keys[p])
                            .password(true)
                            .desired_width(width),
                    );
                    ui.small(format!("Key: {}", key_hint(&self.keys[p])));
                });
                ui.end_row();

                ui.label("Model");
                ui.horizontal(|ui| {
                    let list = PROVIDERS[p].models;
                    let w = if list.is_empty() { width } else { width - 34.0 };
                    ui.add(
                        egui::TextEdit::singleline(&mut self.models[p])
                            .hint_text(PROVIDERS[p].model_hint)
                            .desired_width(w),
                    );
                    if !list.is_empty() {
                        ui.menu_button("…", |ui| {
                            for m in list {
                                if ui.button(*m).clicked() {
                                    self.models[p] = m.to_string();
                                    ui.close();
                                }
                            }
                        });
                    }
                });
                ui.end_row();

                ui.label("Language");
                let current = language_index(&self.language)
                    .map_or(self.language.as_str(), |i| LANGUAGES[i].0);
                egui::ComboBox::from_id_salt("language")
                    .width(width)
                    .height(300.0)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (label, code) in LANGUAGES {
                            ui.selectable_value(&mut self.language, code.to_string(), *label);
                        }
                    });
                ui.end_row();

                ui.label("Hotkey");
                let text = if self.capturing {
                    let m = ui.input(|i| i.modifiers);
                    let held = hotkey::modifier_prefix(modifier_bits(m));
                    if held.is_empty() {
                        "Press the keys…".to_string()
                    } else {
                        format!("{held}…")
                    }
                } else {
                    self.hotkey.clone()
                };
                let button = egui::Button::new(text)
                    .min_size(egui::vec2(width, 0.0))
                    .selected(self.capturing);
                if ui
                    .add(button)
                    .on_hover_text("Click, then press the key combination")
                    .clicked()
                {
                    self.capturing = !self.capturing;
                }
                ui.end_row();

                ui.label("Keep transcriptions");
                ui.horizontal(|ui| {
                    let label = HISTORY_SIZES
                        .iter()
                        .find(|h| h.1 == self.history_size)
                        .map_or_else(
                            || format!("Last {}", self.history_size),
                            |h| h.0.to_string(),
                        );
                    egui::ComboBox::from_id_salt("history")
                        .width(width - 90.0)
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for (label, n) in HISTORY_SIZES {
                                ui.selectable_value(&mut self.history_size, *n, *label);
                            }
                        });
                    if ui.button("View…").clicked() {
                        action = Action::ViewHistory;
                    }
                });
                ui.end_row();
            });

        if let Some(e) = &self.error {
            ui.add_space(6.0);
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.weak(concat!("wspr ", env!("CARGO_PKG_VERSION")));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    action = Action::Cancel;
                }
                if ui.button("Save").clicked() {
                    action = Action::Save;
                }
            });
        });
        if !self.capturing {
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                action = Action::Save;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                action = Action::Cancel;
            }
        }
        action
    }

    /// While recording a hotkey, key presses are taken before any widget sees them,
    /// so Space or Enter cannot also press a button.
    fn capture_keys(&mut self, ui: &mut egui::Ui) {
        if !self.capturing {
            return;
        }
        let pressed = ui.input_mut(|i| {
            let mut got = None;
            i.events.retain(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    got = got.or(Some((*key, *modifiers)));
                    false
                }
                egui::Event::Key { .. } | egui::Event::Text(_) => false,
                _ => true,
            });
            got
        });
        let Some((key, m)) = pressed else { return };
        if key == egui::Key::Escape && !m.any() {
            self.capturing = false;
            return;
        }
        let text = hotkey::modifier_prefix(modifier_bits(m)) + key_name(key);
        match hotkey::parse(&text) {
            Ok(_) => {
                self.hotkey = text;
                self.capturing = false;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
}

fn modifier_bits(m: egui::Modifiers) -> u32 {
    let mut bits = 0;
    if m.ctrl {
        bits |= hotkey::MOD_CONTROL;
    }
    if m.alt {
        bits |= hotkey::MOD_ALT;
    }
    if m.shift {
        bits |= hotkey::MOD_SHIFT;
    }
    bits
}

/// egui's key name in the app's hotkey syntax.
fn key_name(key: egui::Key) -> &'static str {
    use egui::Key::*;
    match key {
        ArrowLeft => "Left",
        ArrowRight => "Right",
        ArrowUp => "Up",
        ArrowDown => "Down",
        Num0 => "0",
        Num1 => "1",
        Num2 => "2",
        Num3 => "3",
        Num4 => "4",
        Num5 => "5",
        Num6 => "6",
        Num7 => "7",
        Num8 => "8",
        Num9 => "9",
        other => other.name(),
    }
}
