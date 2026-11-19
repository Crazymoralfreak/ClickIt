import { useEffect, useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { Scenario, ScenarioFormat, ScenarioSummary } from "../../types/scenario";
import { Button, EmptyState, Field, Modal, PageFrame, Segmented, TextInput, cx } from "../../ui";
import { stepCountOf } from "../canvas/graph";
import { ScenarioEditor } from "../editor/ScenarioEditor";
import { ScenarioList } from "./ScenarioList";
import "./scenarios.css";

function summaryOf(scenario: Scenario): ScenarioSummary {
  return {
    id: scenario.id,
    name: scenario.name,
    mode: scenario.mode,
    stepCount: stepCountOf(scenario),
    format: scenario.graph ? "canvas" : "list",
  };
}

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
  const [creating, setCreating] = useState(false);
  // A canvas may take the whole width of the page.
  const [listHidden, setListHidden] = useState(false);
  // Tells the open editor that the list changed its scenario on disk.
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    let cancelled = false;
    api
      .scenarioList()
      .then((list) => {
        if (!cancelled) {
          setItems(list);
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

  /** Shows a new scenario right away, before the list is read again, and selects it. */
  const adopt = (scenario: Scenario) => {
    setItems((current) => [...(current ?? []).filter((item) => item.id !== scenario.id), summaryOf(scenario)]);
    selectScenario(scenario.id);
    touch("scenarios");
  };

  const create = async (name: string, format: ScenarioFormat) => {
    setCreating(false);
    try {
      adopt(await api.scenarioCreate(name, format));
    } catch (error) {
      showError(error);
    }
  };

  const importFile = async () => {
    try {
      const imported = await api.scenarioImport();
      if (imported) {
        adopt(imported);
        notify(t("scenarios.imported", { name: imported.name }));
      }
    } catch (error) {
      showError(error);
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
      adopt(await api.scenarioDuplicate(item.id, t("common.copySuffix", { name: item.name })));
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
  const edited = items?.find((item) => item.id === selectedScenarioId) ?? null;
  const editedId = edited?.id ?? null;
  // Only a canvas has the button that brings the list back.
  const hideList = listHidden && edited?.format === "canvas";

  return (
    <PageFrame
      title={t("nav.scenarios")}
      actions={
        <>
          <Button disabled={locked} onClick={() => void importFile()}>
            {t("scenarios.import")}
          </Button>
          {createButton}
        </>
      }
    >
      {items === null ? (
        <div className="scenarios-loading">{t("common.loading")}</div>
      ) : items.length === 0 ? (
        <EmptyState title={t("scenarios.emptyTitle")} text={t("scenarios.emptyText")} action={createButton} />
      ) : (
        <div className="scenarios-layout">
          {hideList ? null : (
            <ScenarioList
              items={items}
              selectedId={editedId}
              locked={locked}
              onSelect={selectScenario}
              onRename={(item) => void rename(item)}
              onDuplicate={(item) => void duplicate(item)}
              onExport={(item) => void exportFile(item)}
              onDelete={(item) => void remove(item)}
            />
          )}
          <div className={cx("scenarios-editor", edited?.format === "canvas" && "scenarios-editor-canvas")}>
            {editedId !== null ? (
              <ScenarioEditor
                key={editedId}
                scenarioId={editedId}
                revision={revision}
                onAdopt={adopt}
                listHidden={listHidden}
                onToggleList={() => setListHidden((hidden) => !hidden)}
              />
            ) : null}
          </div>
        </div>
      )}
      {creating ? <CreateModal onClose={() => setCreating(false)} onCreate={(name, format) => void create(name, format)} /> : null}
    </PageFrame>
  );
}
