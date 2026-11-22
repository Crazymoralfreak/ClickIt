import type { HTMLAttributes } from "react";
import { useT } from "../../i18n";
import type { Folder } from "../../types/scenario";
import { Button, cx } from "../../ui";

/** Pixels a row moves to the right for every folder it lies in. */
export const ROW_INDENT = 12;

interface FolderRowProps {
  folder: Folder;
  depth: number;
  /** Scenarios inside, nested folders included. */
  count: number;
  open: boolean;
  /** The row shows its actions. */
  menuOpen: boolean;
  /** One more folder fits inside this one. */
  canAddInside: boolean;
  /** Where a dragged row would land relative to this one. */
  dropClass?: string | false;
  /** Pointer handlers that drag the folder; none while the list cannot be rearranged. */
  dragProps: HTMLAttributes<HTMLButtonElement>;
  onToggle: () => void;
  onMenu: () => void;
  onCreateInside: () => void;
  onRename: () => void;
  onMoveTo: () => void;
  onDelete: () => void;
}

/** A folder of the list of scenarios: a click opens or closes it, the button on the right shows its actions. */
export function FolderRow({
  folder,
  depth,
  count,
  open,
  menuOpen,
  canAddInside,
  dropClass,
  dragProps,
  onToggle,
  onMenu,
  onCreateInside,
  onRename,
  onMoveTo,
  onDelete,
}: FolderRowProps) {
  const { t } = useT();
  return (
    <li
      className={cx("folder-item", menuOpen && "folder-item-active", dropClass)}
      style={{ marginLeft: depth * ROW_INDENT }}
      data-row="folder"
      data-id={folder.id}
    >
      <div className="folder-item-head">
        <button
          type="button"
          className="folder-item-main"
          aria-expanded={open}
          title={open ? t("scenarios.folderCollapse") : t("scenarios.folderExpand")}
          onClick={onToggle}
          {...dragProps}
        >
          <span className={cx("section-mark", open && "section-mark-open")} aria-hidden="true" />
          <span className="folder-item-name">{folder.name}</span>
          <span className="folder-item-count" title={t("scenarios.folderCount", { count })}>
            {count}
          </span>
        </button>
        <button
          type="button"
          className="folder-item-more"
          aria-expanded={menuOpen}
          aria-label={t("scenarios.folderActions")}
          title={t("scenarios.folderActions")}
          onClick={onMenu}
        >
          ⋯
        </button>
      </div>
      {menuOpen ? (
        <div className="scenario-item-actions">
          <Button
            small
            disabled={!canAddInside}
            title={canAddInside ? undefined : t("errors.folderTooDeep")}
            onClick={onCreateInside}
          >
            {t("scenarios.folderSub")}
          </Button>
          <Button small onClick={onRename}>
            {t("common.rename")}
          </Button>
          <Button small onClick={onMoveTo}>
            {t("scenarios.moveTo")}
          </Button>
          <Button small onClick={onDelete}>
            {t("common.delete")}
          </Button>
        </div>
      ) : null}
    </li>
  );
}
