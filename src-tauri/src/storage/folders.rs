//! Folders of the list of scenarios. They are a view only: queues, the
//! schedule and "Run scenario" blocks know a scenario by its id, wherever it lies.
//!
//! `folders.json` holds the folders and which scenario lies in which one.
//! Folders with the same parent are shown in the order of the file; scenarios
//! inside a folder go by name.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{read_json, valid_id, write_json, Store};
use crate::error::{AppError, AppResult};
use crate::model::scenario::new_id;

const FOLDERS_FILE: &str = "folders.json";
/// How many folders may lie inside one another, the outermost one included.
pub const MAX_FOLDER_DEPTH: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub name: String,
    /// Absent for a folder at the top of the list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folders {
    pub folders: Vec<Folder>,
    /// Scenario id to folder id. A scenario without an entry lies at the top.
    pub placement: BTreeMap<String, String>,
}

fn missing() -> AppError {
    AppError::new("folderMissing")
}

impl Folders {
    fn get(&self, id: &str) -> Option<&Folder> {
        self.folders.iter().find(|folder| folder.id == id)
    }

    fn position(&self, id: &str) -> AppResult<usize> {
        self.folders.iter().position(|folder| folder.id == id).ok_or_else(missing)
    }

    /// 1 for a folder at the top; 0 for one that is not there.
    fn depth(&self, id: &str) -> usize {
        let mut depth = 0;
        let mut at = self.get(id);
        while let Some(folder) = at {
            depth += 1;
            // A circle in a file edited by hand must not hang the walk.
            if depth > self.folders.len() {
                break;
            }
            at = folder.parent_id.as_deref().and_then(|parent| self.get(parent));
        }
        depth
    }

    /// Levels of folders under this one, itself included.
    fn height(&self, id: &str) -> usize {
        let below = self.folders.iter().filter(|folder| folder.parent_id.as_deref() == Some(id));
        1 + below.map(|folder| self.height(&folder.id)).max().unwrap_or(0)
    }

    /// Whether `id` is `ancestor` itself or lies somewhere inside it.
    fn inside(&self, id: &str, ancestor: &str) -> bool {
        let mut at = self.get(id);
        let mut steps = 0;
        while let Some(folder) = at {
            if folder.id == ancestor {
                return true;
            }
            steps += 1;
            if steps > self.folders.len() {
                break;
            }
            at = folder.parent_id.as_deref().and_then(|parent| self.get(parent));
        }
        false
    }

    /// Names from the top of the list down to the folder.
    pub fn path(&self, id: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut at = self.get(id);
        while let Some(folder) = at {
            names.push(folder.name.clone());
            if names.len() > self.folders.len() {
                break;
            }
            at = folder.parent_id.as_deref().and_then(|parent| self.get(parent));
        }
        names.reverse();
        names
    }

    /// Brings a file that was edited by hand, or that outlived its scenarios,
    /// to a tree: no repeated ids, no circles, no folder deeper than the
    /// limit, and nothing that points at what is gone.
    fn clean(&mut self, scenario_exists: &dyn Fn(&str) -> bool) {
        let mut seen = HashSet::new();
        self.folders.retain(|folder| valid_id(&folder.id) && seen.insert(folder.id.clone()));
        for at in 0..self.folders.len() {
            let name = self.folders[at].name.trim().to_string();
            self.folders[at].name = if name.is_empty() { self.folders[at].id.clone() } else { name };
            let orphan = match self.folders[at].parent_id.as_deref() {
                Some(parent) => parent == self.folders[at].id || !seen.contains(parent),
                None => false,
            };
            if orphan {
                self.folders[at].parent_id = None;
            }
        }
        // A folder that is too deep, or that is a part of a circle, goes to the top.
        for at in 0..self.folders.len() {
            let mut depth = 1;
            let mut parent = self.folders[at].parent_id.clone();
            while let Some(id) = parent {
                depth += 1;
                if depth > MAX_FOLDER_DEPTH {
                    self.folders[at].parent_id = None;
                    break;
                }
                parent = self.get(&id).and_then(|folder| folder.parent_id.clone());
            }
        }
        self.placement.retain(|scenario, folder| seen.contains(folder.as_str()) && scenario_exists(scenario));
    }

