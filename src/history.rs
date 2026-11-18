//! Last transcriptions, kept so text pasted into the wrong place can be recovered.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Entry {
    pub time: String,
    pub text: String,
}

/// Newest first, at most `keep` entries.
pub fn push(list: &mut Vec<Entry>, e: Entry, keep: usize) {
    list.insert(0, e);
    list.truncate(keep);
}

pub fn load(path: &Path) -> Vec<Entry> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, list: &[Entry]) -> std::io::Result<()> {
    write_atomic(path, serde_json::to_string_pretty(list)?.as_bytes())
}

/// Write to a temp file and rename, so a crash mid-write cannot destroy the old file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

pub fn to_text(list: &[Entry]) -> String {
    list.iter()
        .map(|e| format!("[{}]\r\n{}\r\n", e.time, e.text))
        .collect::<Vec<_>>()
        .join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(text: &str) -> Entry {
        Entry {
            time: "2026-11-18 14:03".into(),
            text: text.into(),
        }
    }

    #[test]
    fn push_keeps_newest_first_and_caps() {
        let mut l = Vec::new();
        for i in 0..15 {
            push(&mut l, e(&i.to_string()), 10);
        }
        assert_eq!(l.len(), 10);
        assert_eq!(l[0].text, "14");
        push(&mut l, e("x"), 0);
        assert!(l.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("wspr-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("history.json");
        let l = vec![e("tere"), e("maailm")];
        save(&p, &l).unwrap();
        assert_eq!(load(&p), l);
        assert!(load(&dir.join("missing.json")).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
