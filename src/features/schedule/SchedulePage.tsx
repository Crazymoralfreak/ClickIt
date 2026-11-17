import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT, type TKey } from "../../i18n";
import type { QueueSummary, ScenarioSummary, ScheduleRule } from "../../types/scenario";
import { Button, EmptyState, PageFrame, Section, type Option } from "../../ui";
import { RuleCard } from "./RuleCard";
import { defaultRule, firstTarget, ruleErrors, type Targets } from "./scheduleRules";
import "./schedule.css";

const SAVE_DELAY_MS = 400;

const NOTES: ReadonlyArray<TKey> = [
  "schedule.noteLocalTime",
  "schedule.noteRunning",
  "schedule.noteNoCountdown",
  "schedule.noteSkipped",
  "schedule.noteCatchUp",
  "schedule.noteGrid",
];

export function SchedulePage() {
  const { t } = useT();
  const { dataVersion, touch, showError } = useApp();
  const [rules, setRules] = useState<ScheduleRule[] | null>(null);
  const [scenarios, setScenarios] = useState<ScenarioSummary[] | null>(null);
  const [queues, setQueues] = useState<QueueSummary[] | null>(null);

  const rulesRef = useRef<ScheduleRule[] | null>(null);
  /** True while the local list differs from the one on disk. */
  const pendingRef = useRef(false);
  const editsRef = useRef(0);
  const timerRef = useRef<number | null>(null);
  const savesRef = useRef<Promise<void>>(Promise.resolve());

  const targets = useMemo<Targets | null>(
    () => (scenarios && queues ? { scenarios, queues } : null),
    [scenarios, queues],
  );
  const targetsRef = useRef(targets);
  targetsRef.current = targets;

  const targetOptions = useMemo<Option<string>[] | null>(() => {
    if (!targets) {
      return null;
    }
    return [
      ...targets.scenarios.map((scenario) => ({
        value: `scenario:${scenario.id}`,
        label: t("schedule.targetScenario", { name: scenario.name }),
      })),
      ...targets.queues.map((queue) => ({
        value: `queue:${queue.id}`,
        label: t("schedule.targetQueue", { name: queue.name }),
      })),
    ];
  }, [targets, t]);

  useEffect(() => {
    let cancelled = false;
    const editsAtStart = editsRef.current;
    api
      .scheduleGet()
      .then((loaded) => {
        // This page's own saves bump the version too. Edits made meanwhile, or still
        // waiting for a valid list, are newer than the copy on disk and must stay.
        if (cancelled || pendingRef.current || editsRef.current !== editsAtStart) {
          return;
        }
        rulesRef.current = loaded;
        setRules(loaded);
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          showError(error);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [dataVersion.schedule, showError]);

  useEffect(() => {
    let cancelled = false;
    api
      .scenarioList()
      .then((list) => {
        if (!cancelled) {
          setScenarios(list);
        }
      })
      .catch(showError);
    return () => {
      cancelled = true;
    };
  }, [dataVersion.scenarios, showError]);

  useEffect(() => {
    let cancelled = false;
    api
      .queueList()
      .then((list) => {
        if (!cancelled) {
          setQueues(list);
        }
      })
      .catch(showError);
    return () => {
      cancelled = true;
    };
  }, [dataVersion.queues, dataVersion.scenarios, showError]);

  /** Writes the list when it differs from the disk and every rule is valid. */
  const flush = useCallback(async (): Promise<void> => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    const list = rulesRef.current;
    const known = targetsRef.current;
    if (pendingRef.current && list && known && list.every((rule) => ruleErrors(rule, known).length === 0)) {
      pendingRef.current = false;
      savesRef.current = savesRef.current.then(async () => {
        try {
          await api.scheduleSet(list);
          touch("schedule");
        } catch (error) {
          pendingRef.current = true;
          showError(error);
        }
      });
    }
    await savesRef.current;
  }, [touch, showError]);

  useEffect(
    () => () => {
      void flush();
    },
    [flush],
  );

  const change = (next: ScheduleRule[]) => {
    rulesRef.current = next;
    pendingRef.current = true;
    editsRef.current += 1;
    setRules(next);
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
    }
    timerRef.current = window.setTimeout(() => void flush(), SAVE_DELAY_MS);
  };

  const newTarget = targets ? firstTarget(targets) : null;
  const errors = (rules ?? []).map((rule) => ruleErrors(rule, targets));
  const anyInvalid = errors.some((list) => list.length > 0);

  const addButton = (
    <Button
      variant="primary"
      disabled={rules === null || newTarget === null}
      onClick={() => {
        if (rules && newTarget) {
          change([...rules, defaultRule(newTarget, new Date())]);
        }
      }}
    >
      {t("schedule.addRule")}
    </Button>
  );

  return (
    <PageFrame title={t("nav.schedule")} actions={addButton}>
      {targets && newTarget === null ? <div className="schedule-note">{t("schedule.nothingToRun")}</div> : null}
      {anyInvalid ? <div className="schedule-warning">{t("schedule.notSaved")}</div> : null}
      {rules !== null && rules.length === 0 ? (
        <EmptyState title={t("schedule.emptyTitle")} text={t("schedule.emptyText")} />
      ) : null}
      {(rules ?? []).map((rule, index) => (
        <RuleCard
          key={rule.id}
          rule={rule}
          targetOptions={targetOptions}
          errors={errors[index]}
          onChange={(next) => change((rules ?? []).map((current) => (current.id === rule.id ? next : current)))}
          onDelete={() => change((rules ?? []).filter((current) => current.id !== rule.id))}
        />
      ))}
      <Section title={t("schedule.notesTitle")}>
        <ul className="schedule-notes">
          {NOTES.map((key) => (
            <li key={key}>{t(key)}</li>
          ))}
        </ul>
      </Section>
    </PageFrame>
  );
}
