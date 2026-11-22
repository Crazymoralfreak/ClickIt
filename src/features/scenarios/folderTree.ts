// The list of scenarios as a tree of folders. Pure functions: the list, the
// "Move to" dialog and dragging all read the same rules as the core.

import { MAX_FOLDER_DEPTH, type Folder, type ScenarioSummary } from "../../types/scenario";

export interface FolderRowData {
  kind: "folder";
  folder: Folder;
  /** 0 for the top of the list. */
  depth: number;
  /** Scenarios inside, the ones of nested folders included. During a search, the ones that match. */
  count: number;
  open: boolean;
}

export interface ScenarioRowData {
  kind: "scenario";
  item: ScenarioSummary;
  depth: number;
}

export type ListRow = FolderRowData | ScenarioRowData;

function parentOf(folder: Folder): string | null {
  return folder.parentId ?? null;
}

function find(folders: ReadonlyArray<Folder>, id: string | null): Folder | null {
  return id === null ? null : (folders.find((folder) => folder.id === id) ?? null);
}

/** Folders right inside `parentId`, in the order they are shown. */
export function childFolders(folders: ReadonlyArray<Folder>, parentId: string | null): Folder[] {
  return folders.filter((folder) => parentOf(folder) === parentId);
}

function byName(a: ScenarioSummary, b: ScenarioSummary): number {
  const left = a.name.toLowerCase();
  const right = b.name.toLowerCase();
  return left < right ? -1 : left > right ? 1 : 0;
}

/** The folder a scenario is shown in; a folder that is gone means the top of the list. */
export function folderOf(folders: ReadonlyArray<Folder>, item: ScenarioSummary): string | null {
  return find(folders, item.folderId)?.id ?? null;
}

/**
 * Rows of the list from top to bottom: folders first, then the scenarios of
 * the same level by name. A search shows every match with the folders that
 * lead to it, opened, and leaves out the folders without matches.
 */
export function buildRows(
  folders: ReadonlyArray<Folder>,
  items: ReadonlyArray<ScenarioSummary>,
  query: string,
): ListRow[] {
  const needle = query.trim().toLowerCase();
  const searching = needle !== "";
  const placed = new Map<string | null, ScenarioSummary[]>();
  for (const item of items) {
    if (searching && !item.name.toLowerCase().includes(needle)) {
      continue;
    }
    const key = folderOf(folders, item);
    placed.set(key, [...(placed.get(key) ?? []), item]);
  }
  const rows: ListRow[] = [];
  const walk = (parentId: string | null, depth: number): number => {
    let total = 0;
    // The core keeps the tree shallow; this only stops a circle that slipped through.
    const inner = depth <= MAX_FOLDER_DEPTH ? childFolders(folders, parentId) : [];
    for (const folder of inner) {
      const at = rows.length;
      const row: FolderRowData = { kind: "folder", folder, depth, count: 0, open: searching || !folder.collapsed };
      rows.push(row);
      row.count = walk(folder.id, depth + 1);
      if (searching && row.count === 0) {
        rows.length = at;
      } else if (!row.open) {
        rows.length = at + 1;
      }
      total += row.count;
    }
    const own = [...(placed.get(parentId) ?? [])].sort(byName);
    for (const item of own) {
      rows.push({ kind: "scenario", item, depth });
    }
    return total + own.length;
  };
  walk(null, 0);
  return rows;
}

/** 1 for a folder at the top of the list, 0 for the top itself. */
export function folderDepth(folders: ReadonlyArray<Folder>, id: string | null): number {
  let depth = 0;
  let at = find(folders, id);
  while (at !== null && depth <= folders.length) {
    depth += 1;
    at = find(folders, parentOf(at));
  }
  return depth;
}

/** Levels of folders under this one, itself included. */
function folderHeight(folders: ReadonlyArray<Folder>, id: string, guard = 0): number {
  if (guard > folders.length) {
    return 1;
  }
  const inner = childFolders(folders, id).map((folder) => folderHeight(folders, folder.id, guard + 1));
  return 1 + Math.max(0, ...inner);
}

