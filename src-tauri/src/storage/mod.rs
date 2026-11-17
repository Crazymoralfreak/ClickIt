//! Files under `%AppData%\ClickIt`. The disk is the single source of truth:
//! the engine, the tray and the scheduler read scenarios and queues from here.
//!
//! ```text
//! settings.json
//! schedule.json            rules
//! schedule-state.json      what already fired
//! scenarios\<id>\scenario.json  + template PNG files next to it
//! queues\<id>.json
//! journal.log, journal.prev.log
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::model::queue::Queue;
use crate::model::scenario::{new_id, Scenario, ScenarioMode};
use crate::model::schedule::ScheduleRule;
use crate::model::settings::Settings;

const DATA_DIR_NAME: &str = "ClickIt";
const SCENARIO_FILE: &str = "scenario.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioSummary {
    pub id: String,
    pub name: String,
    pub mode: ScenarioMode,
    pub step_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSummary {
    pub id: String,
    pub name: String,
    pub item_count: usize,
    pub broken: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct ScheduleFile {
    rules: Vec<ScheduleRule>,
}

/// What the scheduler remembers about one rule between launches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleState {
    /// The rule's target and timing as text; a change resets the state.
    pub fingerprint: String,
    /// When this version of the rule appeared, `YYYY-MM-DDTHH:MM:SS` local.
    pub since: String,
    /// The last slot that ran, was skipped or passed, `YYYY-MM-DDTHH:MM` local.
    #[serde(default)]
    pub last_slot: Option<String>,
}

pub type ScheduleState = BTreeMap<String, RuleState>;

pub struct Store {
    root: PathBuf,
}

