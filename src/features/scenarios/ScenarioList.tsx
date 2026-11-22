import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type HTMLAttributes,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { useT } from "../../i18n";
import type { Folder, ScenarioSummary } from "../../types/scenario";
import { Button, TextInput, cx } from "../../ui";
import { FolderRow, ROW_INDENT } from "./FolderRow";
import { buildRows, canCreateIn, canMoveFolder, folderOf, nextSibling, type ListRow } from "./folderTree";

/** A press that moves further than this is a drag, not a click. */
const DRAG_THRESHOLD = 6;
/** The list scrolls while a dragged row is this close to its top or bottom edge. */
const SCROLL_EDGE = 28;
const SCROLL_STEP = 8;
const SCROLL_TICK_MS = 30;
/** A closed folder opens after a dragged row stayed over it this long. */
const HOVER_OPEN_MS = 700;

type Dragged = { kind: "scenario"; item: ScenarioSummary } | { kind: "folder"; folder: Folder };

/** Where a dragged row lands: inside a folder (`null` is the top of the list) or next to one. */
type DropTarget = { kind: "into"; folderId: string | null } | { kind: "before" | "after"; folderId: string };

interface Press {
  x: number;
  y: number;
  dragged: boolean;
  /** Escape was pressed: the release must not move anything. */
  cancelled: boolean;
  what: Dragged;
}

interface Ghost {
  label: string;
  x: number;
  y: number;
}

function sameTarget(a: DropTarget | null, b: DropTarget | null): boolean {
  return a === b || (a !== null && b !== null && a.kind === b.kind && a.folderId === b.folderId);
}

interface ScenarioListProps {
  items: ScenarioSummary[];
  folders: Folder[];
  selectedId: string | null;
  /** True while a run or a recording is in progress. */
  locked: boolean;
  onSelect: (id: string) => void;
  onRename: (item: ScenarioSummary) => void;
  onDuplicate: (item: ScenarioSummary) => void;
  onExport: (item: ScenarioSummary) => void;
  onDelete: (item: ScenarioSummary) => void;
  /** Opens the choice of a folder for the scenario. */
  onMoveTo: (item: ScenarioSummary) => void;
  onToggleFolder: (folder: Folder, collapsed: boolean) => void;
  onCreateFolder: (parent: Folder) => void;
  onRenameFolder: (folder: Folder) => void;
  /** Opens the choice of a folder to put this one into. */
  onMoveFolderTo: (folder: Folder) => void;
  onDeleteFolder: (folder: Folder) => void;
  /** A scenario was dragged into a folder; `null` is the top of the list. */
  onDropScenario: (item: ScenarioSummary, folderId: string | null) => void;
  /** A folder was dragged into `parentId`, in front of `beforeId` or after the last folder there. */
  onDropFolder: (folder: Folder, parentId: string | null, beforeId: string | null) => void;
}

/**
 * Left column of the scenarios page: the search field and the tree of folders.
 * The selected scenario shows its actions. Rows are dragged with the mouse:
 * a scenario into a folder, a folder into another one or between its neighbours.
 */