/** Whether `id` is `ancestor` itself or lies somewhere inside it. */
export function isInside(folders: ReadonlyArray<Folder>, id: string | null, ancestor: string): boolean {
  let at = find(folders, id);
  let steps = 0;
  while (at !== null && steps <= folders.length) {
    if (at.id === ancestor) {
      return true;
    }
    steps += 1;
    at = find(folders, parentOf(at));
  }
  return false;
}

/** Whether one more folder fits inside `parentId`. */
export function canCreateIn(folders: ReadonlyArray<Folder>, parentId: string | null): boolean {
  return folderDepth(folders, parentId) < MAX_FOLDER_DEPTH;
}

/** A folder goes neither into itself nor into its own folders, and not deeper than the limit. */
export function canMoveFolder(folders: ReadonlyArray<Folder>, id: string, parentId: string | null): boolean {
  if (parentId === null) {
    return true;
  }
  if (find(folders, parentId) === null || isInside(folders, parentId, id)) {
    return false;
  }
  return folderDepth(folders, parentId) + folderHeight(folders, id) <= MAX_FOLDER_DEPTH;
}

/** The folders after a move, the way the core orders them. */
export function moveFolder(
  folders: ReadonlyArray<Folder>,
  id: string,
  parentId: string | null,
  beforeId: string | null,
): Folder[] {
  const moved = find(folders, id);
  if (moved === null || beforeId === id) {
    return [...folders];
  }
  const rest = folders.filter((folder) => folder.id !== id);
  const plain: Folder = { id: moved.id, name: moved.name, collapsed: moved.collapsed };
  const next: Folder = parentId === null ? plain : { ...plain, parentId };
  const at = beforeId === null ? -1 : rest.findIndex((folder) => folder.id === beforeId);
  rest.splice(at < 0 ? rest.length : at, 0, next);
  return rest;
}

/** The folder shown right after `id` among the ones with the same parent, `skip` left out. */
export function nextSibling(folders: ReadonlyArray<Folder>, id: string, skip: string): string | null {
  const folder = find(folders, id);
  if (folder === null) {
    return null;
  }
  const siblings = childFolders(folders, parentOf(folder)).filter((other) => other.id !== skip);
  const at = siblings.findIndex((other) => other.id === id);
  return siblings[at + 1]?.id ?? null;
}

/** Names from the top of the list down to the folder. */
export function folderPath(folders: ReadonlyArray<Folder>, id: string | null): string[] {
  const names: string[] = [];
  let at = find(folders, id);
  while (at !== null && names.length <= folders.length) {
    names.unshift(at.name);
    at = find(folders, parentOf(at));
  }
  return names;
}

export interface FolderOption {
  folder: Folder;
  depth: number;
}

/** Every folder from top to bottom, the way an open tree shows them. */
export function folderOptions(folders: ReadonlyArray<Folder>): FolderOption[] {
  const out: FolderOption[] = [];
  const walk = (parentId: string | null, depth: number) => {
    if (depth > MAX_FOLDER_DEPTH) {
      return;
    }
    for (const folder of childFolders(folders, parentId)) {
      out.push({ folder, depth });
      walk(folder.id, depth + 1);
    }
  };
  walk(null, 0);
  return out;
}

/** "Folder / Subfolder / Name": how a scenario is named where the tree is not shown. */
export function scenarioLabel(item: Pick<ScenarioSummary, "name" | "folderPath">): string {
  return [...item.folderPath, item.name].join(" / ");
}

/** Scenarios for a list to choose from: by folder, then by name, the way the tree goes. */
export function scenarioChoices(items: ReadonlyArray<ScenarioSummary>): Array<{ item: ScenarioSummary; label: string }> {
  return items
    .map((item) => ({ item, label: scenarioLabel(item) }))
    .sort((a, b) => {
      const left = a.label.toLowerCase();
      const right = b.label.toLowerCase();
      return left < right ? -1 : left > right ? 1 : 0;
    });
}
