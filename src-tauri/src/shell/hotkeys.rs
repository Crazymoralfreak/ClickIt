//! Global hotkeys. Settings store them as `Ctrl+Shift+F6`; Windows gets them
//! through the global shortcut plugin.

use std::str::FromStr;

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers as ShortcutModifiers, Shortcut};

use crate::error::{AppError, AppResult};
use crate::input::keys::{parse_hotkey, vk_from_code};
use crate::model::scenario::Modifiers;

/// A hotkey from the settings in the forms its users need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub shortcut: Shortcut,
    pub modifiers: Modifiers,
    /// Virtual key, for the recorder to leave the hotkeys of ClickIt out of the steps.
    pub vk: u16,
}

pub fn parse(text: &str) -> AppResult<Hotkey> {
    let invalid = || AppError::new("hotkeyInvalid").with("hotkey", text);
    let (modifiers, key) = parse_hotkey(text).ok_or_else(invalid)?;
    let code = Code::from_str(&key).map_err(|_| invalid())?;
    let vk = vk_from_code(&key).ok_or_else(invalid)?.vk;
    let mut flags = ShortcutModifiers::empty();
    flags.set(ShortcutModifiers::CONTROL, modifiers.ctrl);
    flags.set(ShortcutModifiers::ALT, modifiers.alt);
    flags.set(ShortcutModifiers::SHIFT, modifiers.shift);
    flags.set(ShortcutModifiers::SUPER, modifiers.win);
    Ok(Hotkey { shortcut: Shortcut::new(Some(flags), code), modifiers, vk })
}

/// A hotkey that may be left unassigned: an empty text is `None`.
pub fn parse_optional(text: &str) -> AppResult<Option<Hotkey>> {
    if text.is_empty() {
        Ok(None)
    } else {
        parse(text).map(Some)
    }
}

/// Registers the hotkey with Windows. `text` is what the user typed, for the message.
pub fn register(app: &AppHandle, hotkey: &Hotkey, text: &str) -> AppResult<()> {
    app.global_shortcut().register(hotkey.shortcut).map_err(|_| AppError::new("hotkeyTaken").with("hotkey", text))
}

pub fn unregister(app: &AppHandle, hotkey: &Hotkey) {
    let _ = app.global_shortcut().unregister(hotkey.shortcut);
}
