//! Hotkeys as text ("Ctrl+Shift+Space") <-> Win32 RegisterHotKey modifiers and virtual key.

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

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

/// "Ctrl+Shift+" for the held modifiers, shown while the user is still pressing them.
pub fn modifier_prefix(mods: u32) -> String {
    [
        (MOD_CONTROL, "Ctrl+"),
        (MOD_ALT, "Alt+"),
        (MOD_SHIFT, "Shift+"),
        (MOD_WIN, "Win+"),
    ]
    .iter()
    .filter(|(flag, _)| mods & flag != 0)
    .map(|(_, name)| *name)
    .collect()
}

/// The hotkey in the accelerator syntax of the `global-hotkey` crate, e.g. "ctrl+shift+Space".
pub fn to_accelerator(s: &str) -> Result<String, String> {
    let (mods, vk) = parse(s)?;
    let key = match vk {
        0x30..=0x39 => format!("Digit{}", vk - 0x30),
        0x41..=0x5A => format!("Key{}", char::from_u32(vk).unwrap_or('A')),
        0x60..=0x69 => format!("Numpad{}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x25 => "ArrowLeft".into(),
        0x26 => "ArrowUp".into(),
        0x27 => "ArrowRight".into(),
        0x28 => "ArrowDown".into(),
        _ => NAMED
            .iter()
            .find(|(_, v)| *v == vk)
            .map(|(n, _)| n.to_string())
            .ok_or_else(|| format!("Unsupported key in {s}"))?,
    };
    let prefix: String = [
        (MOD_CONTROL, "ctrl+"),
        (MOD_ALT, "alt+"),
        (MOD_SHIFT, "shift+"),
        (MOD_WIN, "super+"),
    ]
    .iter()
    .filter(|(flag, _)| mods & flag != 0)
    .map(|(_, name)| *name)
    .collect();
    Ok(prefix + &key)
}

/// The hotkey in GTK accelerator form, as GNOME shortcuts store it: "<Control><Shift>space".
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn to_gtk(s: &str) -> Result<String, String> {
    let (mods, vk) = parse(s)?;
    let key = match vk {
        0x30..=0x39 => char::from_u32(vk).unwrap_or('0').to_string(),
        0x41..=0x5A => char::from_u32(vk + 0x20).unwrap_or('a').to_string(),
        0x60..=0x69 => format!("KP_{}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x20 => "space".into(),
        0x21 => "Page_Up".into(),
        0x22 => "Page_Down".into(),
        0x91 => "Scroll_Lock".into(),
        _ => NAMED
            .iter()
            .find(|(_, v)| *v == vk)
            .map(|(n, _)| n.to_string())
            .ok_or_else(|| format!("Unsupported key in {s}"))?,
    };
    let prefix: String = [
        (MOD_CONTROL, "<Control>"),
        (MOD_ALT, "<Alt>"),
        (MOD_SHIFT, "<Shift>"),
        (MOD_WIN, "<Super>"),
    ]
    .iter()
    .filter(|(flag, _)| mods & flag != 0)
    .map(|(_, name)| *name)
    .collect();
    Ok(prefix + &key)
}

/// The hotkey as a Qt key code, as KDE's shortcut service takes it.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn to_qt(s: &str) -> Result<i32, String> {
    let (mods, vk) = parse(s)?;
    let key = match vk {
        0x20 | 0x30..=0x39 | 0x41..=0x5A => vk,
        0x60..=0x69 => 0x2000_0000 | (vk - 0x30), // keypad modifier + digit
        0x70..=0x87 => 0x0100_0030 + (vk - 0x70),
        0x21 => 0x0100_0016,
        0x22 => 0x0100_0017,
        0x23 => 0x0100_0011,
        0x24 => 0x0100_0010,
        0x25..=0x28 => 0x0100_0012 + (vk - 0x25),
        0x2D => 0x0100_0006,
        0x2E => 0x0100_0007,
        0x13 => 0x0100_0008,
        0x91 => 0x0100_0026,
        _ => return Err(format!("Unsupported key in {s}")),
    };
    let qt_mods: u32 = [
        (MOD_SHIFT, 0x0200_0000),
        (MOD_CONTROL, 0x0400_0000),
        (MOD_ALT, 0x0800_0000),
        (MOD_WIN, 0x1000_0000),
    ]
    .iter()
    .filter(|(flag, _)| mods & flag != 0)
    .map(|(_, q)| q)
    .sum();
    Ok((qt_mods | key) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qt_key_codes() {
        assert_eq!(to_qt("Ctrl+Shift+Space").unwrap(), 0x0600_0020);
        assert_eq!(to_qt("F9").unwrap(), 0x0100_0038);
        assert_eq!(to_qt("Alt+Down").unwrap(), 0x0800_0000 | 0x0100_0015);
        assert_eq!(to_qt("Ctrl+Num5").unwrap(), 0x0400_0000 | 0x2000_0035);
    }

    #[test]
    fn gtk_accelerators() {
        assert_eq!(to_gtk("Ctrl+Shift+Space").unwrap(), "<Control><Shift>space");
        assert_eq!(to_gtk("Alt+Win+K").unwrap(), "<Alt><Super>k");
        assert_eq!(to_gtk("F9").unwrap(), "F9");
        assert_eq!(to_gtk("Ctrl+PageDown").unwrap(), "<Control>Page_Down");
        assert_eq!(to_gtk("Shift+Left").unwrap(), "<Shift>Left");
    }

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
    fn accelerator_syntax() {
        assert_eq!(
            to_accelerator("Ctrl+Shift+Space").unwrap(),
            "ctrl+shift+Space"
        );
        assert_eq!(to_accelerator("Alt+D").unwrap(), "alt+KeyD");
        assert_eq!(to_accelerator("F9").unwrap(), "F9");
        assert_eq!(to_accelerator("Win+Left").unwrap(), "super+ArrowLeft");
        assert_eq!(to_accelerator("Ctrl+7").unwrap(), "ctrl+Digit7");
        assert_eq!(to_accelerator("Ctrl+Num5").unwrap(), "ctrl+Numpad5");
        assert_eq!(to_accelerator("Ctrl+PageDown").unwrap(), "ctrl+PageDown");
        assert!(to_accelerator("D").is_err());
    }
}
