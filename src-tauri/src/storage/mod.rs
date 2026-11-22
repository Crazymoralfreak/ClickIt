//! Files under `%AppData%\ClickIt`. The disk is the single source of truth:
//! the engine, the tray and the scheduler read scenarios and queues from here.
//!
//! ```text
//! settings.json
//! schedule.json            rules
//! schedule-state.json      what already fired
//! scenarios\<id>\scenario.json  a list of steps, + template PNG files next to it
//! scenarios\<id>\graph.json     a graph instead; such a folder has no scenario.json
//! scenarios\<id>\steps.backup.json  the list a graph was made from
//! folders.json            folders of the list of scenarios and what lies where
//! queues\<id>.json
//! snippets\<id>\snippet.json    the user's canvas snippets with their templates
//! chats\<id>.jsonl
//! journal.log, journal.prev.log
//! ```
//!
//! A graph lives in a file of its own because ClickIt 1.0.x does not know the
//! `graph` field and would save such a scenario without it. Without a
//! `scenario.json` an old copy simply does not list the scenario.

pub mod folders;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::model::graph::{self, Edge, Graph, Node, GRAPH_VERSION};
use crate::model::queue::Queue;
use crate::model::scenario::{new_id, Scenario, ScenarioMode, SCENARIO_VERSION};
use crate::model::schedule::ScheduleRule;
use crate::model::settings::Settings;
use crate::model::validate::{validate_with, Issue, Lookup};

const DATA_DIR_NAME: &str = "ClickIt";
const SCENARIO_FILE: &str = "scenario.json";
const GRAPH_FILE: &str = "graph.json";
/// The list a graph was converted from, kept for "Return the list".
const BACKUP_FILE: &str = "steps.backup.json";
const SNIPPET_FILE: &str = "snippet.json";
/// A template younger than this may have been cut a moment ago for an editor
/// that has not saved yet.
pub const FRESH_TEMPLATE: Duration = Duration::from_secs(60 * 60);
/// A temporary file younger than this may still be on its way to its real name.
const FRESH_TEMP: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioSummary {
    pub id: String,
    pub name: String,
    pub mode: ScenarioMode,
    /// Steps of a list or blocks of a graph.
    pub step_count: usize,
    /// `"list"` or `"canvas"`.
    pub format: &'static str,
    /// The folder the scenario lies in; `None` at the top of the list.
    pub folder_id: Option<String>,
    /// Names of the folders from the top of the list down to that one.
    pub folder_path: Vec<String>,
}

/// A piece of a graph the user saved to put into other scenarios.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

impl Snippet {
    fn templates(&self) -> Vec<String> {
        let mut out = Vec::new();
        Graph { nodes: self.nodes.clone(), edges: Vec::new(), notes: Vec::new() }.collect_templates(&mut out);
        out.sort();
        out.dedup();
        out
    }
}

/// What one "Clear" button of the settings removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanupKind {
    Journal,
    Chats,
    Connections,
    Templates,
    Backups,
    Cache,
    Leftovers,
}

impl CleanupKind {
    pub const ALL: [CleanupKind; 7] = [
        CleanupKind::Journal,
        CleanupKind::Chats,
        CleanupKind::Connections,
        CleanupKind::Templates,
        CleanupKind::Backups,
        CleanupKind::Cache,
        CleanupKind::Leftovers,
    ];
}

/// Size of what a "Clear" button would remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub bytes: u64,
    pub files: usize,
}

impl Usage {
    pub fn of(paths: &[PathBuf]) -> Self {
        let mut usage = Usage::default();
        for path in paths {
            if let Ok(meta) = fs::metadata(path) {
                usage.bytes += meta.len();
                usage.files += 1;
            }
        }
        usage
    }

    /// Everything under a folder, nested folders included.
    pub fn of_dir(dir: &Path) -> Self {
        let mut usage = Usage::default();
        let Ok(entries) = fs::read_dir(dir) else {
            return usage;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let inner = Usage::of_dir(&path);
                usage.bytes += inner.bytes;
                usage.files += inner.files;
            } else if let Ok(meta) = entry.metadata() {
                usage.bytes += meta.len();
                usage.files += 1;
            }
        }
        usage
    }
}

