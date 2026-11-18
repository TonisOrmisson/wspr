//! User settings stored as JSON. The API key is stored encrypted (Windows DPAPI) as hex.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub api_key_enc: String,
    pub model: String,
    /// Comma-separated ISO-639-1 codes, e.g. "et" or "et,en".
    pub languages: String,
    pub hotkey: String,
    /// Optional vocabulary hint (names, terms) sent as the API `prompt`.
    pub prompt: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_key_enc: String::new(),
            model: "gpt-transcribe".into(),
            languages: "et".into(),
            hotkey: "Ctrl+Shift+Space".into(),
            prompt: String::new(),
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
    }
}
