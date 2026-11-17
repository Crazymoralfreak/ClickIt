//! Texts shown outside the web view: the tray, notifications, file dialogs.
//! They come from the same dictionaries as the interface.

use std::sync::LazyLock;

use serde_json::Value;

use crate::error::AppError;
use crate::runtime::{RunState, RunStatus};

static RU: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/ru.json")));
static EN: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/en.json")));

fn parse(text: &str) -> Value {
    serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap_or(Value::Null)
}

fn lookup<'a>(dictionary: &'a Value, key: &str) -> Option<&'a str> {
    key.split('.').try_fold(dictionary, |node, part| node.get(part))?.as_str()
}

fn fill(template: &str, params: &[(&str, &str)]) -> String {
    params.iter().fold(template.to_string(), |text, (key, value)| text.replace(&format!("{{{key}}}"), value))
}

/// Text for a key such as `native.tray.open`. A key missing in the language
/// falls back to English, a key missing everywhere is shown as it is.
pub fn text(lang: &str, key: &str, params: &[(&str, &str)]) -> String {
    let dictionary: &Value = if lang == "ru" { &RU } else { &EN };
    let template = lookup(dictionary, key).or_else(|| lookup(&EN, key)).unwrap_or(key);
    fill(template, params)
}

pub fn error_text(lang: &str, error: &AppError) -> String {
    let key = format!("errors.{}", error.code);
    let known = lookup(&EN, &key).is_some();
    if !known {
        return text(lang, "errors.unknown", &[("message", &error.code)]);
    }
    let params: Vec<(&str, &str)> = error.params.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
    text(lang, &key, &params)
}

/// One line about what ClickIt is doing, for the tooltip of the tray icon.
pub fn status_text(lang: &str, status: &RunStatus) -> String {
    let headline = match status.state {
        RunState::Idle => text(lang, "status.idle", &[]),
        RunState::Countdown => {
            let seconds = status.countdown_left.unwrap_or(0).to_string();
            text(lang, "status.countdown", &[("seconds", &seconds)])
        }
        RunState::Running => text(lang, "status.running", &[]),
        RunState::Waiting => text(lang, "status.waiting", &[]),
        RunState::PausedWindow => text(lang, "status.pausedWindow", &[]),
        RunState::PausedProcess => {
            text(lang, "status.pausedProcess", &[("process", status.blocked_process.as_deref().unwrap_or(""))])
        }
        RunState::Recording if status.recording.as_ref().is_some_and(|recording| recording.paused) => {
            text(lang, "status.recordingPaused", &[])
        }
        RunState::Recording => text(lang, "status.recording", &[]),
    };
    let name = status.target.as_ref().map(|target| target.name.as_str()).or(status.scenario_name.as_deref());
    let line = match name {
        Some(name) => format!("ClickIt · {headline} · {name}"),
        None => format!("ClickIt · {headline}"),
    };
    // Windows cuts a tooltip at 127 characters.
    line.chars().take(120).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NATIVE_KEYS: &[&str] = &[
        "native.tray.open",
        "native.tray.start",
        "native.tray.startNone",
        "native.tray.stop",
        "native.tray.stopRecording",
        "native.tray.scenarios",
        "native.tray.queues",
        "native.tray.empty",
        "native.tray.quit",
        "native.notify.scheduleSkipped",
        "native.notify.startFailed",
        "native.notify.runFailed",
        "native.notify.recordStopped",
        "native.notify.named",
        "native.dialog.exportTitle",
        "native.dialog.importTitle",
        "native.dialog.fileType",
        "status.idle",
        "status.countdown",
        "status.running",
        "status.waiting",
        "status.pausedWindow",
        "status.pausedProcess",
        "status.recording",
        "status.recordingPaused",
        "journal.recordReasons.timeLimit",
        "journal.recordReasons.stepLimit",
        "errors.unknown",
    ];

    /// Error codes the core can return or put into the journal.
    const ERROR_CODES: &[&str] = &[
        "io",
        "badFile",
        "notFound",
        "nameEmpty",
        "busy",
        "invalid",
        "queueInvalid",
        "nothingToRun",
        "blockedProcess",
        "targetNotFound",
        "targetNotFocused",
        "hotkeyInvalid",
        "hotkeyTaken",
        "hotkeySame",
        "importBad",
        "exportFailed",
        "captureFailed",
        "captureOutsideTarget",
        "regionTooSmall",
        "stepLimit",
        "templateUnreadable",
        "autostartFailed",
        "scheduleInvalid",
        "recordFailed",
        "userStop",
        "cornerStop",
        "stopStep",
        "conditionTimeout",
        "targetClosed",
        "pointOutsideTarget",
        "inputFailed",
        "appExit",
    ];

    #[test]
    fn both_dictionaries_have_every_text_the_core_shows() {
        for dictionary in [&*RU, &*EN] {
            assert!(dictionary.is_object(), "a dictionary is not valid JSON");
            for key in NATIVE_KEYS {
                assert!(lookup(dictionary, key).is_some(), "missing {key}");
            }
            for code in ERROR_CODES {
                assert!(lookup(dictionary, &format!("errors.{code}")).is_some(), "missing errors.{code}");
            }
        }
    }

    #[test]
    fn placeholders_are_filled_and_unknown_codes_do_not_leak_keys() {
        assert_eq!(text("en", "native.tray.start", &[("name", "Demo")]), "Start: Demo");
        assert_eq!(text("ru", "native.tray.start", &[("name", "Demo")]), "Старт: Demo");
        assert_eq!(text("de", "native.tray.quit", &[]), text("en", "native.tray.quit", &[]));
        let message = error_text("en", &AppError::new("somethingNew"));
        assert!(message.contains("somethingNew") && !message.contains("errors."));
        let blocked = error_text("en", &AppError::new("blockedProcess").with("process", "game.exe"));
        assert!(blocked.contains("game.exe"));
    }
}