fn older_than(path: &Path, age: Duration) -> bool {
    let modified = fs::metadata(path).and_then(|meta| meta.modified());
    modified.is_ok_and(|at| SystemTime::now().duration_since(at).is_ok_and(|passed| passed >= age))
}

/// Removes the files and returns how many of them are gone.
pub fn remove_files(paths: &[PathBuf]) -> usize {
    paths.iter().filter(|path| fs::remove_file(path).is_ok()).count()
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
    /// Held while `folders.json` is read, changed and written.
    folders_lock: Mutex<()>,
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

/// Files right inside a folder that `keep` accepts.
fn files_in(dir: &Path, keep: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries.flatten().map(|entry| entry.path()).filter(|path| path.is_file() && keep(path)).collect()
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
        Ok(Self { root, folders_lock: Mutex::new(()) })
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

    pub fn harness_token_path(&self) -> PathBuf {
        self.root.join("harness.token")
    }

    pub fn agent_key_path(&self) -> PathBuf {
        self.root.join("agent.key")
    }

    /// The single conversation of ClickIt 1.0.x. It is moved into `chats` on first use.
    pub fn agent_transcript_path(&self) -> PathBuf {
        self.root.join("agent.jsonl")
    }

    /// Conversations with the model, one `<id>.jsonl` per chat. Text only.
    pub fn chats_dir(&self) -> PathBuf {
        self.root.join("chats")
    }

    /// Text of external harness dialogs. Pictures are not stored here.
    pub fn harness_dialog_path(&self) -> PathBuf {
        self.root.join("harness.jsonl")
    }

    /// The secret the local pipe asks for. Created the first time it is needed.
    pub fn harness_token(&self) -> AppResult<String> {
        let path = self.harness_token_path();
        if let Ok(text) = fs::read_to_string(&path) {
            let token = text.trim();
            if token.len() >= 32 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Ok(token.to_string());
            }
        }
        let token: String = (0..32).map(|_| format!("{:02x}", fastrand::u8(..))).collect();
        fs::write(&path, &token)?;
        Ok(token)
    }

    pub fn agent_key(&self) -> String {
        fs::read_to_string(self.agent_key_path()).unwrap_or_default().trim().to_string()
    }

    pub fn set_agent_key(&self, key: &str) -> AppResult<()> {
        let key = key.trim();
        if key.is_empty() {
            let path = self.agent_key_path();
            if path.exists() {
                fs::remove_file(path)?;
            }
            return Ok(());
        }
        fs::write(self.agent_key_path(), key)?;
        Ok(())
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
        if !valid_id(id) {
            return false;
        }
        let dir = self.scenario_dir(id);
        dir.join(GRAPH_FILE).is_file() || dir.join(SCENARIO_FILE).is_file()
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

    /// Reads `graph.json` when the folder has one, `scenario.json` otherwise.
    pub fn load_scenario(&self, id: &str) -> AppResult<Scenario> {
        let dir = self.scenario_dir(checked_id(id)?);
        let graph_path = dir.join(GRAPH_FILE);
        let is_graph = graph_path.is_file();
        let path = if is_graph { graph_path } else { dir.join(SCENARIO_FILE) };
        if !path.is_file() {
            return Err(AppError::new("notFound"));
        }
        let mut scenario: Scenario = read_json(&path)?;
        // The folder name is the identity, whatever the file says.
        scenario.id = id.to_string();
        if is_graph {
            scenario.graph.get_or_insert_with(Graph::default);
            scenario.steps.clear();
        }
        Ok(scenario)
    }

    /// Writes the file of the scenario's form. A list that turns into a graph
    /// leaves its `scenario.json` behind as the backup list.
    pub fn save_scenario(&self, scenario: &Scenario) -> AppResult<()> {
        let dir = self.scenario_dir(checked_id(&scenario.id)?);
        fs::create_dir_all(&dir)?;
        let list = dir.join(SCENARIO_FILE);
        if scenario.graph.is_none() {
            write_json(&list, scenario)?;
            let graph = dir.join(GRAPH_FILE);
            if graph.is_file() {
                fs::remove_file(graph)?;
            }
            return Ok(());
        }
        write_json(&dir.join(GRAPH_FILE), scenario)?;
        if list.is_file() {
            let backup = dir.join(BACKUP_FILE);
            if backup.is_file() {
                fs::remove_file(&list)?;
            } else {
                fs::rename(&list, backup)?;
            }
        }
        Ok(())
    }

    pub fn delete_scenario(&self, id: &str) -> AppResult<()> {
        let dir = self.scenario_dir(checked_id(id)?);
        if dir.is_dir() {
            fs::remove_dir_all(dir)?;
        }
        self.forget_placement(id);
        Ok(())
    }

    /// Scenarios sorted by name. Folders with a damaged file are left alone and not listed.
    pub fn list_scenarios(&self) -> Vec<ScenarioSummary> {
        let folders = self.load_folders();
        let mut list: Vec<ScenarioSummary> = self
            .scenario_ids()
            .iter()
            .filter_map(|id| self.load_scenario(id).ok())
            .map(|scenario| {
                let folder_id = folders.placement.get(&scenario.id).cloned();
                ScenarioSummary {
                    step_count: scenario.step_count(),
                    format: scenario.format(),
                    folder_path: folder_id.as_deref().map(|id| folders.path(id)).unwrap_or_default(),
                    folder_id,
                    id: scenario.id,
                    name: scenario.name,
                    mode: scenario.mode,
                }
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

    /// Issues of a scenario, with the other scenarios and the template files
    /// looked up on the disk.
    pub fn issues(&self, scenario: &Scenario) -> Vec<Issue> {
        validate_with(
            scenario,
            &Lookup {
                template_exists: &|id, file| self.template_exists(id, file),
                scenario: &|id| self.load_scenario(id).ok(),
            },
        )
    }

    /// Scenarios whose graph runs the given one, sorted by name.
    pub fn callers(&self, id: &str) -> Vec<ScenarioSummary> {
        let calling: Vec<String> = self
            .scenario_ids()
            .into_iter()
            .filter(|other| other != id)
            .filter_map(|other| self.load_scenario(&other).ok())
            .filter(|scenario| scenario.graph.as_ref().is_some_and(|graph| graph.callees().iter().any(|c| c == id)))
            .map(|scenario| scenario.id)
            .collect();
        self.list_scenarios().into_iter().filter(|summary| calling.contains(&summary.id)).collect()
    }

    /// Copies the template files `scenario` refers to out of another scenario's folder.
    fn copy_templates(&self, from_id: &str, scenario: &Scenario) -> AppResult<()> {
        let source = self.scenario_dir(from_id);
        let target = self.scenario_dir(&scenario.id);
        fs::create_dir_all(&target)?;
        for file in scenario.templates() {
            if valid_template_name(&file) && source.join(&file).is_file() {
                fs::copy(source.join(&file), target.join(&file))?;
            }
        }
        Ok(())
    }

    /// Copies a scenario with its templates under a new id and name. The
    /// backup list of a converted scenario is not copied.
    pub fn duplicate_scenario(&self, id: &str, name: &str) -> AppResult<Scenario> {
        let mut scenario = self.load_scenario(id)?;
        scenario.id = new_id("sc");
        scenario.name = name.to_string();
        scenario.revision = 0;
        scenario.renew_step_ids();
        self.copy_templates(id, &scenario)?;
        self.save_scenario(&scenario)?;
        self.place_beside(&scenario.id, id);
        Ok(scenario)
    }

    // List and canvas

    pub fn has_backup(&self, id: &str) -> bool {
        valid_id(id) && self.scenario_dir(id).join(BACKUP_FILE).is_file()
    }

    fn backup(&self, id: &str) -> Option<Scenario> {
        read_json(&self.scenario_dir(id).join(BACKUP_FILE)).ok()
    }

    /// Turns a list into a graph in place. The list stays next to it as the
    /// backup "Return the list" reads.
    pub fn to_canvas(&self, id: &str) -> AppResult<Scenario> {
        let mut scenario = self.load_scenario(id)?;
        if scenario.graph.is_some() {
            return Err(AppError::new("alreadyCanvas"));
        }
        scenario.graph = Some(graph::from_steps(&scenario.steps));
        scenario.steps.clear();
        scenario.version = GRAPH_VERSION;
        scenario.revision = scenario.revision.saturating_add(1);
        self.save_scenario(&scenario)?;
        Ok(scenario)
    }

    /// Makes a graph out of a list as a new scenario; the list stays as it was.
    pub fn to_canvas_copy(&self, id: &str, name: &str) -> AppResult<Scenario> {
        let mut scenario = self.load_scenario(id)?;
        if scenario.graph.is_some() {
            return Err(AppError::new("alreadyCanvas"));
        }
        scenario.id = new_id("sc");
        scenario.name = name.to_string();
        scenario.revision = 0;
        scenario.renew_step_ids();
        scenario.graph = Some(graph::from_steps(&scenario.steps));
        scenario.steps.clear();
        scenario.version = GRAPH_VERSION;
        self.copy_templates(id, &scenario)?;
        self.save_scenario(&scenario)?;
        self.place_beside(&scenario.id, id);
        Ok(scenario)
    }

    /// Brings back the list a graph was made from. What was changed on the
    /// canvas is lost; the name of the scenario stays.
    pub fn to_list(&self, id: &str) -> AppResult<Scenario> {
        let current = self.load_scenario(id)?;
        let dir = self.scenario_dir(id);
        let backup = dir.join(BACKUP_FILE);
        if current.graph.is_none() || !backup.is_file() {
            return Err(AppError::new("backupMissing"));
        }
        let mut list: Scenario = read_json(&backup)?;
        list.id = id.to_string();
        list.name = current.name;
        list.graph = None;
        list.version = SCENARIO_VERSION;
        list.revision = current.revision.saturating_add(1);
        self.save_scenario(&list)?;
        fs::remove_file(backup)?;
        Ok(list)
    }

    /// Saves a scenario whose blocks were moved out, and the new scenario
    /// that got them together with the template images they refer to.
    pub fn save_extracted(&self, source: &Scenario, created: &Scenario) -> AppResult<()> {
        let stored = self.copy_templates(&source.id, created).and_then(|()| self.save_scenario(created));
        let stored = stored.and_then(|()| self.save_scenario(source));
        if stored.is_err() {
            let _ = self.delete_scenario(&created.id);
        } else {
            self.place_beside(&created.id, &source.id);
        }
        stored
    }

    // Templates nothing refers to

    /// Template files that no step, no block and no backup list refers to.
    /// `min_age` leaves out the ones that were written a moment ago.
    pub fn unused_templates(&self, min_age: Option<Duration>) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for id in self.scenario_ids() {
            let Ok(scenario) = self.load_scenario(&id) else {
                continue;
            };
            let mut used = scenario.templates();
            if let Some(backup) = self.backup(&id) {
                used.extend(backup.templates());
            }
            let Ok(entries) = fs::read_dir(self.scenario_dir(&id)) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if !valid_template_name(&name) || used.contains(&name) {
                    continue;
                }
                let path = entry.path();
                if min_age.is_none_or(|age| older_than(&path, age)) {
                    out.push(path);
                }
            }
        }
        out
    }

    /// Removes template files that nothing refers to. Run at startup, so a
    /// template captured a moment ago is never taken from under the editor.
    pub fn sweep_templates(&self) {
        remove_files(&self.unused_templates(None));
    }

    // Cleanup

    /// Files a "Clear" button of the settings removes. The kinds that are not
    /// plain files of this folder (connections, the window cache) give nothing.
    pub fn cleanup_files(&self, kind: CleanupKind) -> Vec<PathBuf> {
        let existing = |paths: Vec<PathBuf>| paths.into_iter().filter(|path| path.is_file()).collect();
        match kind {
            CleanupKind::Journal => existing(vec![self.journal_path(), self.journal_prev_path()]),
            CleanupKind::Chats => files_in(&self.chats_dir(), &|_| true),
            CleanupKind::Templates => self.unused_templates(Some(FRESH_TEMPLATE)),
            CleanupKind::Backups => existing(
                self.scenario_ids().iter().map(|id| self.scenario_dir(id).join(BACKUP_FILE)).collect(),
            ),
            CleanupKind::Leftovers => {
                let temp = |path: &Path| {
                    path.extension().is_some_and(|ext| ext == "tmp") && older_than(path, FRESH_TEMP)
                };
                let mut out: Vec<PathBuf> = existing(vec![self.root.join("settings.json.bad")]);
                let mut dirs = vec![self.root.clone(), self.root.join("queues"), self.chats_dir()];
                dirs.extend(self.scenario_ids().iter().map(|id| self.scenario_dir(id)));
                dirs.extend(self.snippet_ids().iter().map(|id| self.snippet_dir(id)));
                for dir in dirs {
                    out.extend(files_in(&dir, &temp));
                }
                out
            }
            CleanupKind::Connections | CleanupKind::Cache => Vec::new(),
        }
    }

    // Snippets

    fn snippet_dir(&self, id: &str) -> PathBuf {
        self.root.join("snippets").join(id)
    }

    fn snippet_ids(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.root.join("snippets")) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| valid_id(name))
            .collect()
    }

    /// The user's snippets, sorted by name.
    pub fn list_snippets(&self) -> Vec<Snippet> {
        let mut list: Vec<Snippet> = self
            .snippet_ids()
            .into_iter()
            .filter_map(|id| {
                let mut snippet: Snippet = read_json(&self.snippet_dir(&id).join(SNIPPET_FILE)).ok()?;
                snippet.id = id;
                Some(snippet)
            })
            .collect();
        list.sort_by_key(|item| item.name.to_lowercase());
        list
    }

    /// Saves blocks of a scenario as a snippet, with the template images they refer to.
    pub fn save_snippet(&self, scenario_id: &str, name: &str, nodes: Vec<Node>, edges: Vec<Edge>) -> AppResult<Snippet> {
        let source = self.scenario_dir(checked_id(scenario_id)?);
        let snippet = Snippet { id: new_id("sn"), name: name.to_string(), nodes, edges };
        let dir = self.snippet_dir(&snippet.id);
        fs::create_dir_all(&dir)?;
        let stored = (|| {
            for file in snippet.templates() {
                if valid_template_name(&file) && source.join(&file).is_file() {
                    fs::copy(source.join(&file), dir.join(&file))?;
                }
            }
            write_json(&dir.join(SNIPPET_FILE), &snippet)
        })();
        if let Err(error) = stored {
            let _ = fs::remove_dir_all(&dir);
            return Err(error);
        }
        Ok(snippet)
    }

    /// Copies the template images of a snippet into a scenario that is about to get its blocks.
    pub fn use_snippet(&self, id: &str, scenario_id: &str) -> AppResult<Snippet> {
        let dir = self.snippet_dir(checked_id(id)?);
        let mut snippet: Snippet = read_json(&dir.join(SNIPPET_FILE)).map_err(|_| AppError::new("notFound"))?;
        snippet.id = id.to_string();
        let target = self.scenario_dir(checked_id(scenario_id)?);
        if !target.is_dir() {
            return Err(AppError::new("notFound"));
        }
        for file in snippet.templates() {
            if valid_template_name(&file) && dir.join(&file).is_file() && !target.join(&file).is_file() {
                fs::copy(dir.join(&file), target.join(&file))?;
            }
        }
        Ok(snippet)
    }

    pub fn delete_snippet(&self, id: &str) -> AppResult<()> {
        let dir = self.snippet_dir(checked_id(id)?);
        if dir.is_dir() {
            fs::remove_dir_all(dir)?;
        }
        Ok(())
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

    use crate::model::graph::{NodeKind, Port};
    use crate::model::scenario::{Area, Condition, OnFound, OnTimeout, Step, StepKind};

    fn temp_store() -> Store {
        Store::at(std::env::temp_dir().join(format!("clickit-store-{}", new_id("t")))).unwrap()
    }

    fn wait_for(template: &str) -> Step {
        Step::new(StepKind::Wait {
            condition: Condition::ImageFound { template: template.to_string(), area: Area::Screen, threshold: 0.9 },
            poll_ms: 200,
            timeout_ms: 0,
            on_timeout: OnTimeout::Stop,
            on_found: OnFound::None,
        })
    }

    fn touch(store: &Store, id: &str, file: &str) -> PathBuf {
        let path = store.scenario_dir(id).join(file);
        fs::write(&path, b"png").unwrap();
        path
    }

    #[test]
    fn a_list_and_a_graph_live_in_files_of_their_own() {
        let store = temp_store();
        let list = Scenario::new("List");
        let canvas = Scenario::new_canvas("Canvas");
        store.save_scenario(&list).unwrap();
        store.save_scenario(&canvas).unwrap();

        assert!(store.scenario_dir(&list.id).join(SCENARIO_FILE).is_file());
        assert!(!store.scenario_dir(&list.id).join(GRAPH_FILE).exists());
        assert!(store.scenario_dir(&canvas.id).join(GRAPH_FILE).is_file());
        // A copy of ClickIt that does not know graphs must not see this folder as a scenario.
        assert!(!store.scenario_dir(&canvas.id).join(SCENARIO_FILE).exists());
        assert!(store.scenario_exists(&canvas.id));

        assert_eq!(store.load_scenario(&list.id).unwrap(), list);
        assert_eq!(store.load_scenario(&canvas.id).unwrap(), canvas);
        let formats: Vec<_> = store.list_scenarios().into_iter().map(|item| (item.name, item.format)).collect();
        assert_eq!(formats, vec![("Canvas".to_string(), "canvas"), ("List".to_string(), "list")]);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn a_list_turns_into_a_graph_and_comes_back() {
        let store = temp_store();
        let mut list = Scenario::new("Arena");
        list.steps = vec![wait_for("tpl-a.png"), Step::new(StepKind::Stop)];
        store.save_scenario(&list).unwrap();
        touch(&store, &list.id, "tpl-a.png");

        let canvas = store.to_canvas(&list.id).unwrap();
        assert_eq!(canvas.id, list.id);
        assert_eq!(canvas.version, GRAPH_VERSION);
        assert_eq!(canvas.revision, list.revision + 1);
        assert!(canvas.steps.is_empty());
        assert_eq!(canvas.graph.as_ref().unwrap().block_count(), 2);
        assert!(store.has_backup(&list.id));
        assert!(!store.scenario_dir(&list.id).join(SCENARIO_FILE).exists());
        assert_eq!(store.to_canvas(&list.id).unwrap_err().code, "alreadyCanvas");

        // An edit on the canvas, then the way back: the edit is gone, the name stays.
        let mut edited = store.load_scenario(&list.id).unwrap();
        edited.name = "Arena 2".to_string();
        edited.graph.as_mut().unwrap().nodes.truncate(1);
        edited.graph.as_mut().unwrap().edges.clear();
        store.save_scenario(&edited).unwrap();
        // The image only the backup list refers to survives a sweep.
        store.sweep_templates();
        assert!(store.template_exists(&list.id, "tpl-a.png"));

        let back = store.to_list(&list.id).unwrap();
        assert_eq!(back.steps, list.steps);
        assert_eq!(back.name, "Arena 2");
        assert!(back.graph.is_none());
        assert!(back.revision > edited.revision);
        assert!(!store.has_backup(&list.id));
        assert!(!store.scenario_dir(&list.id).join(GRAPH_FILE).exists());
        assert_eq!(store.load_scenario(&list.id).unwrap(), back);
        assert_eq!(store.to_list(&list.id).unwrap_err().code, "backupMissing");
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn a_converted_copy_leaves_the_list_alone_and_has_no_backup() {
        let store = temp_store();
        let mut list = Scenario::new("Arena");
        list.steps = vec![wait_for("tpl-a.png")];
        store.save_scenario(&list).unwrap();
        touch(&store, &list.id, "tpl-a.png");

        let copy = store.to_canvas_copy(&list.id, "Arena (canvas)").unwrap();
        assert_ne!(copy.id, list.id);
        assert!(copy.graph.is_some());
        assert!(!store.has_backup(&copy.id));
        assert!(store.template_exists(&copy.id, "tpl-a.png"));
        assert_eq!(store.load_scenario(&list.id).unwrap(), list);

        // A duplicate of a converted scenario does not carry the backup list either.
        store.to_canvas(&list.id).unwrap();
        let twin = store.duplicate_scenario(&list.id, "Twin").unwrap();
        assert!(twin.graph.is_some());
        assert!(!store.has_backup(&twin.id));
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn scenarios_that_run_another_one_are_found() {
        let store = temp_store();
        let callee = Scenario::new("Callee");
        store.save_scenario(&callee).unwrap();
        let mut caller = Scenario::new_canvas("Caller");
        let graph = caller.graph.as_mut().unwrap();
        let mut call = Node::new(NodeKind::Call { scenario_id: callee.id.clone() });
        call.id = "call".to_string();
        graph.edges.push(Edge { from: graph.nodes[0].id.clone(), port: Port::Next, to: call.id.clone(), points: vec![] });
        graph.nodes.push(call);
        store.save_scenario(&caller).unwrap();

        let names: Vec<String> = store.callers(&callee.id).into_iter().map(|item| item.name).collect();
        assert_eq!(names, vec!["Caller".to_string()]);
        assert!(store.callers(&caller.id).is_empty());
        assert!(store.issues(&caller).is_empty());
        store.delete_scenario(&callee.id).unwrap();
        assert_eq!(store.issues(&caller)[0].code, "callMissing");
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn every_cleanup_removes_only_its_own() {
        let store = temp_store();
        let mut list = Scenario::new("Arena");
        list.steps = vec![wait_for("tpl-list.png")];
        store.save_scenario(&list).unwrap();
        touch(&store, &list.id, "tpl-list.png");
        store.to_canvas(&list.id).unwrap();
        // The graph drops the block; only the backup list still needs the image.
        let mut canvas = store.load_scenario(&list.id).unwrap();
        canvas.graph = Some(Graph::new());
        store.save_scenario(&canvas).unwrap();
        let fresh = touch(&store, &list.id, "tpl-fresh.png");

        fs::write(store.journal_path(), b"1234").unwrap();
        fs::write(store.journal_prev_path(), b"12").unwrap();
        fs::create_dir_all(store.chats_dir()).unwrap();
        fs::write(store.chats_dir().join("c1.jsonl"), b"chat").unwrap();
        fs::write(store.root().join("settings.json.bad"), b"bad").unwrap();
        fs::write(store.root().join("settings.json"), b"{}").unwrap();

        assert_eq!(Usage::of(&store.cleanup_files(CleanupKind::Journal)), Usage { bytes: 6, files: 2 });
        assert_eq!(Usage::of(&store.cleanup_files(CleanupKind::Chats)), Usage { bytes: 4, files: 1 });
        assert_eq!(Usage::of(&store.cleanup_files(CleanupKind::Leftovers)), Usage { bytes: 3, files: 1 });
        assert_eq!(store.cleanup_files(CleanupKind::Backups).len(), 1);
        // A fresh image and an image of the backup list are not rubbish.
        assert!(store.cleanup_files(CleanupKind::Templates).is_empty());
        assert_eq!(store.unused_templates(None), vec![fresh.clone()]);
        assert!(store.cleanup_files(CleanupKind::Cache).is_empty());

        assert_eq!(remove_files(&store.cleanup_files(CleanupKind::Journal)), 2);
        assert!(store.chats_dir().join("c1.jsonl").is_file());
        assert!(store.root().join("settings.json.bad").is_file());
        assert!(store.has_backup(&list.id));

        remove_files(&store.cleanup_files(CleanupKind::Leftovers));
        assert!(!store.root().join("settings.json.bad").exists());
        assert!(store.root().join("settings.json").is_file());

        // Without the backup list nothing refers to its image any more.
        remove_files(&store.cleanup_files(CleanupKind::Backups));
        assert!(!store.has_backup(&list.id));
        assert!(store.scenario_exists(&list.id));
        assert_eq!(store.unused_templates(None).len(), 2);
        assert!(fresh.is_file());
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn a_snippet_keeps_its_images_and_brings_them_into_a_scenario() {
        let store = temp_store();
        let from = Scenario::new_canvas("From");
        let to = Scenario::new_canvas("To");
        store.save_scenario(&from).unwrap();
        store.save_scenario(&to).unwrap();
        touch(&store, &from.id, "tpl-a.png");
        let condition = Condition::ImageFound { template: "tpl-a.png".into(), area: Area::Screen, threshold: 0.9 };
        let node = Node::new(NodeKind::Branch { condition, poll_ms: 200, timeout_ms: 0 });

        let saved = store.save_snippet(&from.id, "Check", vec![node], vec![]).unwrap();
        // The snippet does not depend on the scenario it was taken from.
        store.delete_scenario(&from.id).unwrap();
        store.sweep_templates();
        assert_eq!(store.list_snippets(), vec![saved.clone()]);

        let used = store.use_snippet(&saved.id, &to.id).unwrap();
        assert_eq!(used, saved);
        assert!(store.template_exists(&to.id, "tpl-a.png"));
        store.delete_snippet(&saved.id).unwrap();
        assert!(store.list_snippets().is_empty());
        let _ = fs::remove_dir_all(store.root());
    }
}