export function ScenarioList({
  items,
  folders,
  selectedId,
  locked,
  onSelect,
  onRename,
  onDuplicate,
  onExport,
  onDelete,
  onMoveTo,
  onToggleFolder,
  onCreateFolder,
  onRenameFolder,
  onMoveFolderTo,
  onDeleteFolder,
  onDropScenario,
  onDropFolder,
}: ScenarioListProps) {
  const { t } = useT();
  const [query, setQuery] = useState("");
  const [menuFolderId, setMenuFolderId] = useState<string | null>(null);
  const [ghost, setGhost] = useState<Ghost | null>(null);
  const [drop, setDrop] = useState<DropTarget | null>(null);
  const list = useRef<HTMLUListElement>(null);
  const press = useRef<Press | null>(null);
  const dropNow = useRef<DropTarget | null>(null);
  const pointer = useRef({ x: 0, y: 0 });
  const hover = useRef<{ id: string; timer: number } | null>(null);
  /** Looks again at what lies under the pointer; the list may have scrolled under it. */
  const refresh = useRef<() => void>(() => undefined);

  const searching = query.trim() !== "";
  const rows = useMemo(() => buildRows(folders, items, query), [folders, items, query]);
  const dragging = ghost !== null;

  const endDrag = useCallback(() => {
    if (hover.current) {
      window.clearTimeout(hover.current.timer);
      hover.current = null;
    }
    dropNow.current = null;
    setDrop(null);
    setGhost(null);
  }, []);

  /** What a release at this point would do with the dragged row; `null` when nothing. */
  const targetAt = (x: number, y: number, what: Dragged): DropTarget | null => {
    const box = list.current?.getBoundingClientRect();
    if (!box || x < box.left || x > box.right || y < box.top || y > box.bottom) {
      return null;
    }
    const row = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-row]") ?? null;
    const overFolder = row?.dataset.row === "folder" ? (row.dataset.id ?? null) : null;
    // A scenario row stands for the folder it lies in, empty space for the top of the list.
    const around = row?.dataset.row === "scenario" ? row.dataset.folder || null : null;
    if (what.kind === "scenario") {
      const folderId = overFolder ?? around;
      return folderId === folderOf(folders, what.item) ? null : { kind: "into", folderId };
    }
    const id = what.folder.id;
    const into = (folderId: string | null): DropTarget | null =>
      canMoveFolder(folders, id, folderId) ? { kind: "into", folderId } : null;
    if (row === null || overFolder === null) {
      return into(around);
    }
    if (overFolder === id) {
      return null;
    }
    const at = rows.findIndex((other) => other.kind === "folder" && other.folder.id === overFolder);
    const target = rows[at];
    if (target?.kind !== "folder") {
      return null;
    }
    const head = (row.querySelector(".folder-item-head") ?? row).getBoundingClientRect();
    const part = (y - head.top) / Math.max(1, head.height);
    const beside = canMoveFolder(folders, id, target.folder.parentId ?? null);
    // Under an open folder with rows a line would point at its first row, not after the folder.
    const showsInside = (rows[at + 1]?.depth ?? 0) > target.depth;
    if (beside && part < 0.25) {
      return { kind: "before", folderId: overFolder };
    }
    if (beside && part > 0.75 && !showsInside) {
      return { kind: "after", folderId: overFolder };
    }
    const inside = into(overFolder);
    if (inside !== null || !beside) {
      return inside;
    }
    return { kind: part < 0.5 || showsInside ? "before" : "after", folderId: overFolder };
  };

  const aim = (target: DropTarget | null) => {
    dropNow.current = target;
    setDrop((current) => (sameTarget(current, target) ? current : target));
    const closed =
      target?.kind === "into" && target.folderId !== null
        ? (folders.find((folder) => folder.id === target.folderId && folder.collapsed) ?? null)
        : null;
    if (hover.current?.id === closed?.id) {
      return;
    }
    if (hover.current) {
      window.clearTimeout(hover.current.timer);
      hover.current = null;
    }
    if (closed !== null) {
      const timer = window.setTimeout(() => {
        hover.current = null;
        onToggleFolder(closed, false);
      }, HOVER_OPEN_MS);
      hover.current = { id: closed.id, timer };
    }
  };

  useEffect(() => {
    refresh.current = () => {
      const start = press.current;
      if (start?.dragged && !start.cancelled) {
        aim(targetAt(pointer.current.x, pointer.current.y, start.what));
      }
    };
  });

  // While a row is dragged: Escape drops it back, and the list scrolls at its edges.
  useEffect(() => {
    if (!dragging) {
      return;
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (press.current) {
          press.current.cancelled = true;
        }
        endDrag();
      }
    };
    const timer = window.setInterval(() => {
      const element = list.current;
      if (!element) {
        return;
      }
      const box = element.getBoundingClientRect();
      const { x, y } = pointer.current;
      if (x < box.left || x > box.right) {
        return;
      }
      const before = element.scrollTop;
      if (y < box.top + SCROLL_EDGE) {
        element.scrollTop -= SCROLL_STEP;
      } else if (y > box.bottom - SCROLL_EDGE) {
        element.scrollTop += SCROLL_STEP;
      }
      if (element.scrollTop !== before) {
        refresh.current();
      }
    }, SCROLL_TICK_MS);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [dragging, endDrag]);

  useEffect(() => endDrag, [endDrag]);

  const commit = (what: Dragged, target: DropTarget) => {
    if (what.kind === "scenario") {
      if (target.kind === "into") {
        onDropScenario(what.item, target.folderId);
      }
      return;
    }
    if (target.kind === "into") {
      onDropFolder(what.folder, target.folderId, null);
      return;
    }
    const neighbour = folders.find((folder) => folder.id === target.folderId);
    if (!neighbour) {
      return;
    }
    const parentId = neighbour.parentId ?? null;
    switch (target.kind) {
      case "before":
        onDropFolder(what.folder, parentId, neighbour.id);
        break;
      case "after":
        onDropFolder(what.folder, parentId, nextSibling(folders, neighbour.id, what.folder.id));
        break;
      default: {
        const unreachable: never = target;
        throw new Error(`Unknown drop target: ${String(unreachable)}`);
      }
    }
  };

  // The pointer handlers do the dragging; HTML drag and drop is taken by the window for dropped files.
  // A search shows a part of the tree, so nothing is rearranged in it.
  const dragProps = (what: Dragged, label: string): HTMLAttributes<HTMLButtonElement> => {
    if (searching) {
      return {};
    }
    return {
      onPointerDown: (event: ReactPointerEvent<HTMLButtonElement>) => {
        if (event.button !== 0) {
          return;
        }
        press.current = { x: event.clientX, y: event.clientY, dragged: false, cancelled: false, what };
        event.currentTarget.setPointerCapture(event.pointerId);
      },
      onPointerMove: (event: ReactPointerEvent<HTMLButtonElement>) => {
        const start = press.current;
        if (!start || start.cancelled) {
          return;
        }
        if (!start.dragged && Math.hypot(event.clientX - start.x, event.clientY - start.y) < DRAG_THRESHOLD) {
          return;
        }
        start.dragged = true;
        pointer.current = { x: event.clientX, y: event.clientY };
        setGhost({ label, x: event.clientX, y: event.clientY });
        aim(targetAt(event.clientX, event.clientY, start.what));
      },
      onPointerUp: () => {
        const start = press.current;
        const target = dropNow.current;
        endDrag();
        if (start?.dragged && !start.cancelled && target !== null) {
          commit(start.what, target);
        }
        // The click that follows a drag must neither select the row nor fold the folder.
        window.setTimeout(() => {
          press.current = null;
        }, 0);
      },
      onPointerCancel: () => {
        press.current = null;
        endDrag();
      },
    };
  };

  /** A click that ends a drag is not a click. */
  const clicked = (run: () => void) => () => {
    if (!press.current?.dragged) {
      run();
    }
  };

  const dropClass = (folderId: string) => {
    if (drop === null || drop.folderId !== folderId) {
      return false;
    }
    switch (drop.kind) {
      case "into":
        return "scenarios-drop-into";
      case "before":
        return "scenarios-drop-before";
      case "after":
        return "scenarios-drop-after";
      default: {
        const unreachable: never = drop;
        throw new Error(`Unknown drop target: ${String(unreachable)}`);
      }
    }
  };

  const renderRow = (row: ListRow) => {
    if (row.kind === "folder") {
      const { folder } = row;
      return (
        <FolderRow
          key={`folder-${folder.id}`}
          folder={folder}
          depth={row.depth}
          count={row.count}
          open={row.open}
          menuOpen={menuFolderId === folder.id}
          canAddInside={canCreateIn(folders, folder.id)}
          dropClass={dropClass(folder.id)}
          dragProps={dragProps({ kind: "folder", folder }, folder.name)}
          // A search opens every folder it shows; folding one would do nothing visible.
          onToggle={clicked(() => (searching ? undefined : onToggleFolder(folder, row.open)))}
          onMenu={() => setMenuFolderId((current) => (current === folder.id ? null : folder.id))}
          onCreateInside={() => onCreateFolder(folder)}
          onRename={() => onRenameFolder(folder)}
          onMoveTo={() => onMoveFolderTo(folder)}
          onDelete={() => onDeleteFolder(folder)}
        />
      );
    }
    const { item } = row;
    const selected = item.id === selectedId;
    return (
      <li
        key={`scenario-${item.id}`}
        className={cx("scenario-item", selected && "scenario-item-selected")}
        style={{ marginLeft: row.depth * ROW_INDENT }}
        data-row="scenario"
        data-folder={folderOf(folders, item) ?? ""}
      >
        <button
          type="button"
          className="scenario-item-main"
          aria-current={selected ? "true" : undefined}
          onClick={clicked(() => onSelect(item.id))}
          {...dragProps({ kind: "scenario", item }, item.name)}
        >
          <span className="scenario-item-name">{item.name}</span>
          <span className="scenario-item-meta">
            {item.mode === "window" ? t("scenarios.modeWindow") : t("scenarios.modeScreen")}
            {" · "}
            {item.format === "canvas"
              ? `${t("scenarios.blockCount", { count: item.stepCount })} · ${t("scenarios.canvasMark")}`
              : t("scenarios.stepCount", { count: item.stepCount })}
          </span>
        </button>
        {selected ? (
          <div className="scenario-item-actions">
            <Button small disabled={locked} onClick={() => onRename(item)}>
              {t("common.rename")}
            </Button>
            <Button small disabled={locked} onClick={() => onDuplicate(item)}>
              {t("common.duplicate")}
            </Button>
            <Button small onClick={() => onMoveTo(item)}>
              {t("scenarios.moveTo")}
            </Button>
            <Button small onClick={() => onExport(item)}>
              {t("scenarios.export")}
            </Button>
            <Button small disabled={locked} onClick={() => onDelete(item)}>
              {t("common.delete")}
            </Button>
          </div>
        ) : null}
      </li>
    );
  };

  return (
    <div className="scenarios-side">
      <TextInput
        type="search"
        className="scenarios-search"
        value={query}
        placeholder={t("scenarios.search")}
        aria-label={t("scenarios.search")}
        onChange={setQuery}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            setQuery("");
          }
        }}
      />
      {searching && rows.length === 0 ? <div className="scenarios-none">{t("scenarios.searchEmpty")}</div> : null}
      <ul
        ref={list}
        className={cx(
          "scenarios-list",
          dragging && "scenarios-list-dragging",
          drop?.kind === "into" && drop.folderId === null && "scenarios-list-drop",
        )}
        aria-label={t("scenarios.listLabel")}
      >
        {rows.map(renderRow)}
      </ul>
      {ghost ? (
        <div className="scenarios-ghost" style={{ left: ghost.x, top: ghost.y }}>
          {ghost.label}
        </div>
      ) : null}
    </div>
  );
}