/// Ids arrive from the UI and become file names: letters, digits, `-` and `_` only.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Template files are plain `*.png` names inside the scenario folder.
pub fn valid_template_name(name: &str) -> bool {
    let stem_ok = name
        .strip_suffix(".png")
        .is_some_and(|stem| !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    stem_ok && name.len() <= 80
}

fn checked_id(id: &str) -> AppResult<&str> {
    if valid_id(id) {
        Ok(id)
    } else {
        Err(AppError::new("notFound"))
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> AppResult<T> {
    let text = fs::read_to_string(path)?;
    // Files edited by hand may start with a byte order mark.
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}

/// Writes through a temporary file so that a crash never leaves half a file.
fn write_json<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    let text = serde_json::to_string_pretty(value)?;
    let temp = path.with_extension("tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, path)?;
    Ok(())
}

impl Store {
    /// Opens `%AppData%\ClickIt`, creating the folders on first use.
    pub fn open() -> AppResult<Self> {
        let app_data = std::env::var_os("APPDATA")
            .ok_or_else(|| AppError::new("io").with("message", "APPDATA is not set"))?;
        Self::at(PathBuf::from(app_data).join(DATA_DIR_NAME))
    }

    pub fn at(root: PathBuf) -> AppResult<Self> {
        fs::create_dir_all(root.join("scenarios"))?;
        fs::create_dir_all(root.join("queues"))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn journal_path(&self) -> PathBuf {
        self.root.join("journal.log")
    }

    pub fn journal_prev_path(&self) -> PathBuf {
        self.root.join("journal.prev.log")
    }

    // Settings

    /// A missing file gives defaults. A damaged one is kept as `settings.json.bad`
    /// and also gives defaults, so the app still starts.
    pub fn load_settings(&self) -> Settings {
        let path = self.root.join("settings.json");
        if !path.exists() {
            return Settings::default();
        }
        match read_json::<Settings>(&path) {
            Ok(settings) => settings,
            Err(_) => {
                let _ = fs::copy(&path, self.root.join("settings.json.bad"));
                Settings::default()
            }
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> AppResult<()> {
        write_json(&self.root.join("settings.json"), settings)
    }

    // Scenarios

    pub fn scenario_dir(&self, id: &str) -> PathBuf {
        self.root.join("scenarios").join(id)
    }

    pub fn scenario_exists(&self, id: &str) -> bool {
        valid_id(id) && self.scenario_dir(id).join(SCENARIO_FILE).is_file()
    }

    pub fn template_path(&self, scenario_id: &str, file: &str) -> AppResult<PathBuf> {
        if !valid_id(scenario_id) || !valid_template_name(file) {
            return Err(AppError::new("templateUnreadable").with("file", file));
        }
        Ok(self.scenario_dir(scenario_id).join(file))
    }

    pub fn template_exists(&self, scenario_id: &str, file: &str) -> bool {
        self.template_path(scenario_id, file).is_ok_and(|path| path.is_file())
    }

    pub fn load_scenario(&self, id: &str) -> AppResult<Scenario> {
        let path = self.scenario_dir(checked_id(id)?).join(SCENARIO_FILE);
        if !path.is_file() {
            return Err(AppError::new("notFound"));
        }
        let mut scenario: Scenario = read_json(&path)?;
        // The folder name is the identity, whatever the file says.
        scenario.id = id.to_string();
        Ok(scenario)
    }

    pub fn save_scenario(&self, scenario: &Scenario) -> AppResult<()> {
        let dir = self.scenario_dir(checked_id(&scenario.id)?);
        fs::create_dir_all(&dir)?;
        write_json(&dir.join(SCENARIO_FILE), scenario)
    }

    pub fn delete_scenario(&self, id: &str) -> AppResult<()> {
        let dir = self.scenario_dir(checked_id(id)?);
        if dir.is_dir() {
            fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    /// Scenarios sorted by name. Folders with a damaged file are left alone and not listed.
    pub fn list_scenarios(&self) -> Vec<ScenarioSummary> {
        let mut list: Vec<ScenarioSummary> = self
            .scenario_ids()
            .iter()
            .filter_map(|id| self.load_scenario(id).ok())
            .map(|scenario| ScenarioSummary {
                step_count: scenario.step_count(),
                id: scenario.id,
                name: scenario.name,
                mode: scenario.mode,
            })
            .collect();
        list.sort_by_key(|item| item.name.to_lowercase());
        list
    }

    fn scenario_ids(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.root.join("scenarios")) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| valid_id(name))
            .collect()
    }

    /// Copies a scenario with its templates under a new id and name.
    pub fn duplicate_scenario(&self, id: &str, name: &str) -> AppResult<Scenario> {
        let mut scenario = self.load_scenario(id)?;
        let source = self.scenario_dir(id);
        scenario.id = new_id("sc");
        scenario.name = name.to_string();
        scenario.renew_step_ids();
        let target = self.scenario_dir(&scenario.id);
        fs::create_dir_all(&target)?;
        for file in scenario.templates() {
            if valid_template_name(&file) && source.join(&file).is_file() {
                fs::copy(source.join(&file), target.join(&file))?;
            }
        }
        self.save_scenario(&scenario)?;
        Ok(scenario)
    }

    /// Removes template files that no step refers to. Run at startup only, so a
    /// template captured a moment ago is never taken from under the editor.
    pub fn sweep_templates(&self) {
        for id in self.scenario_ids() {
            let Ok(scenario) = self.load_scenario(&id) else {
                continue;
            };
            let used = scenario.templates();
            let Ok(entries) = fs::read_dir(self.scenario_dir(&id)) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if valid_template_name(&name) && !used.contains(&name) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    // Queues

    fn queue_path(&self, id: &str) -> PathBuf {
        self.root.join("queues").join(format!("{id}.json"))
    }

    pub fn load_queue(&self, id: &str) -> AppResult<Queue> {
        let path = self.queue_path(checked_id(id)?);
        if !path.is_file() {
            return Err(AppError::new("notFound"));
        }
        let mut queue: Queue = read_json(&path)?;
        queue.id = id.to_string();
        Ok(queue)
    }

    pub fn save_queue(&self, queue: &Queue) -> AppResult<()> {
        write_json(&self.queue_path(checked_id(&queue.id)?), queue)
    }

    pub fn delete_queue(&self, id: &str) -> AppResult<()> {
        let path = self.queue_path(checked_id(id)?);
        if path.is_file() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn list_queues(&self) -> Vec<QueueSummary> {
        let Ok(entries) = fs::read_dir(self.root.join("queues")) else {
            return Vec::new();
        };
        let mut list: Vec<QueueSummary> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let is_json = path.extension().is_some_and(|ext| ext == "json");
                let id = path.file_stem()?.to_str()?.to_string();
                (is_json && valid_id(&id)).then_some(id)
            })
            .filter_map(|id| self.load_queue(&id).ok())
            .map(|queue| QueueSummary {
                broken: queue.items.iter().any(|item| !self.scenario_exists(&item.scenario_id)),
                item_count: queue.items.len(),
                id: queue.id,
                name: queue.name,
            })
            .collect();
        list.sort_by_key(|item| item.name.to_lowercase());
        list
    }

    // Schedule

    pub fn load_schedule(&self) -> Vec<ScheduleRule> {
        read_json::<ScheduleFile>(&self.root.join("schedule.json")).map(|file| file.rules).unwrap_or_default()
    }

    pub fn save_schedule(&self, rules: &[ScheduleRule]) -> AppResult<()> {
        write_json(&self.root.join("schedule.json"), &ScheduleFile { rules: rules.to_vec() })
    }

    pub fn load_schedule_state(&self) -> ScheduleState {
        read_json(&self.root.join("schedule-state.json")).unwrap_or_default()
    }

    pub fn save_schedule_state(&self, state: &ScheduleState) -> AppResult<()> {
        write_json(&self.root.join("schedule-state.json"), state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_template_names_cannot_leave_their_folder() {
        assert!(valid_id("sc18f2a0b3c4d"));
        assert!(!valid_id(""));
        assert!(!valid_id(".."));
        assert!(!valid_id("a/b"));
        assert!(!valid_id("a\\b"));
        assert!(valid_template_name("tpl-1a2b.png"));
        assert!(!valid_template_name("..\\x.png"));
        assert!(!valid_template_name("x.exe"));
        assert!(!valid_template_name(".png"));
    }
}
