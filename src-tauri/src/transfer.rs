//! Export and import of one scenario as a `.clickit` file: a zip archive with
//! `scenario.json` and the template images of the scenario. A graph is stored
//! in the same `scenario.json` with version 2, and the scenarios it runs come
//! along under `calls/<id>/`.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use image::ImageFormat;
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{AppError, AppResult};
use crate::model::graph::GRAPH_VERSION;
use crate::model::scenario::{new_id, CommandSpec, Scenario, MAX_STEPS};
use crate::storage::{valid_id, valid_template_name, Store};

pub const EXTENSION: &str = "clickit";
const SCENARIO_ENTRY: &str = "scenario.json";
const CALLS_DIR: &str = "calls";
/// A file from somewhere else must not be able to fill the memory.
const MAX_SCENARIO_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TEMPLATE_BYTES: u64 = 32 * 1024 * 1024;
/// Scenarios one file may carry besides the main one.
const MAX_CALLEES: usize = 64;

fn message(error: impl ToString) -> String {
    error.to_string()
}

/// Writes the scenario, the scenarios it runs and the template images they refer to into `path`.
pub fn export(store: &Store, id: &str, path: &Path) -> AppResult<()> {
    let scenario = store.load_scenario(id)?;
    let written = write_archive(store, &scenario, path);
    if written.is_err() {
        let _ = fs::remove_file(path);
    }
    written.map_err(|text| AppError::new("exportFailed").with("message", text))
}

/// Scenarios the given one runs, directly or through others. The ones that
/// are gone are left out: the import shows the same broken block.
fn callees_of(store: &Store, scenario: &Scenario) -> Vec<Scenario> {
    let mut found: Vec<Scenario> = Vec::new();
    let mut pending: Vec<String> = scenario.graph.as_ref().map(|graph| graph.callees()).unwrap_or_default();
    while let Some(id) = pending.pop() {
        if id == scenario.id || found.iter().any(|known| known.id == id) || found.len() >= MAX_CALLEES {
            continue;
        }
        let Ok(callee) = store.load_scenario(&id) else {
            continue;
        };
        pending.extend(callee.graph.as_ref().map(|graph| graph.callees()).unwrap_or_default());
        found.push(callee);
    }
    found
}

fn write_archive(store: &Store, scenario: &Scenario, path: &Path) -> Result<(), String> {
    let mut archive = ZipWriter::new(File::create(path).map_err(message)?);
    write_scenario(store, &mut archive, scenario, "")?;
    for callee in callees_of(store, scenario) {
        write_scenario(store, &mut archive, &callee, &format!("{CALLS_DIR}/{}/", callee.id))?;
    }
    archive.finish().map_err(message)?;
    Ok(())
}

fn write_scenario(store: &Store, archive: &mut ZipWriter<File>, scenario: &Scenario, prefix: &str) -> Result<(), String> {
    let mut stored = scenario.clone();
    stored.version = stored.format_version();
    let json = serde_json::to_string_pretty(&stored).map_err(message)?;
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    archive.start_file(format!("{prefix}{SCENARIO_ENTRY}"), options).map_err(message)?;
    archive.write_all(json.as_bytes()).map_err(message)?;
    for name in scenario.templates() {
        // A template that is already missing stays missing after the import.
        let Ok(bytes) = store.template_path(&scenario.id, &name).and_then(|source| Ok(fs::read(source)?)) else {
            continue;
        };
        archive.start_file(format!("{prefix}{name}"), options).map_err(message)?;
        archive.write_all(&bytes).map_err(message)?;
    }
    Ok(())
}

/// A scenario read from a file, with its template images as `(file name, PNG bytes)`.
struct Imported {
    scenario: Scenario,
    templates: Vec<(String, Vec<u8>)>,
}

