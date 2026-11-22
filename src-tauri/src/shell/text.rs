//! Texts shown outside the web view: the tray, notifications, file dialogs.
//! They come from the same dictionaries as the interface.

use std::sync::LazyLock;

use serde_json::Value;

use crate::error::AppError;
use crate::runtime::{RunState, RunStatus};

static RU: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/ru.json")));
static EN: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/en.json")));
static DE: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/de.json")));
static ES: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/es.json")));
static FR: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/fr.json")));
static PT: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/pt.json")));
static PL: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/pl.json")));
static TR: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/tr.json")));
static ZH: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/zh.json")));
static JA: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/ja.json")));
static KO: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../../src/i18n/ko.json")));

const LANGUAGES: &[&str] = &["ru", "en", "de", "es", "fr", "pt", "pl", "tr", "zh", "ja", "ko"];

fn parse(text: &str) -> Value {
    serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap_or(Value::Null)
}

fn dictionary(lang: &str) -> &'static Value {
    match lang {
        "ru" => &RU,
        "de" => &DE,
        "es" => &ES,
        "fr" => &FR,
        "pt" => &PT,
        "pl" => &PL,
        "tr" => &TR,
        "zh" => &ZH,
        "ja" => &JA,
        "ko" => &KO,
        _ => &EN,
    }
}

/// Whether ClickIt has a dictionary for this language code.
pub fn known_language(lang: &str) -> bool {
    LANGUAGES.contains(&lang)
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
    let template = lookup(dictionary(lang), key).or_else(|| lookup(&EN, key)).unwrap_or(key);
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
        RunState::PausedBreak => text(lang, "status.pausedBreak", &[]),
        RunState::Recording if status.recording.as_ref().is_some_and(|recording| recording.paused) => {
            text(lang, "status.recordingPaused", &[])
        }
        RunState::Recording if status.recording.as_ref().is_some_and(|recording| recording.held) => {
            text(lang, "status.recordingHeld", &[])
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
        "native.notify.recordImage",
        "native.notify.recordImageText",
        "native.notify.named",
        "native.notify.updateAvailable",
        "native.notify.updateAvailableText",
        "native.notify.updateInstalling",
        "native.notify.updateInstallingText",
        "native.dialog.exportTitle",
        "native.dialog.importTitle",
        "native.dialog.fileType",
        "native.dialog.commandFileTitle",
        "native.dialog.commandFileType",
        "native.dialog.commandFolderTitle",
        "native.dialog.allFiles",
        "native.dialog.importCommandsTitle",
        "native.dialog.importCommandsText",
        "native.dialog.importCommandsMore",
        "native.dialog.importCommandsOk",
        "native.dialog.importCommandsCancel",
        "status.idle",
        "status.countdown",
        "status.running",
        "status.waiting",
        "status.pausedWindow",
        "status.pausedProcess",
        "status.pausedBreak",
        "status.recording",
        "status.recordingPaused",
        "status.recordingHeld",
        "journal.recordReasons.timeLimit",
        "journal.recordReasons.stepLimit",
        "scenarios.canvasCopyName",
        "errors.unknown",
    ];

    /// Error codes the core can return or put into the journal.
    const ERROR_CODES: &[&str] = &[
        "io",
        "badFile",
        "notFound",
        "nameEmpty",
        "busy",
        "updating",
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
        "stale",
        "pending",
        "agentUnconfigured",
        "agentBusy",
        "agentFailed",
        "dialogOpen",
        "chatBusy",
        "chatMissing",
        "alreadyCanvas",
        "backupMissing",
        "formatMismatch",
        "callMissing",
        "callTooDeep",
        "extractEmpty",
        "extractEntry",
        "extractManyEntries",
        "extractManyExits",
        "folderMissing",
        "folderTooDeep",
        "folderIntoItself",
        "commandFailed",
        "commandTimeout",
        "commandStartFailed",
        "commandsDisabled",
        "commandsDeclined",
    ];

    /// `{name}` placeholders that appear in a string, sorted, so word order may differ.
    fn placeholders(text: &str) -> Vec<&str> {
        let mut names = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            let after = &rest[start + 1..];
            let Some(end) = after.find('}') else {
                break;
            };
            let name = &after[..end];
            if !name.is_empty()
                && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                && !names.contains(&name)
            {
                names.push(name);
            }
            rest = &after[end + 1..];
        }
        names.sort_unstable();
        names
    }

    fn leaf_strings(value: &Value, prefix: &str, out: &mut Vec<(String, String)>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                    leaf_strings(child, &path, out);
                }
            }
            Value::String(text) => out.push((prefix.to_string(), text.clone())),
            _ => {}
        }
    }

    #[test]
    fn every_dictionary_has_the_same_keys_and_placeholders() {
        let mut english = Vec::new();
        leaf_strings(&EN, "", &mut english);
        assert!(!english.is_empty());
        let english_map: std::collections::HashMap<&str, &str> =
            english.iter().map(|(key, text)| (key.as_str(), text.as_str())).collect();
        for (lang, dictionary) in [
            ("ru", &*RU),
            ("de", &*DE),
            ("es", &*ES),
            ("fr", &*FR),
            ("pt", &*PT),
            ("pl", &*PL),
            ("tr", &*TR),
            ("zh", &*ZH),
            ("ja", &*JA),
            ("ko", &*KO),
        ] {
            assert!(dictionary.is_object(), "{lang} is not valid JSON");
            let mut leaves = Vec::new();
            leaf_strings(dictionary, "", &mut leaves);
            assert_eq!(leaves.len(), english.len(), "{lang} has a different number of strings");
            for (key, text) in &leaves {
                let Some(en_text) = english_map.get(key.as_str()) else {
                    panic!("{lang} has unexpected key {key}");
                };
                assert_eq!(
                    placeholders(text),
                    placeholders(en_text),
                    "{lang} placeholders differ for {key}"
                );
            }
            for key in NATIVE_KEYS {
                assert!(lookup(dictionary, key).is_some(), "{lang} missing {key}");
            }
            for code in ERROR_CODES {
                assert!(
                    lookup(dictionary, &format!("errors.{code}")).is_some(),
                    "{lang} missing errors.{code}"
                );
            }
        }
    }

    #[test]
    fn placeholders_are_filled_and_unknown_codes_do_not_leak_keys() {
        assert_eq!(text("en", "native.tray.start", &[("name", "Demo")]), "Start: Demo");
        assert_eq!(text("ru", "native.tray.start", &[("name", "Demo")]), "Старт: Demo");
        assert_ne!(text("de", "native.tray.quit", &[]), text("en", "native.tray.quit", &[]));
        assert_eq!(text("xx", "native.tray.quit", &[]), text("en", "native.tray.quit", &[]));
        assert!(known_language("de") && !known_language("xx"));
        let message = error_text("en", &AppError::new("somethingNew"));
        assert!(message.contains("somethingNew") && !message.contains("errors."));
        let blocked = error_text("en", &AppError::new("blockedProcess").with("process", "game.exe"));
        assert!(blocked.contains("game.exe"));
    }
}
