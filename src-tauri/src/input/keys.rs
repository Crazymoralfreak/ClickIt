//! Key names. Scenarios and hotkeys store W3C `KeyboardEvent.code` names
//! (`KeyA`, `Digit1`, `Enter`, `ArrowLeft`), which do not depend on the layout.

use crate::model::scenario::Modifiers;

/// Virtual-key code and whether the key needs `KEYEVENTF_EXTENDEDKEY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualKey {
    pub vk: u16,
    pub extended: bool,
}

const NAMED: &[(&str, u16, bool)] = &[
    ("Enter", 0x0D, false),
    ("NumpadEnter", 0x0D, true),
    ("Escape", 0x1B, false),
    ("Backspace", 0x08, false),
    ("Tab", 0x09, false),
    ("Space", 0x20, false),
    ("ArrowLeft", 0x25, true),
    ("ArrowUp", 0x26, true),
    ("ArrowRight", 0x27, true),
    ("ArrowDown", 0x28, true),
    ("Insert", 0x2D, true),
    ("Delete", 0x2E, true),
    ("Home", 0x24, true),
    ("End", 0x23, true),
    ("PageUp", 0x21, true),
    ("PageDown", 0x22, true),
    ("CapsLock", 0x14, false),
    ("NumLock", 0x90, true),
    ("ScrollLock", 0x91, false),
    ("PrintScreen", 0x2C, true),
    ("Pause", 0x13, false),
    ("ContextMenu", 0x5D, true),
    ("ShiftLeft", 0xA0, false),
    ("ShiftRight", 0xA1, false),
    ("ControlLeft", 0xA2, false),
    ("ControlRight", 0xA3, true),
    ("AltLeft", 0xA4, false),
    ("AltRight", 0xA5, true),
    ("MetaLeft", 0x5B, true),
    ("MetaRight", 0x5C, true),
    ("Minus", 0xBD, false),
    ("Equal", 0xBB, false),
    ("BracketLeft", 0xDB, false),
    ("BracketRight", 0xDD, false),
    ("Backslash", 0xDC, false),
    ("Semicolon", 0xBA, false),
    ("Quote", 0xDE, false),
    ("Backquote", 0xC0, false),
    ("Comma", 0xBC, false),
    ("Period", 0xBE, false),
    ("Slash", 0xBF, false),
    ("IntlBackslash", 0xE2, false),
    ("NumpadMultiply", 0x6A, false),
    ("NumpadAdd", 0x6B, false),
    ("NumpadSubtract", 0x6D, false),
    ("NumpadDecimal", 0x6E, false),
    ("NumpadDivide", 0x6F, true),
];

pub const VK_SHIFT_L: u16 = 0xA0;
pub const VK_CONTROL_L: u16 = 0xA2;
pub const VK_ALT_L: u16 = 0xA4;
pub const VK_WIN_L: u16 = 0x5B;

/// Maps a code name to its virtual key, `None` for unknown names.
pub fn vk_from_code(code: &str) -> Option<VirtualKey> {
    let plain = |vk: u16| Some(VirtualKey { vk, extended: false });
    if let Some(letter) = code.strip_prefix("Key") {
        let mut chars = letter.chars();
        return match (chars.next(), chars.next()) {
            (Some(c @ 'A'..='Z'), None) => plain(c as u16),
            _ => None,
        };
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        return match digit.parse::<u16>() {
            Ok(n @ 0..=9) if digit.len() == 1 => plain(0x30 + n),
            _ => None,
        };
    }
    if let Some(n) = code.strip_prefix("Numpad").and_then(|rest| rest.parse::<u16>().ok()) {
        return if n <= 9 { plain(0x60 + n) } else { None };
    }
    if let Some(n) = code.strip_prefix('F').and_then(|rest| rest.parse::<u16>().ok()) {
        return if (1..=24).contains(&n) { plain(0x70 + n - 1) } else { None };
    }
    NAMED
        .iter()
        .find(|(name, _, _)| *name == code)
        .map(|&(_, vk, extended)| VirtualKey { vk, extended })
}

