import { useEffect, useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { QueueSummary, RunTarget, ScenarioSummary, TargetKind } from "../../types/scenario";
import { Button, cx } from "../../ui";
import { statusDetails, statusHeadline } from "./statusText";

function encodeTarget(target: RunTarget | null): string {
  return target ? `${target.kind}:${target.id}` : "";
}

function decodeTarget(value: string): RunTarget | null {
  const split = value.indexOf(":");
  if (split < 0) {
    return null;
  }
  const kind: TargetKind = value.slice(0, split) === "queue" ? "queue" : "scenario";
  return { kind, id: value.slice(split + 1) };
}

/** Bottom panel: what to run, Start / Stop and the live status. */
export function RunBar() {
  const translator = useT();
  const { t } = translator;
  const { settings, saveSettings, status, locked, start, stop, dataVersion, showError } = useApp();
  const [scenarios, setScenarios] = useState<ScenarioSummary[]>([]);
  const [queues, setQueues] = useState<QueueSummary[]>([]);

  useEffect(() => {
    api.scenarioList().then(setScenarios).catch(showError);
  }, [dataVersion.scenarios, showError]);

  useEffect(() => {
    api.queueList().then(setQueues).catch(showError);
  }, [dataVersion.queues, dataVersion.scenarios, showError]);

  const current = settings.current;
  const currentExists =
    current !== null &&
    (current.kind === "scenario"
      ? scenarios.some((item) => item.id === current.id)
      : queues.some((item) => item.id === current.id));
  const details = statusDetails(status, translator);

  return (
    <div className={cx("runbar", `runbar-${status.state}`)}>
      <label className="runbar-target">
        <span className="runbar-label">{t("run.current")}</span>
        <select
          className="input select"
          value={currentExists ? encodeTarget(current) : ""}
          disabled={locked}
          onChange={(event) => void saveSettings({ ...settings, current: decodeTarget(event.target.value) })}
        >
          <option value="">{t("run.noneSelected")}</option>
          {scenarios.length > 0 ? (
            <optgroup label={t("nav.scenarios")}>
              {scenarios.map((item) => (
                <option key={item.id} value={`scenario:${item.id}`}>
                  {item.name}
                </option>
              ))}
            </optgroup>
          ) : null}
          {queues.length > 0 ? (
            <optgroup label={t("nav.queues")}>
              {queues.map((item) => (
                <option key={item.id} value={`queue:${item.id}`}>
                  {item.name}
                </option>
              ))}
            </optgroup>
          ) : null}
        </select>
      </label>
      {locked ? (
        <Button variant="danger" onClick={() => void stop()}>
          {t("run.stop")} ({settings.hotkeyStartStop})
        </Button>
      ) : (
        <Button variant="primary" disabled={!currentExists} onClick={() => void start()}>
          {t("run.start")} ({settings.hotkeyStartStop})
        </Button>
      )}
      <div className="runbar-status" title={settings.cornerStop ? t("run.cornerHint") : undefined}>
        <span className="runbar-dot" />
        <span className="runbar-headline">{statusHeadline(status, translator)}</span>
        {status.target ? <span className="runbar-name">«{status.target.name}»</span> : null}
        {details.length > 0 ? <span className="runbar-details">{details.join(" · ")}</span> : null}
      </div>
    </div>
  );
}
