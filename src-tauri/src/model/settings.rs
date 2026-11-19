use serde::{Deserialize, Serialize};

use super::schedule::RunTarget;

pub const MAX_COUNTDOWN_SEC: u32 = 10;

/// App-wide settings stored in `settings.json`. Autostart is not here: the
/// registry entry of the current user is the single source of truth for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// `"ru"` or `"en"`; `None` until the first launch picks it from Windows.
    pub language: Option<String>,
    /// Hotkeys as `Ctrl+Shift+F6`: modifiers plus a W3C key code name.
    pub hotkey_start_stop: String,
    pub hotkey_capture: String,
    /// Held only while a run with breakpoints is going: one block further, and on to the next breakpoint.
    pub hotkey_step: String,
    pub hotkey_continue: String,
    /// Held only while a recording is going: pause and resume, and the two that cut
    /// an image out of the screen and add a step that waits for it. Empty means "not assigned".
    #[serde(default = "default_record_pause")]
    pub hotkey_record_pause: String,
    #[serde(default = "default_record_wait_image")]
    pub hotkey_record_wait_image: String,
    #[serde(default = "default_record_click_image")]
    pub hotkey_record_click_image: String,
    /// Countdown before a manual start, 0..=10 seconds. 0 turns it off.
    pub countdown_sec: u32,
    /// Stop when the cursor hits the top-left corner of the primary monitor.
    pub corner_stop: bool,
    /// Executable names ("Do not run while these are running").
    pub blocked_processes: Vec<String>,
    /// Closing the main window quits instead of hiding to the tray.
    pub close_exits: bool,
    /// Run a missed daily or weekly slot once after startup.
    pub catch_up: bool,
    /// Ask GitHub for a newer version at startup. With it off, ClickIt does not
    /// contact GitHub by itself. The assistant is a separate, explicit request.
    pub check_updates: bool,
    /// Accept a local connection from `clickit-mcp`. Off by default.
    pub harness: bool,
    /// A harness or the assistant may start a scenario without the confirm dialog.
    pub harness_auto_run: bool,
    /// OpenAI-compatible base address, for example `http://127.0.0.1:11434/v1`. Empty means the assistant is off.
    pub agent_endpoint: String,
    /// Model name sent with every assistant request.
    pub agent_model: String,
    /// What Start, the hotkey and the tray "Start" item run.
    pub current: Option<RunTarget>,
}

fn default_record_pause() -> String {
    "F7".to_string()
}

fn default_record_wait_image() -> String {
    "Shift+F8".to_string()
}

fn default_record_click_image() -> String {
    "Ctrl+F8".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: None,
            hotkey_start_stop: "F6".to_string(),
            hotkey_capture: "F8".to_string(),
            hotkey_step: "F10".to_string(),
            hotkey_continue: "F9".to_string(),
            hotkey_record_pause: "F7".to_string(),
            hotkey_record_wait_image: "Shift+F8".to_string(),
            hotkey_record_click_image: "Ctrl+F8".to_string(),
            countdown_sec: 3,
            corner_stop: true,
            blocked_processes: Vec::new(),
            close_exits: false,
            catch_up: true,
            check_updates: true,
            harness: false,
            harness_auto_run: false,
            agent_endpoint: String::new(),
            agent_model: String::new(),
            current: None,
        }
    }
}

impl Settings {
    /// Language to show: the saved one, or English until it is chosen.
    pub fn lang(&self) -> &str {
        match self.language.as_deref() {
            Some("ru") => "ru",
            _ => "en",
        }
    }
}