/// Reverse of [`vk_from_code`], used by the recorder.
pub fn code_from_vk(vk: u16, extended: bool) -> Option<String> {
    match vk {
        0x41..=0x5A => Some(format!("Key{}", (vk as u8) as char)),
        0x30..=0x39 => Some(format!("Digit{}", vk - 0x30)),
        0x60..=0x69 => Some(format!("Numpad{}", vk - 0x60)),
        0x70..=0x87 => Some(format!("F{}", vk - 0x70 + 1)),
        // Generic modifier codes reported by some drivers.
        0x10 => Some("ShiftLeft".to_string()),
        0x11 => Some(if extended { "ControlRight" } else { "ControlLeft" }.to_string()),
        0x12 => Some(if extended { "AltRight" } else { "AltLeft" }.to_string()),
        0x0D => Some(if extended { "NumpadEnter" } else { "Enter" }.to_string()),
        _ => NAMED.iter().find(|(_, code, _)| *code == vk).map(|(name, _, _)| name.to_string()),
    }
}

pub fn is_known(code: &str) -> bool {
    vk_from_code(code).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifierKey {
    Ctrl,
    Alt,
    Shift,
    Win,
}

/// Tells whether a virtual key is one of the four modifiers.
pub fn modifier_of(vk: u16) -> Option<ModifierKey> {
    match vk {
        0x10 | 0xA0 | 0xA1 => Some(ModifierKey::Shift),
        0x11 | 0xA2 | 0xA3 => Some(ModifierKey::Ctrl),
        0x12 | 0xA4 | 0xA5 => Some(ModifierKey::Alt),
        0x5B | 0x5C => Some(ModifierKey::Win),
        _ => None,
    }
}

/// Parses `Ctrl+Shift+F6` into modifiers and a key code name.
pub fn parse_hotkey(text: &str) -> Option<(Modifiers, String)> {
    let mut modifiers = Modifiers::default();
    let mut key: Option<String> = None;
    for part in text.split('+').map(str::trim) {
        match part {
            "Ctrl" | "Control" => modifiers.ctrl = true,
            "Alt" => modifiers.alt = true,
            "Shift" => modifiers.shift = true,
            "Win" | "Meta" | "Super" => modifiers.win = true,
            code if key.is_none() && is_known(code) && modifier_of(vk_from_code(code)?.vk).is_none() => {
                key = Some(code.to_string())
            }
            _ => return None,
        }
    }
    key.map(|key| (modifiers, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_names_map_to_virtual_keys() {
        assert_eq!(vk_from_code("KeyA"), Some(VirtualKey { vk: 0x41, extended: false }));
        assert_eq!(vk_from_code("Digit7"), Some(VirtualKey { vk: 0x37, extended: false }));
        assert_eq!(vk_from_code("F6"), Some(VirtualKey { vk: 0x75, extended: false }));
        assert_eq!(vk_from_code("Numpad3"), Some(VirtualKey { vk: 0x63, extended: false }));
        assert_eq!(vk_from_code("ArrowLeft"), Some(VirtualKey { vk: 0x25, extended: true }));
        assert_eq!(vk_from_code("KeyAB"), None);
        assert_eq!(vk_from_code("F25"), None);
        assert_eq!(vk_from_code(""), None);
    }

    #[test]
    fn virtual_keys_map_back_to_code_names() {
        for code in ["KeyZ", "Digit0", "F12", "Numpad9", "Enter", "NumpadEnter", "Delete", "Slash"] {
            let key = vk_from_code(code).unwrap();
            assert_eq!(code_from_vk(key.vk, key.extended).as_deref(), Some(code));
        }
    }

    #[test]
    fn hotkeys_parse_with_and_without_modifiers() {
        assert_eq!(parse_hotkey("F6"), Some((Modifiers::default(), "F6".to_string())));
        let (modifiers, key) = parse_hotkey("Ctrl+Shift+KeyR").unwrap();
        assert!(modifiers.ctrl && modifiers.shift && !modifiers.alt && !modifiers.win);
        assert_eq!(key, "KeyR");
        assert_eq!(parse_hotkey("Ctrl+Shift"), None);
        assert_eq!(parse_hotkey("Ctrl+F6+F7"), None);
        assert_eq!(parse_hotkey("Bogus"), None);
    }
}