/// Adds the scenario from `path` as a new one, together with the scenarios it
/// runs. A broken file changes nothing on the disk.
///
/// A file with "Command" steps is added only when `approve` says so: it gets
/// every command of the file, the switched off ones too, before anything is
/// written. `None` when it said no.
pub fn import(
    store: &Store,
    path: &Path,
    approve: impl FnOnce(&[&CommandSpec]) -> bool,
) -> AppResult<Option<Scenario>> {
    let imported = read_archive(path).map_err(|text| AppError::new("importBad").with("message", text))?;
    let commands: Vec<&CommandSpec> =
        imported.iter().flat_map(|Imported { scenario, .. }| scenario.commands(false)).collect();
    if !commands.is_empty() && !approve(&commands) {
        return Ok(None);
    }
    let stored = imported.iter().try_for_each(|Imported { scenario, templates }| {
        store.save_scenario(scenario)?;
        for (name, bytes) in templates {
            fs::write(store.template_path(&scenario.id, name)?, bytes)?;
        }
        Ok(())
    });
    if let Err(error) = stored {
        for Imported { scenario, .. } in &imported {
            let _ = store.delete_scenario(&scenario.id);
        }
        return Err(error);
    }
    // The main scenario is the first one.
    imported.into_iter().next().map(|main| Some(main.scenario)).ok_or_else(|| AppError::new("importBad"))
}

fn read_entry(archive: &mut ZipArchive<File>, name: &str, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(message(error)),
    };
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes).map_err(message)?;
    if bytes.len() as u64 > limit {
        return Err(format!("{name} is too large"));
    }
    Ok(Some(bytes))
}

/// One scenario of the archive with its images, as it was written. `None` when it is not there.
fn read_scenario(archive: &mut ZipArchive<File>, prefix: &str) -> Result<Option<Imported>, String> {
    let entry = format!("{prefix}{SCENARIO_ENTRY}");
    let Some(json) = read_entry(archive, &entry, MAX_SCENARIO_BYTES)? else {
        return Ok(None);
    };
    let json = String::from_utf8(json).map_err(message)?;
    let scenario: Scenario = serde_json::from_str(json.trim_start_matches('\u{feff}')).map_err(message)?;
    if scenario.version > GRAPH_VERSION {
        return Err(format!("made by a newer ClickIt (format {})", scenario.version));
    }
    let size = scenario.graph.as_ref().map_or(scenario.step_count(), |graph| graph.nodes.len());
    if size > MAX_STEPS {
        return Err(format!("more than {MAX_STEPS} steps"));
    }
    let mut templates = Vec::new();
    for name in scenario.templates() {
        if !valid_template_name(&name) {
            return Err(format!("bad template name {name}"));
        }
        if let Some(bytes) = read_entry(archive, &format!("{prefix}{name}"), MAX_TEMPLATE_BYTES)? {
            image::load_from_memory_with_format(&bytes, ImageFormat::Png)
                .map_err(|error| format!("{name}: {error}"))?;
            templates.push((name, bytes));
        }
    }
    Ok(Some(Imported { scenario, templates }))
}

