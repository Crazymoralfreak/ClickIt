import { useT } from "../../i18n";
import type { ScenarioSummary } from "../../types/scenario";
import { Button, cx } from "../../ui";

interface ScenarioListProps {
  items: ScenarioSummary[];
  selectedId: string | null;
  /** True while a run or a recording is in progress. */
  locked: boolean;
  onSelect: (id: string) => void;
  onRename: (item: ScenarioSummary) => void;
  onDuplicate: (item: ScenarioSummary) => void;
  onExport: (item: ScenarioSummary) => void;
  onDelete: (item: ScenarioSummary) => void;
}

/** Left column of the scenarios page; the selected row shows its actions. */
export function ScenarioList({
  items,
  selectedId,
  locked,
  onSelect,
  onRename,
  onDuplicate,
  onExport,
  onDelete,
}: ScenarioListProps) {
  const { t } = useT();
  return (
    <ul className="scenarios-list" aria-label={t("scenarios.listLabel")}>
      {items.map((item) => {
        const selected = item.id === selectedId;
        return (
          <li key={item.id} className={cx("scenario-item", selected && "scenario-item-selected")}>
            <button
              type="button"
              className="scenario-item-main"
              aria-current={selected ? "true" : undefined}
              onClick={() => onSelect(item.id)}
            >
              <span className="scenario-item-name">{item.name}</span>
              <span className="scenario-item-meta">
                {item.mode === "window" ? t("scenarios.modeWindow") : t("scenarios.modeScreen")}
                {" · "}
                {t("scenarios.stepCount", { count: item.stepCount })}
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
      })}
    </ul>
  );
}