    /// A new folder goes after the ones that have the same parent.
    pub fn create(&mut self, name: &str, parent_id: Option<&str>) -> AppResult<Folder> {
        if let Some(parent) = parent_id {
            self.position(parent)?;
            if self.depth(parent) >= MAX_FOLDER_DEPTH {
                return Err(AppError::new("folderTooDeep"));
            }
        }
        let folder = Folder {
            id: new_id("f"),
            name: name.to_string(),
            parent_id: parent_id.map(str::to_string),
            collapsed: false,
        };
        self.folders.push(folder.clone());
        Ok(folder)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> AppResult<()> {
        let at = self.position(id)?;
        self.folders[at].name = name.to_string();
        Ok(())
    }

    pub fn set_collapsed(&mut self, id: &str, collapsed: bool) -> AppResult<()> {
        let at = self.position(id)?;
        self.folders[at].collapsed = collapsed;
        Ok(())
    }

    /// Removes the folder only: what was inside goes one level up and takes its place.
    pub fn delete(&mut self, id: &str) -> AppResult<()> {
        let at = self.position(id)?;
        let removed = self.folders.remove(at);
        let own = |folder: &Folder| folder.parent_id.as_deref() == Some(id);
        let to = self.folders[..at].iter().filter(|folder| !own(folder)).count();
        let (mut inner, mut rest): (Vec<Folder>, Vec<Folder>) = std::mem::take(&mut self.folders).into_iter().partition(own);
        for folder in &mut inner {
            folder.parent_id = removed.parent_id.clone();
        }
        rest.splice(to..to, inner);
        self.folders = rest;
        match &removed.parent_id {
            Some(parent) => {
                for folder in self.placement.values_mut().filter(|folder| folder.as_str() == id) {
                    *folder = parent.clone();
                }
            }
            None => self.placement.retain(|_, folder| folder != id),
        }
        Ok(())
    }

    /// Puts the folder into `parent_id` (the top when `None`), in front of
    /// `before_id` or after the last folder there.
    pub fn move_folder(&mut self, id: &str, parent_id: Option<&str>, before_id: Option<&str>) -> AppResult<()> {
        let at = self.position(id)?;
        if let Some(parent) = parent_id {
            self.position(parent)?;
            if self.inside(parent, id) {
                return Err(AppError::new("folderIntoItself"));
            }
            if self.depth(parent) + self.height(id) > MAX_FOLDER_DEPTH {
                return Err(AppError::new("folderTooDeep"));
            }
        }
        if let Some(before) = before_id {
            // The folder to stand in front of has to be one of the new neighbours.
            let neighbour = self.get(before).ok_or_else(missing)?;
            if neighbour.parent_id.as_deref() != parent_id {
                return Err(missing());
            }
            if before == id {
                return Ok(());
            }
        }
        let mut folder = self.folders.remove(at);
        folder.parent_id = parent_id.map(str::to_string);
        let to = before_id
            .and_then(|before| self.folders.iter().position(|other| other.id == before))
            .unwrap_or(self.folders.len());
        self.folders.insert(to, folder);
        Ok(())
    }

    /// Puts a scenario into a folder, or at the top of the list.
    pub fn place(&mut self, scenario_id: &str, folder_id: Option<&str>) -> AppResult<()> {
        match folder_id {
            Some(folder) => {
                self.position(folder)?;
                self.placement.insert(scenario_id.to_string(), folder.to_string());
            }
            None => {
                self.placement.remove(scenario_id);
            }
        }
        Ok(())
    }
}

impl Store {
    fn folders_path(&self) -> PathBuf {
        self.root.join(FOLDERS_FILE)
    }

