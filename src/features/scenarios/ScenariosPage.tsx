import { useEffect, useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { Scenario, ScenarioSummary } from "../../types/scenario";
import { Button, EmptyState, PageFrame } from "../../ui";
import { ScenarioEditor } from "../editor/ScenarioEditor";
import { countSteps } from "../editor/tree";
import { ScenarioList } from "./ScenarioList";
import "./scenarios.css";

function summaryOf(scenario: Scenario): ScenarioSummary {
  return { id: scenario.id, name: scenario.name, mode: scenario.mode, stepCount: countSteps(scenario.steps) };
}

/** Scenario list on the left, the editor of the selected scenario on the right. */
export function ScenariosPage() {
  const { t } = useT();
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

  const create = async () => {
    const name = await askName({
      title: t("scenarios.createTitle"),
      initial: t("scenarios.defaultName"),
      okLabel: t("common.create"),
    });
    if (name === null) {
      return;
    }
    try {
      adopt(await api.scenarioCreate(name));
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
    const ok = await confirm({
      title: t("scenarios.deleteTitle", { name: item.name }),
      text: t("scenarios.deleteText"),
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
    <Button variant="primary" disabled={locked} onClick={() => void create()}>
      {t("common.create")}
    </Button>
  );
  const editedId =
    items !== null && items.some((item) => item.id === selectedScenarioId) ? selectedScenarioId : null;

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
          <div className="scenarios-editor">
            {editedId !== null ? <ScenarioEditor key={editedId} scenarioId={editedId} revision={revision} /> : null}
          </div>
        </div>
      )}
    </PageFrame>
  );
}
