//! Hotkeys as text ("Ctrl+Shift+Space") <-> Win32 RegisterHotKey modifiers and virtual key,
//! plus conversion to and from the Windows hotkey control's format.

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

// Hotkey control (msctls_hotkey32) modifier flags.
const HOTKEYF_SHIFT: u32 = 0x1;
const HOTKEYF_CONTROL: u32 = 0x2;
const HOTKEYF_ALT: u32 = 0x4;
const HOTKEYF_EXT: u32 = 0x8;

/// Named keys besides A-Z, 0-9 and F1-F24.
const NAMED: &[(&str, u32)] = &[
    ("Space", 0x20),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("End", 0x23),
    ("Home", 0x24),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("Pause", 0x13),
    ("ScrollLock", 0x91),
    ("Num0", 0x60),
    ("Num1", 0x61),
    ("Num2", 0x62),
    ("Num3", 0x63),
    ("Num4", 0x64),
    ("Num5", 0x65),
    ("Num6", 0x66),
    ("Num7", 0x67),
    ("Num8", 0x68),
    ("Num9", 0x69),
];

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
                    return Err(format!("The hotkey has more than one main key: {s}"));
                }
                key = Some(vk(k).ok_or_else(|| format!("Unknown key: \"{k}\""))?);
            }
        }
    }
    let key = key.ok_or_else(|| format!("The hotkey has no main key: {s}"))?;
    // A bare letter or arrow as a global hotkey would stop that key from typing anywhere.
    let bare_ok = matches!(key, 0x70..=0x87 | 0x13 | 0x91);
    if mods == 0 && !bare_ok {
        return Err(format!(
            "Add Ctrl, Alt or Shift to {s}, otherwise the key stops working for typing"
        ));
    }
    Ok((mods, key))
}

fn vk(k: &str) -> Option<u32> {
    if let Some((_, v)) = NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)) {
        return Some(*v);
    }
    if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() {
        return Some(k.as_bytes()[0].to_ascii_uppercase() as u32);
    }
    k.strip_prefix('f')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| (1..=24).contains(n))
        .map(|n| 0x6F + n)
}

/// Text form of a hotkey, or None for a key this app has no name for.
pub fn format(mods: u32, vk: u32) -> Option<String> {
    let key = match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk)?.to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => NAMED.iter().find(|(_, v)| *v == vk)?.0.to_string(),
    };
    let mut parts = Vec::new();
    for (flag, name) in [
        (MOD_CONTROL, "Ctrl"),
        (MOD_ALT, "Alt"),
        (MOD_SHIFT, "Shift"),
        (MOD_WIN, "Win"),
    ] {
        if mods & flag != 0 {
            parts.push(name.to_string());
        }
    }
    parts.push(key);
    Some(parts.join("+"))
}

/// Hotkey control value (HKM_GETHOTKEY) -> (modifiers, virtual key).
pub fn from_control(word: u32) -> (u32, u32) {
    let flags = (word >> 8) & 0xFF;
    let mut mods = 0;
    for (f, m) in [
        (HOTKEYF_SHIFT, MOD_SHIFT),
        (HOTKEYF_CONTROL, MOD_CONTROL),
        (HOTKEYF_ALT, MOD_ALT),
    ] {
        if flags & f != 0 {
            mods |= m;
        }
    }
    (mods, word & 0xFF)
}

/// (modifiers, virtual key) -> hotkey control value (HKM_SETHOTKEY). The control cannot show Win.
pub fn to_control(mods: u32, vk: u32) -> u32 {
    let mut flags = 0;
    for (m, f) in [
        (MOD_SHIFT, HOTKEYF_SHIFT),
        (MOD_CONTROL, HOTKEYF_CONTROL),
        (MOD_ALT, HOTKEYF_ALT),
    ] {
        if mods & m != 0 {
            flags |= f;
        }
    }
    // Navigation keys are "extended"; without the flag the control names them as numpad keys.
    if (0x21..=0x2E).contains(&vk) {
        flags |= HOTKEYF_EXT;
    }
    (flags << 8) | vk
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
        assert_eq!(parse("Ctrl+pagedown"), Ok((MOD_CONTROL, 0x22)));
    }

    #[test]
    fn rejects_bad_hotkeys() {
        assert!(parse("").is_err());
        assert!(parse("Ctrl+Shift").is_err());
        assert!(parse("Ctrl+A+B").is_err());
        assert!(parse("F25").is_err());
        assert!(parse("Ctrl+Ö").is_err());
        assert!(parse("D").unwrap_err().contains("Add Ctrl"));
        assert!(parse("Space").is_err());
    }

    #[test]
    fn format_roundtrips_with_parse() {
        for s in [
            "Ctrl+Shift+Space",
            "Alt+D",
            "F9",
            "Ctrl+Alt+Num5",
            "Shift+Delete",
            "Ctrl+7",
        ] {
            let (m, k) = parse(s).unwrap();
            assert_eq!(format(m, k).as_deref(), Some(s));
        }
        assert_eq!(format(MOD_CONTROL, 0xBA), None); // OEM key without a name
    }

    #[test]
    fn hotkey_control_conversion() {
        let (m, k) = parse("Ctrl+Shift+Space").unwrap();
        assert_eq!(to_control(m, k), 0x0320);
        assert_eq!(from_control(0x0320), (m, k));
        assert_eq!(from_control(to_control(MOD_ALT, 0x2E)), (MOD_ALT, 0x2E)); // EXT flag dropped
    }
}
