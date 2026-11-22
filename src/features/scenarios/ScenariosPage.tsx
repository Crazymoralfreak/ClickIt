import { useEffect, useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { Folder, Scenario, ScenarioFormat, ScenarioSummary } from "../../types/scenario";
import { Button, EmptyState, Field, Modal, PageFrame, Segmented, TextInput, cx } from "../../ui";
import { stepCountOf } from "../canvas/graph";
import { ScenarioEditor } from "../editor/ScenarioEditor";
import { FolderPicker } from "./FolderPicker";
import { ScenarioList } from "./ScenarioList";
import { canMoveFolder, folderOf, folderPath, moveFolder } from "./folderTree";
import "./scenarios.css";

function summaryOf(scenario: Scenario, folderId: string | null, path: string[]): ScenarioSummary {
  return {
    id: scenario.id,
    name: scenario.name,
    mode: scenario.mode,
    stepCount: stepCountOf(scenario),
    format: scenario.graph ? "canvas" : "list",
    folderId,
    folderPath: path,
  };
}

/** The row "Move to" was asked for. */
type Moving = { kind: "scenario"; item: ScenarioSummary } | { kind: "folder"; folder: Folder };

interface CreateModalProps {
  onClose: () => void;
  onCreate: (name: string, format: ScenarioFormat) => void;
}

/** Asks for the name of a new scenario and for the way it is edited: a list or a canvas. */
function CreateModal({ onClose, onCreate }: CreateModalProps) {
  const { t } = useT();
  const [name, setName] = useState(t("scenarios.defaultName"));
  const [format, setFormat] = useState<ScenarioFormat>("list");
  const trimmed = name.trim();
  const submit = () => {
    if (trimmed !== "") {
      onCreate(trimmed, format);
    }
  };
  return (
    <Modal
      title={t("scenarios.createTitle")}
      onClose={onClose}
      wide
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" disabled={trimmed === ""} onClick={submit}>
            {t("common.create")}
          </Button>
        </>
      }
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label={t("common.name")}>
          <TextInput
            className="scenarios-create-name"
            value={name}
            maxLength={80}
            autoFocus
            invalid={trimmed === ""}
            onChange={setName}
            onFocus={(event) => event.target.select()}
          />
        </Field>
        <Field
          label={t("scenarios.format")}
          hint={format === "canvas" ? t("scenarios.formatCanvasHint") : t("scenarios.formatListHint")}
        >
          <Segmented<ScenarioFormat>
            value={format}
            options={[
              { value: "list", label: t("scenarios.formatList") },
              { value: "canvas", label: t("scenarios.formatCanvas") },
            ]}
            onChange={setFormat}
          />
        </Field>
      </form>
    </Modal>
  );
}

