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
    fn hex_roundtrip() {
        let b = vec![0u8, 1, 0xab, 0xff];
        assert_eq!(to_hex(&b), "0001abff");
        assert_eq!(from_hex("0001abff"), Some(b));
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex("zz"), None);
        assert_eq!(from_hex("éé"), None);
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
