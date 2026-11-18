//! Append-only event log (wspr.log) that stays small: past MAX bytes the older half is dropped.

use std::io::Write;
use std::path::Path;

const MAX: u64 = 512 * 1024;

pub fn append(path: &Path, line: &str) -> std::io::Result<()> {
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX {
        let old = std::fs::read(path).unwrap_or_default();
        let half = old.len() / 2;
        // Cut in bytes at a line start, so neither a line nor a UTF-8 character is split.
        let cut = old[half..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(old.len(), |i| half + i + 1);
        crate::history::write_atomic(path, &old[cut..])?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    write!(f, "{line}\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_and_trims_old_lines() {
        let dir = std::env::temp_dir().join(format!("wspr-log-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("wspr.log");
        append(&p, "first").unwrap();
        append(&p, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "first\r\nsecond\r\n");

        let filler = "ä…".repeat(33); // multi-byte characters must not break the trim
        for _ in 0..6000 {
            append(&p, &filler).unwrap();
        }
        append(&p, "last").unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.len() as u64 <= MAX + 200);
        assert!(!s.contains("first"));
        assert!(s.starts_with(&filler) && s.ends_with("last\r\n"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
