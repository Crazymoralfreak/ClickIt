import { useMemo } from "react";
import { useT } from "../../i18n";
import type { Issue } from "../../types/runtime";
import type { QueueItem, ScenarioSummary } from "../../types/scenario";
import { Button, NumberInput, Select, cx, type Option } from "../../ui";
import { scenarioChoices } from "../scenarios/folderTree";

function newItemId(): string {
  const random = Math.floor(Math.random() * 0x1000000)
    .toString(16)
    .padStart(6, "0");
  return `qi${Date.now().toString(16)}${random}`;
}

interface QueueItemsProps {
  items: QueueItem[];
  /** Null until the scenario list is loaded. */
  scenarios: ScenarioSummary[] | null;
  /** Issues of the whole queue; the rows pick their own by item id. */
  issues: Issue[];
  disabled: boolean;
  onChange: (items: QueueItem[]) => void;
}

/** Ordered rows of a queue: scenario, rounds, move and remove, plus the "Add scenario" button. */
export function QueueItems({ items, scenarios, issues, disabled, onChange }: QueueItemsProps) {
  const { t, td } = useT();

  const scenarioOptions = useMemo<Option<string>[]>(
    () => scenarioChoices(scenarios ?? []).map(({ item, label }) => ({ value: item.id, label })),
    [scenarios],
  );

  const replace = (index: number, item: QueueItem) => {
    onChange(items.map((current, at) => (at === index ? item : current)));
  };

  const move = (index: number, delta: -1 | 1) => {
    const target = index + delta;
    if (target < 0 || target >= items.length) {
      return;
    }
    const next = [...items];
    const moved = next.splice(index, 1);
    next.splice(target, 0, ...moved);
    onChange(next);
  };

  const add = () => {
    if (scenarios === null || scenarios.length === 0) {
      return;
    }
    onChange([...items, { id: newItemId(), scenarioId: scenarios[0].id, rounds: 1 }]);
  };

  return (
    <>
      {items.map((item, index) => {
        const missing = scenarios !== null && !scenarios.some((scenario) => scenario.id === item.scenarioId);
        const codes = Array.from(
          new Set(issues.filter((issue) => issue.stepId === item.id).map((issue) => issue.code)),
        );
        if (missing && !codes.includes("scenarioMissing")) {
          codes.unshift("scenarioMissing");
        }

        // A deleted scenario leaves the Select without a valid value: a disabled
        // placeholder stands in for it until the user picks another scenario.
        let options = scenarioOptions;
        if (scenarios === null) {
          options = [{ value: item.scenarioId, label: t("common.loading"), disabled: true }];
        } else if (missing) {
          options = [
            { value: item.scenarioId, label: t("queues.scenarioPlaceholder"), disabled: true },
            ...scenarioOptions,
          ];
        }

        return (
          <div key={item.id} className={cx("queues-item", codes.length > 0 && "queues-item-error")}>
            <div className="queues-item-row">
              <span className="queues-item-number">{index + 1}</span>
              <Select
                value={item.scenarioId}
                options={options}
                disabled={disabled || scenarios === null}
                invalid={missing}
                title={t("queues.scenario")}
                onChange={(scenarioId) => replace(index, { ...item, scenarioId })}
              />
              <label className="queues-item-rounds">
                <span>{t("queues.rounds")}</span>
                <NumberInput
                  value={item.rounds}
                  min={1}
                  disabled={disabled}
                  invalid={codes.includes("roundsRange")}
                  title={t("queues.roundsTitle")}
                  onChange={(rounds) => replace(index, { ...item, rounds })}
                />
              </label>
              <span className="queues-item-actions">
                <Button small variant="ghost" disabled={disabled || index === 0} onClick={() => move(index, -1)}>
                  {t("common.moveUp")}
                </Button>
                <Button
                  small
                  variant="ghost"
                  disabled={disabled || index === items.length - 1}
                  onClick={() => move(index, 1)}
                >
                  {t("common.moveDown")}
                </Button>
                <Button
                  small
                  variant="ghost"
                  disabled={disabled}
                  onClick={() => onChange(items.filter((current) => current.id !== item.id))}
                >
                  {t("queues.removeItem")}
                </Button>
              </span>
            </div>
            {codes.map((code) => (
              <div key={code} className="queues-item-issues">
                {td(`issues.${code}`, undefined, code)}
              </div>
            ))}
          </div>
        );
      })}
      <div className="queues-add">
        <Button disabled={disabled || scenarios === null || scenarios.length === 0} onClick={add}>
          {t("queues.addItem")}
        </Button>
        {scenarios !== null && scenarios.length === 0 ? (
          <span className="queues-hint">{t("queues.noScenarios")}</span>
        ) : null}
      </div>
    </>
  );
}