    fn read_folders(&self) -> Folders {
        read_json(&self.folders_path()).unwrap_or_default()
    }

    /// Folders as a tree that can be trusted. A missing or damaged file gives none.
    pub fn load_folders(&self) -> Folders {
        let mut folders = self.read_folders();
        folders.clean(&|id| self.scenario_exists(id));
        folders
    }

    /// Reads, changes and writes under one lock, so two changes do not lose each other.
    fn change_folders<T>(&self, change: impl FnOnce(&mut Folders) -> AppResult<T>) -> AppResult<T> {
        let _guard = self.folders_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut folders = self.load_folders();
        let out = change(&mut folders)?;
        write_json(&self.folders_path(), &folders)?;
        Ok(out)
    }

    pub fn create_folder(&self, name: &str, parent_id: Option<&str>) -> AppResult<Folder> {
        self.change_folders(|folders| folders.create(name, parent_id))
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> AppResult<()> {
        self.change_folders(|folders| folders.rename(id, name))
    }

    pub fn delete_folder(&self, id: &str) -> AppResult<()> {
        self.change_folders(|folders| folders.delete(id))
    }

    pub fn set_folder_collapsed(&self, id: &str, collapsed: bool) -> AppResult<()> {
        self.change_folders(|folders| folders.set_collapsed(id, collapsed))
    }

    /// Returns the folders in their new order.
    pub fn move_folder(&self, id: &str, parent_id: Option<&str>, before_id: Option<&str>) -> AppResult<Vec<Folder>> {
        self.change_folders(|folders| {
            folders.move_folder(id, parent_id, before_id)?;
            Ok(folders.folders.clone())
        })
    }

    pub fn move_scenario(&self, id: &str, folder_id: Option<&str>) -> AppResult<()> {
        if !self.scenario_exists(id) {
            return Err(AppError::new("notFound"));
        }
        self.change_folders(|folders| folders.place(id, folder_id))
    }

    /// A scenario made from another one lies next to it. The scenario is
    /// already saved; a folder file that could not be written leaves it at the top.
    pub(super) fn place_beside(&self, id: &str, source_id: &str) {
        let Some(folder) = self.read_folders().placement.get(source_id).cloned() else {
            return;
        };
        let _ = self.change_folders(|folders| folders.place(id, Some(&folder)));
    }

    /// Drops the entry of a deleted scenario.
    pub(super) fn forget_placement(&self, id: &str) {
        if self.read_folders().placement.contains_key(id) {
            // Reading the file again leaves out what is no longer on the disk.
            let _ = self.change_folders(|_| Ok(()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::scenario::Scenario;

    fn temp_store() -> Store {
        Store::at(std::env::temp_dir().join(format!("clickit-folders-{}", new_id("t")))).unwrap()
    }

    fn order(folders: &Folders, parent: Option<&str>) -> Vec<String> {
        folders
            .folders
            .iter()
            .filter(|folder| folder.parent_id.as_deref() == parent)
            .map(|folder| folder.name.clone())
            .collect()
    }

    #[test]
    fn folders_keep_the_order_they_were_given() {
        let mut folders = Folders::default();
        let a = folders.create("A", None).unwrap();
        let b = folders.create("B", None).unwrap();
        let c = folders.create("C", None).unwrap();
        assert_eq!(order(&folders, None), ["A", "B", "C"]);

        folders.move_folder(&c.id, None, Some(&a.id)).unwrap();
        assert_eq!(order(&folders, None), ["C", "A", "B"]);
        // No folder to stand in front of: after the last one.
        folders.move_folder(&c.id, None, None).unwrap();
        assert_eq!(order(&folders, None), ["A", "B", "C"]);
        // In front of itself changes nothing.
        folders.move_folder(&b.id, None, Some(&b.id)).unwrap();
        assert_eq!(order(&folders, None), ["A", "B", "C"]);

        // Into another folder, and in front of what is already there.
        folders.move_folder(&b.id, Some(&a.id), None).unwrap();
        folders.move_folder(&c.id, Some(&a.id), Some(&b.id)).unwrap();
        assert_eq!(order(&folders, None), ["A"]);
        assert_eq!(order(&folders, Some(&a.id)), ["C", "B"]);
        assert_eq!(folders.path(&b.id), ["A", "B"]);
        // A folder of another parent cannot be the neighbour.
        assert_eq!(folders.move_folder(&b.id, None, Some(&c.id)).unwrap_err().code, "folderMissing");
    }

    #[test]
    fn a_folder_does_not_go_into_itself_or_deeper_than_the_limit() {
        let mut folders = Folders::default();
        let mut chain = vec![folders.create("1", None).unwrap()];
        for level in 2..=MAX_FOLDER_DEPTH {
            let parent = chain.last().unwrap().id.clone();
            chain.push(folders.create(&level.to_string(), Some(&parent)).unwrap());
        }
        let deepest = chain.last().unwrap().id.clone();
        assert_eq!(folders.create("too deep", Some(&deepest)).unwrap_err().code, "folderTooDeep");

        let top = chain[0].id.clone();
        assert_eq!(folders.move_folder(&top, Some(&top), None).unwrap_err().code, "folderIntoItself");
        assert_eq!(folders.move_folder(&top, Some(&deepest), None).unwrap_err().code, "folderIntoItself");

        // A folder with one level inside fits under the fourth level, not under the fifth.
        let pair = folders.create("pair", None).unwrap();
        folders.create("inner", Some(&pair.id)).unwrap();
        assert_eq!(folders.move_folder(&pair.id, Some(&chain[3].id), None).unwrap_err().code, "folderTooDeep");
        folders.move_folder(&pair.id, Some(&chain[2].id), None).unwrap();
        assert_eq!(folders.depth(&pair.id), 4);
        assert_eq!(folders.move_folder("gone", None, None).unwrap_err().code, "folderMissing");
    }

    #[test]
    fn a_deleted_folder_leaves_what_was_inside_one_level_up() {
        let mut folders = Folders::default();
        let first = folders.create("First", None).unwrap();
        let middle = folders.create("Middle", None).unwrap();
        let last = folders.create("Last", None).unwrap();
        let inner_a = folders.create("Inner A", Some(&middle.id)).unwrap();
        let inner_b = folders.create("Inner B", Some(&middle.id)).unwrap();
        let deep = folders.create("Deep", Some(&inner_a.id)).unwrap();
        folders.place("in-middle", Some(&middle.id)).unwrap();
        folders.place("in-inner", Some(&inner_a.id)).unwrap();
        folders.place("in-deep", Some(&deep.id)).unwrap();

        // From the middle of a nested folder: its content goes to that folder.
        folders.delete(&inner_a.id).unwrap();
        assert_eq!(order(&folders, Some(&middle.id)), ["Deep", "Inner B"]);
        assert_eq!(folders.placement.get("in-inner"), Some(&middle.id));
        assert_eq!(folders.placement.get("in-deep"), Some(&deep.id));

        // From the top: the content takes the place of the folder, scenarios lie at the top.
        folders.delete(&middle.id).unwrap();
        assert_eq!(order(&folders, None), ["First", "Deep", "Inner B", "Last"]);
        assert!(!folders.placement.contains_key("in-middle"));
        assert!(!folders.placement.contains_key("in-inner"));
        assert_eq!(folders.placement.get("in-deep"), Some(&deep.id));
        assert!(folders.get(&first.id).is_some() && folders.get(&last.id).is_some() && folders.get(&inner_b.id).is_some());
        assert_eq!(folders.delete(&middle.id).unwrap_err().code, "folderMissing");
    }

    #[test]
    fn a_file_edited_by_hand_is_read_as_a_tree() {
        let folder = |id: &str, parent: Option<&str>| Folder {
            id: id.to_string(),
            name: format!(" {id} "),
            parent_id: parent.map(str::to_string),
            collapsed: false,
        };
        let mut folders = Folders {
            folders: vec![
                folder("a", Some("b")),
                folder("b", Some("a")),
                folder("a", None),
                folder("lost", Some("nowhere")),
                folder("self", Some("self")),
                folder("../x", None),
                folder("d1", None),
                folder("d2", Some("d1")),
                folder("d3", Some("d2")),
                folder("d4", Some("d3")),
                folder("d5", Some("d4")),
                folder("d6", Some("d5")),
                folder("d7", Some("d6")),
            ],
            placement: BTreeMap::from([
                ("alive".to_string(), "d2".to_string()),
                ("dead".to_string(), "d2".to_string()),
                ("stray".to_string(), "gone".to_string()),
            ]),
        };
        folders.clean(&|id| id != "dead");

        let ids: Vec<&str> = folders.folders.iter().map(|folder| folder.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "lost", "self", "d1", "d2", "d3", "d4", "d5", "d6", "d7"]);
        assert_eq!(folders.get("a").unwrap().name, "a");
        // The circle is cut at its first folder.
        assert_eq!(folders.get("a").unwrap().parent_id, None);
        assert_eq!(folders.get("b").unwrap().parent_id.as_deref(), Some("a"));
        assert_eq!(folders.get("lost").unwrap().parent_id, None);
        assert_eq!(folders.get("self").unwrap().parent_id, None);
        // The sixth level starts a tree of its own.
        assert_eq!(folders.depth("d5"), MAX_FOLDER_DEPTH);
        assert_eq!(folders.get("d6").unwrap().parent_id, None);
        assert_eq!(folders.depth("d7"), 2);
        assert_eq!(folders.placement, BTreeMap::from([("alive".to_string(), "d2".to_string())]));
    }

    #[test]
    fn a_scenario_stays_in_its_folder_through_a_copy_and_leaves_it_when_deleted() {
        let store = temp_store();
        let scenario = Scenario::new("Arena");
        store.save_scenario(&scenario).unwrap();
        assert!(store.list_scenarios()[0].folder_id.is_none());

        let outer = store.create_folder("Games", None).unwrap();
        let inner = store.create_folder("Morning", Some(&outer.id)).unwrap();
        store.move_scenario(&scenario.id, Some(&inner.id)).unwrap();
        assert_eq!(store.move_scenario("gone", Some(&inner.id)).unwrap_err().code, "notFound");
        assert_eq!(store.move_scenario(&scenario.id, Some("gone")).unwrap_err().code, "folderMissing");

        let listed = store.list_scenarios();
        assert_eq!(listed[0].folder_id.as_deref(), Some(inner.id.as_str()));
        assert_eq!(listed[0].folder_path, ["Games", "Morning"]);

        // A copy and a converted copy lie next to the original.
        let twin = store.duplicate_scenario(&scenario.id, "Twin").unwrap();
        let canvas = store.to_canvas_copy(&scenario.id, "Canvas").unwrap();
        let folders = store.load_folders();
        assert_eq!(folders.placement.get(&twin.id), Some(&inner.id));
        assert_eq!(folders.placement.get(&canvas.id), Some(&inner.id));

        store.delete_scenario(&twin.id).unwrap();
        assert!(!store.read_folders().placement.contains_key(&twin.id));

        // Without the folder the scenarios are one level up, and the list is as full as before.
        store.delete_folder(&inner.id).unwrap();
        let listed = store.list_scenarios();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|item| item.folder_path == ["Games"]));
        store.move_scenario(&scenario.id, None).unwrap();
        assert!(store.list_scenarios().iter().any(|item| item.id == scenario.id && item.folder_id.is_none()));
        let _ = std::fs::remove_dir_all(store.root());
    }
}
