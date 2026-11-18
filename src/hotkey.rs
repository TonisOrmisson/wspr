//! Parse a hotkey like "Ctrl+Shift+Space" into Win32 RegisterHotKey modifiers and virtual key.

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

pub fn parse(s: &str) -> Result<(u32, u32), String> {
    let mut mods = 0;
    let mut key = None;
    for part in s.split('+').map(|p| p.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" => mods |= MOD_WIN,
            k => {
                if key.is_some() {
                    return Err(format!("Kiirklahvis on mitu tavalist klahvi: {s}"));
                }
                key = Some(vk(k).ok_or_else(|| format!("Tundmatu klahv: \"{k}\""))?);
            }
        }
    }
    key.map(|k| (mods, k))
        .ok_or_else(|| format!("Kiirklahvist puudub tavaline klahv: {s}"))
}

fn vk(k: &str) -> Option<u32> {
    match k {
        "space" => Some(0x20),
        "pause" => Some(0x13),
        "scrolllock" => Some(0x91),
        "insert" => Some(0x2D),
        _ if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => {
            Some(k.as_bytes()[0].to_ascii_uppercase() as u32)
        }
        _ => k
            .strip_prefix('f')
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|n| (1..=24).contains(n))
            .map(|n| 0x6F + n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_hotkeys() {
        assert_eq!(
            parse("Ctrl+Shift+Space"),
            Ok((MOD_CONTROL | MOD_SHIFT, 0x20))
        );
        assert_eq!(parse("alt + d"), Ok((MOD_ALT, b'D' as u32)));
        assert_eq!(parse("F9"), Ok((0, 0x78)));
        assert_eq!(parse("Win+F24"), Ok((MOD_WIN, 0x87)));
    }

    #[test]
    fn rejects_bad_hotkeys() {
        assert!(parse("").is_err());
        assert!(parse("Ctrl+Shift").is_err());
        assert!(parse("Ctrl+A+B").is_err());
        assert!(parse("F25").is_err());
        assert!(parse("Ctrl+Ö").is_err());
    }
}
