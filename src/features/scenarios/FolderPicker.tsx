import { useT } from "../../i18n";
import type { Folder } from "../../types/scenario";
import { Button, Modal, cx } from "../../ui";
import { ROW_INDENT } from "./FolderRow";
import { folderOptions } from "./folderTree";

interface FolderPickerProps {
  title: string;
  folders: ReadonlyArray<Folder>;
  /** Where the row lies now; `null` is the top of the list. It is marked and not offered. */
  currentId: string | null;
  /** Whether the row may go there. A folder does not go into itself or deeper than the limit. */
  allowed: (folderId: string | null) => boolean;
  onPick: (folderId: string | null) => void;
  /** Set when a new folder may be made for the row right from the dialog. */
  onCreate?: () => void;
  onClose: () => void;
}

/** "Move to": the tree of folders, where a click on a folder puts the row into it. */
export function FolderPicker({ title, folders, currentId, allowed, onPick, onCreate, onClose }: FolderPickerProps) {
  const { t } = useT();

  const option = (folderId: string | null, name: string, depth: number) => {
    const current = folderId === currentId;
    return (
      <button
        key={folderId ?? ""}
        type="button"
        className={cx("folder-pick-item", current && "folder-pick-item-current")}
        style={{ marginLeft: depth * ROW_INDENT }}
        disabled={current || !allowed(folderId)}
        onClick={() => onPick(folderId)}
      >
        <span className="folder-pick-name">{name}</span>
        {current ? <span className="folder-pick-note">{t("scenarios.moveCurrent")}</span> : null}
      </button>
    );
  };

  return (
    <Modal
      title={title}
      onClose={onClose}
      footer={
        <>
          {onCreate ? <Button onClick={onCreate}>{t("scenarios.moveNewFolder")}</Button> : null}
          <Button onClick={onClose}>{t("common.cancel")}</Button>
        </>
      }
    >
      <div className="folder-pick">
        {option(null, t("scenarios.moveRoot"), 0)}
        {folderOptions(folders).map(({ folder, depth }) => option(folder.id, folder.name, depth))}
      </div>
    </Modal>
  );
}