/** Scenario list on the left, the editor of the selected scenario on the right. */
export function ScenariosPage() {
  const { t, language } = useT();
  const {
    locked,
    dataVersion,
    selectedScenarioId,
    selectScenario,
    touch,
    notify,
    showError,
    confirm,
    askName,
    flushAll,
  } = useApp();
  const [items, setItems] = useState<ScenarioSummary[] | null>(null);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [creating, setCreating] = useState(false);
  const [moving, setMoving] = useState<Moving | null>(null);
  // A canvas may take the whole width of the page.
  const [listHidden, setListHidden] = useState(false);
  // Tells the open editor that the list changed its scenario on disk.
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    let cancelled = false;
    Promise.all([api.scenarioList(), api.folderList()])
      .then(([list, tree]) => {
        if (!cancelled) {
          setItems(list);
          setFolders(tree);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setItems((current) => current ?? []);
          showError(error);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [dataVersion.scenarios, showError]);

  // The selection always points at a scenario of the list, the first one by default.
  useEffect(() => {
    if (items === null || items.some((item) => item.id === selectedScenarioId)) {
      return;
    }
    selectScenario(items[0]?.id ?? null);
  }, [items, selectedScenarioId, selectScenario]);

  const edited = items?.find((item) => item.id === selectedScenarioId) ?? null;
  const editedId = edited?.id ?? null;
  /** A scenario made next to the open one lies in the same folder. */
  const editedFolder = edited ? folderOf(folders, edited) : null;

  /** Shows a new scenario right away, before the list is read again, and selects it. */
  const adopt = (scenario: Scenario, folderId: string | null) => {
    const summary = summaryOf(scenario, folderId, folderPath(folders, folderId));
    setItems((current) => [...(current ?? []).filter((item) => item.id !== scenario.id), summary]);
    selectScenario(scenario.id);
    touch("scenarios");
  };

  const create = async (name: string, format: ScenarioFormat) => {
    setCreating(false);
    try {
      adopt(await api.scenarioCreate(name, format, editedFolder), editedFolder);
    } catch (error) {
      showError(error);
    }
  };

  const importFile = async () => {
    try {
      const imported = await api.scenarioImport();
      if (imported) {
        adopt(imported, null);
        notify(t("scenarios.imported", { name: imported.name }));
      }
    } catch (error) {
      showError(error);
    }
  };

  // Folders. They are a view only, so they may be changed while something runs.

  const toggleFolder = async (folder: Folder, collapsed: boolean) => {
    const set = (value: boolean) =>
      setFolders((current) => current.map((other) => (other.id === folder.id ? { ...other, collapsed: value } : other)));
    set(collapsed);
    try {
      await api.folderSetCollapsed(folder.id, collapsed);
    } catch (error) {
      set(folder.collapsed);
      showError(error);
    }
  };

  /** Asks for a name and makes a folder; resolves to `null` when cancelled or refused. */
  const createFolder = async (parent: Folder | null): Promise<Folder | null> => {
    const name = await askName({
      title: parent ? t("scenarios.folderSubTitle", { name: parent.name }) : t("scenarios.folderCreateTitle"),
      initial: t("scenarios.folderDefaultName"),
      okLabel: t("common.create"),
    });
    if (name === null) {
      return null;
    }
    try {
      const created = await api.folderCreate(name, parent?.id ?? null);
      // A new folder inside a closed one would not be seen.
      setFolders((current) => [
        ...current.map((other) => (other.id === parent?.id ? { ...other, collapsed: false } : other)),
        created,
      ]);
      if (parent?.collapsed) {
        await api.folderSetCollapsed(parent.id, false);
      }
      return created;
    } catch (error) {
      showError(error);
      return null;
    }
  };

  const renameFolder = async (folder: Folder) => {
    const name = await askName({
      title: t("scenarios.folderRenameTitle"),
      initial: folder.name,
      okLabel: t("common.rename"),
    });
    if (name === null || name === folder.name) {
      return;
    }
    try {
      await api.folderRename(folder.id, name);
      setFolders((current) => current.map((other) => (other.id === folder.id ? { ...other, name } : other)));
      // The other screens show the folder in the names of its scenarios.
      touch("scenarios");
    } catch (error) {
      showError(error);
    }
  };

  const deleteFolder = async (folder: Folder) => {
    const ok = await confirm({
      title: t("scenarios.folderDeleteTitle", { name: folder.name }),
      text: t("scenarios.folderDeleteText"),
      okLabel: t("common.delete"),
    });
    if (!ok) {
      return;
    }
    try {
      await api.folderDelete(folder.id);
    } catch (error) {
      showError(error);
    }
    touch("scenarios");
  };

  /** Shows the scenario in its new folder at once; a refusal of the core puts it back. */
  const moveScenario = async (item: ScenarioSummary, folderId: string | null) => {
    const place = (id: string | null) =>
      setItems((current) =>
        (current ?? []).map((other) =>
          other.id === item.id ? { ...other, folderId: id, folderPath: folderPath(folders, id) } : other,
        ),
      );
    place(folderId);
    try {
      await api.scenarioMove(item.id, folderId);
    } catch (error) {
      place(folderOf(folders, item));
      showError(error);
    }
    touch("scenarios");
  };

  /** Shows the folder in its new place at once; a refusal of the core puts it back. */
  const moveFolderTo = async (folder: Folder, parentId: string | null, beforeId: string | null) => {
    const before = folders;
    setFolders(moveFolder(folders, folder.id, parentId, beforeId));
    try {
      setFolders(await api.folderMove(folder.id, parentId, beforeId));
    } catch (error) {
      setFolders(before);
      showError(error);
    }
    // The paths of the scenarios inside are read again.
    touch("scenarios");
  };

  /** "New folder…" of the "Move to" dialog: a folder at the top of the list with the scenario in it. */
  const moveToNewFolder = async (item: ScenarioSummary) => {
    setMoving(null);
    const created = await createFolder(null);
    if (created !== null) {
      await moveScenario(item, created.id);
    }
  };

  const rename = async (item: ScenarioSummary) => {
    const name = await askName({
      title: t("scenarios.renameTitle"),
      initial: item.name,
      okLabel: t("common.rename"),
    });
    if (name === null || name === item.name) {
      return;
    }
    try {
      // Pending edits of the open editor go to disk first, so the rename does not lose them.
      await flushAll();
      const scenario = await api.scenarioGet(item.id);
      await api.scenarioSave({ ...scenario, name });
      touch("scenarios");
      setRevision((current) => current + 1);
    } catch (error) {
      showError(error);
    }
  };

  const duplicate = async (item: ScenarioSummary) => {
    try {
      await flushAll();
      // The core puts a copy next to the original.
      adopt(await api.scenarioDuplicate(item.id, t("common.copySuffix", { name: item.name })), folderOf(folders, item));
    } catch (error) {
      showError(error);
    }
  };

  const exportFile = async (item: ScenarioSummary) => {
    try {
      await flushAll();
      const path = await api.scenarioExport(item.id);
      if (path !== null) {
        notify(t("scenarios.exported", { path }));
      }
    } catch (error) {
      showError(error);
    }
  };

  const remove = async (item: ScenarioSummary) => {
    let text = t("scenarios.deleteText");
    try {
      // "Run scenario" blocks of other scenarios stop working without this one.
      const { callers } = await api.scenarioMeta(item.id);
      if (callers.length > 0) {
        const quote = (name: string) => (language === "ru" ? `«${name}»` : `"${name}"`);
        text = `${t("scenarios.deleteCalledText", { names: callers.map((caller) => quote(caller.name)).join(", ") })} ${text}`;
      }
    } catch {
      // The plain question is asked when the callers cannot be found out.
    }
    const ok = await confirm({
      title: t("scenarios.deleteTitle", { name: item.name }),
      text,
      okLabel: t("common.delete"),
      danger: true,
    });
    if (!ok) {
      return;
    }
    try {
      // Nothing may be left to save: a late save would bring the scenario back.
      await flushAll();
      await api.scenarioDelete(item.id);
      setItems((current) => (current ?? []).filter((other) => other.id !== item.id));
      touch("scenarios");
    } catch (error) {
      showError(error);
    }
  };

  const createButton = (
    <Button variant="primary" disabled={locked} onClick={() => setCreating(true)}>
      {t("common.create")}
    </Button>
  );
  // Only a canvas has the button that brings the list back.
  const hideList = listHidden && edited?.format === "canvas";

  return (
    <PageFrame
      title={t("nav.scenarios")}
      actions={
        <>
          <Button onClick={() => void createFolder(null)}>{t("scenarios.folderCreate")}</Button>
          <Button disabled={locked} onClick={() => void importFile()}>
            {t("scenarios.import")}
          </Button>
          {createButton}
        </>
      }
    >
      {items === null ? (
        <div className="scenarios-loading">{t("common.loading")}</div>
      ) : items.length === 0 && folders.length === 0 ? (
        <EmptyState title={t("scenarios.emptyTitle")} text={t("scenarios.emptyText")} action={createButton} />
      ) : (
        <div className="scenarios-layout">
          {hideList ? null : (
            <ScenarioList
              items={items}
              folders={folders}
              selectedId={editedId}
              locked={locked}
              onSelect={selectScenario}
              onRename={(item) => void rename(item)}
              onDuplicate={(item) => void duplicate(item)}
              onExport={(item) => void exportFile(item)}
              onDelete={(item) => void remove(item)}
              onMoveTo={(item) => setMoving({ kind: "scenario", item })}
              onToggleFolder={(folder, collapsed) => void toggleFolder(folder, collapsed)}
              onCreateFolder={(parent) => void createFolder(parent)}
              onRenameFolder={(folder) => void renameFolder(folder)}
              onMoveFolderTo={(folder) => setMoving({ kind: "folder", folder })}
              onDeleteFolder={(folder) => void deleteFolder(folder)}
              onDropScenario={(item, folderId) => void moveScenario(item, folderId)}
              onDropFolder={(folder, parentId, beforeId) => void moveFolderTo(folder, parentId, beforeId)}
            />
          )}
          <div className={cx("scenarios-editor", edited?.format === "canvas" && "scenarios-editor-canvas")}>
            {editedId !== null ? (
              <ScenarioEditor
                key={editedId}
                scenarioId={editedId}
                revision={revision}
                onAdopt={(scenario) => adopt(scenario, editedFolder)}
                listHidden={listHidden}
                onToggleList={() => setListHidden((hidden) => !hidden)}
              />
            ) : null}
          </div>
        </div>
      )}
      {creating ? <CreateModal onClose={() => setCreating(false)} onCreate={(name, format) => void create(name, format)} /> : null}
      {moving?.kind === "scenario" ? (
        <FolderPicker
          title={t("scenarios.moveTitle", { name: moving.item.name })}
          folders={folders}
          currentId={folderOf(folders, moving.item)}
          allowed={() => true}
          onPick={(folderId) => {
            setMoving(null);
            void moveScenario(moving.item, folderId);
          }}
          onCreate={() => void moveToNewFolder(moving.item)}
          onClose={() => setMoving(null)}
        />
      ) : null}
      {moving?.kind === "folder" ? (
        <FolderPicker
          title={t("scenarios.moveTitle", { name: moving.folder.name })}
          folders={folders}
          currentId={moving.folder.parentId ?? null}
          allowed={(folderId) => canMoveFolder(folders, moving.folder.id, folderId)}
          onPick={(folderId) => {
            setMoving(null);
            void moveFolderTo(moving.folder, folderId, null);
          }}
          onClose={() => setMoving(null)}
        />
      ) : null}
    </PageFrame>
  );
}
