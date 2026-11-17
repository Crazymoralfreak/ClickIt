//! Local journal: one JSON object per line. Entries hold codes and parameters,
//! the viewer turns them into text in the current language. The file of the
//! previous session is kept next to the current one.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;

use crate::error::AppError;
use crate::runtime::JournalEntry;

/// A run that pauses and resumes for hours must not fill the disk.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ENTRIES_SHOWN: usize = 2000;

pub struct Journal {
    path: PathBuf,
    prev_path: PathBuf,
    lock: Mutex<()>,
    on_change: Box<dyn Fn() + Send + Sync>,
}

impl Journal {
    /// Starts a new session: the current file becomes the previous one.
    pub fn start(path: PathBuf, prev_path: PathBuf, on_change: Box<dyn Fn() + Send + Sync>) -> Self {
        if path.is_file() {
            let _ = fs::remove_file(&prev_path);
            let _ = fs::rename(&path, &prev_path);
        }
        Self { path, prev_path, lock: Mutex::new(()), on_change }
    }

    pub fn add(&self, code: &str, params: &[(&str, String)]) {
        let entry = JournalEntry {
            time: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            code: code.to_string(),
            params: params.iter().map(|(key, value)| (key.to_string(), value.clone())).collect(),
        };
        self.write(&entry);
        (self.on_change)();
    }

    /// Entry whose `reason` is an error: the error code goes to `reason`, the
    /// error's own parameters are merged in (the entry's parameters win).
    pub fn add_with_reason(&self, code: &str, params: &[(&str, String)], reason: &AppError) {
        let mut all: Vec<(&str, String)> =
            reason.params.iter().map(|(key, value)| (key.as_str(), value.clone())).collect();
        all.push(("reason", reason.code.clone()));
        all.extend(params.iter().cloned());
        self.add(code, &all);
    }

    fn write(&self, entry: &JournalEntry) {
        let _guard = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let too_big = fs::metadata(&self.path).is_ok_and(|meta| meta.len() > MAX_FILE_BYTES);
        let mut options = OpenOptions::new();
        options.create(true);
        if too_big {
            options.write(true).truncate(true);
        } else {
            options.append(true);
        }
        let Ok(line) = serde_json::to_string(entry) else {
            return;
        };
        if let Ok(mut file) = options.open(&self.path) {
            let _ = writeln!(file, "{line}");
        }
    }

    /// Previous session followed by the current one, oldest first.
    pub fn read(&self) -> Vec<JournalEntry> {
        let _guard = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut entries: Vec<JournalEntry> = [&self.prev_path, &self.path]
            .iter()
            .filter_map(|path| fs::read_to_string(path).ok())
            .flat_map(|text| {
                text.lines().filter_map(|line| serde_json::from_str::<JournalEntry>(line).ok()).collect::<Vec<_>>()
            })
            .collect();
        if entries.len() > MAX_ENTRIES_SHOWN {
            entries.drain(..entries.len() - MAX_ENTRIES_SHOWN);
        }
        entries
    }
}
