//! User settings stored as JSON. The API key is stored encrypted (Windows DPAPI) as hex.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// `stt::PROVIDERS` id: "openai" or "openrouter".
    pub provider: String,
    /// OpenAI key and model (field names kept from versions with OpenAI only).
    pub api_key_enc: String,
    pub model: String,
    pub openrouter_key_enc: String,
    pub openrouter_model: String,
    /// Comma-separated ISO-639-1 codes, e.g. "et" or "et,en".
    pub languages: String,
    pub hotkey: String,
    /// How many transcriptions to keep; 0 keeps none.
    pub history_size: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "openai".into(),
            api_key_enc: String::new(),
            model: "gpt-transcribe".into(),
            openrouter_key_enc: String::new(),
            openrouter_model: "microsoft/mai-transcribe-2".into(),
            languages: "et".into(),
            hotkey: "Ctrl+Shift+Space".into(),
            history_size: 50,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::history::write_atomic(path, serde_json::to_string_pretty(self)?.as_bytes())
    }

    pub fn provider_index(&self) -> usize {
        crate::stt::provider_index(&self.provider)
    }

    /// Model and encrypted key of provider `p` (index into `stt::PROVIDERS`).
    pub fn model_of(&self, p: usize) -> &str {
        if p == 1 {
            &self.openrouter_model
        } else {
            &self.model
        }
    }

    pub fn key_enc_of(&self, p: usize) -> &str {
        if p == 1 {
            &self.openrouter_key_enc
        } else {
            &self.api_key_enc
        }
    }

    pub fn set_provider_values(&mut self, p: usize, model: String, key_enc: String) {
        if p == 1 {
            (self.openrouter_model, self.openrouter_key_enc) = (model, key_enc);
        } else {
            (self.model, self.api_key_enc) = (model, key_enc);
        }
    }

    pub fn language_list(&self) -> Vec<String> {
        self.languages
            .split(',')
            .map(|l| l.trim().to_ascii_lowercase())
            .filter(|l| !l.is_empty())
            .collect()
    }
}

/// Shown in the language dropdown: (label, ISO-639-1 code). An empty code means auto-detect.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("Auto-detect", ""),
    ("Arabic", "ar"),
    ("Bulgarian", "bg"),
    ("Chinese", "zh"),
    ("Czech", "cs"),
    ("Danish", "da"),
    ("Dutch", "nl"),
    ("English", "en"),
    ("Estonian", "et"),
    ("Finnish", "fi"),
    ("French", "fr"),
    ("German", "de"),
    ("Greek", "el"),
    ("Hebrew", "he"),
    ("Hindi", "hi"),
    ("Hungarian", "hu"),
    ("Italian", "it"),
    ("Japanese", "ja"),
    ("Korean", "ko"),
    ("Latvian", "lv"),
    ("Lithuanian", "lt"),
    ("Norwegian", "no"),
    ("Polish", "pl"),
    ("Portuguese", "pt"),
    ("Romanian", "ro"),
    ("Russian", "ru"),
    ("Slovak", "sk"),
    ("Spanish", "es"),
    ("Swedish", "sv"),
    ("Turkish", "tr"),
    ("Ukrainian", "uk"),
];

pub fn language_index(code: &str) -> Option<usize> {
    let code = code.trim().to_ascii_lowercase();
    LANGUAGES.iter().position(|l| l.1 == code)
}

/// History sizes offered in Settings: (label, count).
pub const HISTORY_SIZES: &[(&str, usize)] = &[
    ("Off (keep nothing)", 0),
    ("Last 10", 10),
    ("Last 50", 50),
    ("Last 100", 100),
    ("Last 500", 500),
];

/// "sk-proj-…wxyz": enough to recognise which key it is without showing the secret part.
pub fn key_hint(key: &str) -> String {
    let k: Vec<char> = key.trim().chars().collect();
    match k.len() {
        0 => "No key entered".into(),
        n if n < 16 => format!("{n} characters"),
        n => format!(
            "{}…{}",
            k[..8].iter().collect::<String>(),
            k[n - 4..].iter().collect::<String>()
        ),
    }
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_hint_shows_only_ends() {
        assert_eq!(key_hint(""), "No key entered");
        assert_eq!(key_hint("short"), "5 characters");
        assert_eq!(
            key_hint(" sk-proj-SECRETSECRETSECRETwxyz "),
            "sk-proj-…wxyz"
        );
    }

    #[test]
    fn hex_roundtrip() {
        let b = vec![0u8, 1, 0xab, 0xff];
        assert_eq!(to_hex(&b), "0001abff");
        assert_eq!(from_hex("0001abff"), Some(b));
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex("zz"), None);
        assert_eq!(from_hex("éé"), None);
    }

    #[test]
    fn language_dropdown_lookup() {
        assert_eq!(LANGUAGES[language_index(" ET ").unwrap()].0, "Estonian");
        assert_eq!(language_index(""), Some(0));
        assert_eq!(language_index("et,en"), None);
    }

    #[test]
    fn languages_and_defaults() {
        let s = Settings {
            languages: " ET, en ,".into(),
            ..Default::default()
        };
        assert_eq!(s.language_list(), vec!["et", "en"]);
        let partial: Settings = serde_json::from_str(r#"{"model":"whisper-1"}"#).unwrap();
        assert_eq!(partial.model, "whisper-1");
        assert_eq!(partial.languages, "et");
        assert_eq!(partial.history_size, 50);
        assert_eq!(partial.provider_index(), 0);
        let mut s = Settings::default();
        s.set_provider_values(1, "m".into(), "k".into());
        assert_eq!((s.model_of(1), s.key_enc_of(1)), ("m", "k"));
        assert_eq!(s.model_of(0), "gpt-transcribe");
        // Files from older versions still load; the removed `prompt` field is ignored.
        let old: Settings = serde_json::from_str(r#"{"prompt":"x","hotkey":"F9"}"#).unwrap();
        assert_eq!(old.hotkey, "F9");
    }
}
