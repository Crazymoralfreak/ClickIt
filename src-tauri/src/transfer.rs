//! Export and import of one scenario as a `.clickit` file: a zip archive with
//! `scenario.json` and the template images of the scenario.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use image::ImageFormat;
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{AppError, AppResult};
use crate::model::scenario::{new_id, Scenario, MAX_STEPS, SCENARIO_VERSION};
use crate::storage::{valid_template_name, Store};

pub const EXTENSION: &str = "clickit";
const SCENARIO_ENTRY: &str = "scenario.json";
/// A file from somewhere else must not be able to fill the memory.
const MAX_SCENARIO_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TEMPLATE_BYTES: u64 = 32 * 1024 * 1024;

fn message(error: impl ToString) -> String {
    error.to_string()
}

/// Writes the scenario and the template images it refers to into `path`.
pub fn export(store: &Store, id: &str, path: &Path) -> AppResult<()> {
    let scenario = store.load_scenario(id)?;
    let written = write_archive(store, &scenario, path);
    if written.is_err() {
        let _ = fs::remove_file(path);
    }
    written.map_err(|text| AppError::new("exportFailed").with("message", text))
}

fn write_archive(store: &Store, scenario: &Scenario, path: &Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(scenario).map_err(message)?;
    let mut archive = ZipWriter::new(File::create(path).map_err(message)?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    archive.start_file(SCENARIO_ENTRY, options).map_err(message)?;
    archive.write_all(json.as_bytes()).map_err(message)?;
    for name in scenario.templates() {
        // A template that is already missing stays missing after the import.
        let Ok(bytes) = store.template_path(&scenario.id, &name).and_then(|source| Ok(fs::read(source)?)) else {
            continue;
        };
        archive.start_file(name, options).map_err(message)?;
        archive.write_all(&bytes).map_err(message)?;
    }
    archive.finish().map_err(message)?;
    Ok(())
}

/// A scenario read from a file, with its template images as `(file name, PNG bytes)`.
struct Imported {
    scenario: Scenario,
    templates: Vec<(String, Vec<u8>)>,
}

/// Adds the scenario from `path` as a new one. A broken file changes nothing on the disk.
pub fn import(store: &Store, path: &Path) -> AppResult<Scenario> {
    let Imported { scenario, templates } =
        read_archive(path).map_err(|text| AppError::new("importBad").with("message", text))?;
    let stored = store.save_scenario(&scenario).and_then(|()| {
        for (name, bytes) in &templates {
            fs::write(store.template_path(&scenario.id, name)?, bytes)?;
        }
        Ok(())
    });
    if let Err(error) = stored {
        let _ = store.delete_scenario(&scenario.id);
        return Err(error);
    }
    Ok(scenario)
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

/// Reads and checks everything before anything is written.
fn read_archive(path: &Path) -> Result<Imported, String> {
    let mut archive = ZipArchive::new(File::open(path).map_err(message)?).map_err(message)?;
    let json = read_entry(&mut archive, SCENARIO_ENTRY, MAX_SCENARIO_BYTES)?
        .ok_or_else(|| format!("{SCENARIO_ENTRY} is missing"))?;
    let json = String::from_utf8(json).map_err(message)?;
    let mut scenario: Scenario = serde_json::from_str(json.trim_start_matches('\u{feff}')).map_err(message)?;
    if scenario.version > SCENARIO_VERSION {
        return Err(format!("made by a newer ClickIt (format {})", scenario.version));
    }
    if scenario.step_count() > MAX_STEPS {
        return Err(format!("more than {MAX_STEPS} steps"));
    }
    if scenario.name.trim().is_empty() {
        scenario.name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("ClickIt").to_string();
    }
    // The copy is a scenario of its own: nothing in it points at the original.
    scenario.id = new_id("sc");
    scenario.version = SCENARIO_VERSION;
    scenario.renew_step_ids();

    let mut templates = Vec::new();
    for name in scenario.templates() {
        if !valid_template_name(&name) {
            return Err(format!("bad template name {name}"));
        }
        if let Some(bytes) = read_entry(&mut archive, &name, MAX_TEMPLATE_BYTES)? {
            image::load_from_memory_with_format(&bytes, ImageFormat::Png)
                .map_err(|error| format!("{name}: {error}"))?;
            templates.push((name, bytes));
        }
    }
    Ok(Imported { scenario, templates })
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
        let copy = import(&store, &file).unwrap();

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

        let error = import(&store, &file).unwrap_err();
        assert_eq!(error.code, "importBad");
        assert_eq!(store.list_scenarios().len(), 1);
        let _ = fs::remove_dir_all(store.root());
    }
}