/// Reads and checks everything before anything is written. The main scenario
/// comes first, the ones it runs follow.
fn read_archive(path: &Path) -> Result<Vec<Imported>, String> {
    let mut archive = ZipArchive::new(File::open(path).map_err(message)?).map_err(message)?;
    let mut main =
        read_scenario(&mut archive, "")?.ok_or_else(|| format!("{SCENARIO_ENTRY} is missing"))?;
    if main.scenario.name.trim().is_empty() {
        main.scenario.name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("ClickIt").to_string();
    }
    let mut all = vec![main];
    let mut at = 0;
    while at < all.len() {
        let wanted = all[at].scenario.graph.as_ref().map(|graph| graph.callees()).unwrap_or_default();
        for id in wanted {
            if !valid_id(&id) || all.iter().any(|known| known.scenario.id == id) {
                continue;
            }
            if all.len() > MAX_CALLEES {
                return Err(format!("more than {MAX_CALLEES} scenarios inside"));
            }
            // A scenario that was not put into the file stays a broken block.
            if let Some(mut callee) = read_scenario(&mut archive, &format!("{CALLS_DIR}/{id}/"))? {
                callee.scenario.id = id;
                all.push(callee);
            }
        }
        at += 1;
    }
    // The copies are scenarios of their own: nothing in them points at the originals.
    let renamed: HashMap<String, String> =
        all.iter().map(|imported| (imported.scenario.id.clone(), new_id("sc"))).collect();
    for Imported { scenario, .. } in &mut all {
        if let Some(id) = renamed.get(&scenario.id) {
            scenario.id = id.clone();
        }
        scenario.version = scenario.format_version();
        scenario.revision = 0;
        scenario.renew_step_ids();
        if let Some(graph) = &mut scenario.graph {
            graph.remap_calls(&renamed);
        }
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::scenario::{Area, Condition, OnFound, OnTimeout, Step, StepKind};

    fn temp_store() -> Store {
        Store::at(std::env::temp_dir().join(format!("clickit-test-{}", new_id("t")))).unwrap()
    }

    #[test]
    fn exported_scenario_comes_back_with_its_template() {
        let store = temp_store();
        let mut scenario = Scenario::new("Demo");
        scenario.steps = vec![Step::new(StepKind::Wait {
            condition: Condition::ImageFound { template: "tpl-1.png".to_string(), area: Area::Screen, threshold: 0.9 },
            poll_ms: 200,
            timeout_ms: 0,
            on_timeout: OnTimeout::Stop,
            on_found: OnFound::None,
        })];
        store.save_scenario(&scenario).unwrap();
        let template = store.template_path(&scenario.id, "tpl-1.png").unwrap();
        image::RgbaImage::new(3, 2).save(&template).unwrap();

        let file = store.root().join("demo.clickit");
        export(&store, &scenario.id, &file).unwrap();
        // A file without commands is added without a question.
        let copy = import(&store, &file, |_| panic!("nothing to ask about")).unwrap().unwrap();

        assert_ne!(copy.id, scenario.id);
        assert_ne!(copy.steps[0].id, scenario.steps[0].id);
        assert_eq!(copy.name, "Demo");
        assert_eq!(copy.steps[0].kind, scenario.steps[0].kind);
        assert!(store.template_exists(&copy.id, "tpl-1.png"));
        assert_eq!(store.list_scenarios().len(), 2);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn broken_file_is_rejected_and_the_list_stays_as_it_was() {
        let store = temp_store();
        store.save_scenario(&Scenario::new("Kept")).unwrap();
        let file = store.root().join("broken.clickit");
        fs::write(&file, b"not a zip archive").unwrap();

        let error = import(&store, &file, |_| true).unwrap_err();
        assert_eq!(error.code, "importBad");
        assert_eq!(store.list_scenarios().len(), 1);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn a_file_with_commands_is_added_only_after_a_yes() {
        use crate::model::graph::{Edge, Node, NodeKind, Port};
        use crate::model::scenario::{CommandRun, OnFailure, ShellKind};

        let command = |text: &str| CommandSpec {
            run: CommandRun::Script { shell: ShellKind::Cmd, text: text.to_string() },
            work_dir: String::new(),
            wait: true,
            timeout_ms: 0,
            hidden: true,
        };
        let store = temp_store();
        // The command of a scenario that is only run from the main one counts
        // too, and so does a step that is switched off.
        let mut callee = Scenario::new("Callee");
        let mut off = Step::new(StepKind::Command { command: command("echo inner"), on_failure: OnFailure::Stop });
        off.enabled = false;
        callee.steps = vec![off];
        store.save_scenario(&callee).unwrap();
        let mut caller = Scenario::new_canvas("Caller");
        let graph = caller.graph.as_mut().unwrap();
        let run = Node::new(NodeKind::Command { command: command("echo outer") });
        let call = Node::new(NodeKind::Call { scenario_id: callee.id.clone() });
        graph.edges.push(Edge { from: graph.nodes[0].id.clone(), port: Port::Next, to: run.id.clone(), points: vec![] });
        graph.edges.push(Edge { from: run.id.clone(), port: Port::Next, to: call.id.clone(), points: vec![] });
        graph.nodes.extend([run, call]);
        store.save_scenario(&caller).unwrap();
        let file = store.root().join("commands.clickit");
        export(&store, &caller.id, &file).unwrap();

        let mut asked = Vec::new();
        let declined = import(&store, &file, |commands| {
            asked = commands.iter().map(|command| command.describe()).collect();
            false
        })
        .unwrap();
        assert!(declined.is_none());
        assert_eq!(asked, ["[cmd]\necho outer", "[cmd]\necho inner"]);
        assert_eq!(store.list_scenarios().len(), 2, "a no adds nothing");

        let copy = import(&store, &file, |_| true).unwrap().unwrap();
        assert_eq!(copy.name, "Caller");
        assert_eq!(store.list_scenarios().len(), 4);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn a_graph_travels_as_version_two_with_the_scenarios_it_runs() {
        use crate::model::graph::{Edge, Node, NodeKind, Port};

        let store = temp_store();
        let mut callee = Scenario::new("Callee");
        callee.steps = vec![Step::new(StepKind::Wait {
            condition: Condition::ImageFound { template: "tpl-c.png".to_string(), area: Area::Screen, threshold: 0.9 },
            poll_ms: 200,
            timeout_ms: 0,
            on_timeout: OnTimeout::Stop,
            on_found: OnFound::None,
        })];
        store.save_scenario(&callee).unwrap();
        image::RgbaImage::new(3, 2).save(store.template_path(&callee.id, "tpl-c.png").unwrap()).unwrap();

        let mut caller = Scenario::new_canvas("Caller");
        let graph = caller.graph.as_mut().unwrap();
        let call = Node::new(NodeKind::Call { scenario_id: callee.id.clone() });
        graph.edges.push(Edge { from: graph.nodes[0].id.clone(), port: Port::Next, to: call.id.clone(), points: vec![] });
        graph.nodes.push(call);
        store.save_scenario(&caller).unwrap();
        // The list the graph could have come from is not a part of the file.
        fs::write(store.scenario_dir(&caller.id).join("steps.backup.json"), b"{}").unwrap();

        let file = store.root().join("caller.clickit");
        export(&store, &caller.id, &file).unwrap();
        let mut archive = ZipArchive::new(File::open(&file).unwrap()).unwrap();
        let names: Vec<String> = archive.file_names().map(str::to_string).collect();
        assert!(!names.iter().any(|name| name.contains("backup")));
        let json = String::from_utf8(read_entry(&mut archive, SCENARIO_ENTRY, MAX_SCENARIO_BYTES).unwrap().unwrap());
        let written: serde_json::Value = serde_json::from_str(&json.unwrap()).unwrap();
        assert_eq!(written["version"], 2);
        drop(archive);

        let copy = import(&store, &file, |_| true).unwrap().unwrap();
        assert_ne!(copy.id, caller.id);
        assert_eq!(copy.version, GRAPH_VERSION);
        let called = copy.graph.as_ref().unwrap().callees();
        assert_eq!(called.len(), 1);
        // The block points at the imported copy of what it runs, not at the original.
        assert_ne!(called[0], callee.id);
        assert_eq!(store.load_scenario(&called[0]).unwrap().name, "Callee");
        assert!(store.template_exists(&called[0], "tpl-c.png"));
        assert!(store.issues(&copy).is_empty());
        assert_eq!(store.list_scenarios().len(), 4);
        let _ = fs::remove_dir_all(store.root());
    }
}
