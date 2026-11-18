// Draft of the scenario being edited: load, debounced save and validation.

import { useCallback, useEffect, useRef, useState } from "react";
import { api, toAppError } from "../../api";
import { useAppEvent } from "../../api/events";
import { useApp } from "../../app/context";
import type { Issue } from "../../types/runtime";
import type { Scenario, ScenarioMode } from "../../types/scenario";
import { countSteps } from "./tree";

const SAVE_DELAY_MS = 400;

/** What the scenario list shows; other screens are told only when this changes. */
interface ListedFields {
  name: string;
  mode: ScenarioMode;
  stepCount: number;
}

function listedFields(scenario: Scenario): ListedFields {
  return { name: scenario.name, mode: scenario.mode, stepCount: countSteps(scenario.steps) };
}

function sameIssues(a: ReadonlyArray<Issue>, b: ReadonlyArray<Issue>): boolean {
  return (
    a.length === b.length &&
    a.every(
      (issue, index) =>
        issue.stepId === b[index].stepId && issue.code === b[index].code && issue.severity === b[index].severity,
    )
  );
}

export interface ScenarioDraft {
  /** Null until the scenario is loaded. */
  draft: Scenario | null;
  /** True when the scenario could not be loaded. */
  failed: boolean;
  issues: ReadonlyArray<Issue>;
  /** Applies an edit; `fn` returns the same object when nothing changes. */
  change: (fn: (scenario: Scenario) => Scenario) => void;
  /** The latest draft, including edits not rendered yet. */
  peek: () => Scenario | null;
  /** Writes pending edits, then reads the scenario from disk again. */
  reload: () => void;
}

export function useScenarioDraft(id: string): ScenarioDraft {
  const { locked, registerFlush, touch, showError } = useApp();
  const [draft, setDraft] = useState<Scenario | null>(null);
  const [failed, setFailed] = useState(false);
  const [issues, setIssues] = useState<ReadonlyArray<Issue>>([]);

  const latest = useRef<Scenario | null>(null);
  const dirty = useRef(false);
  const timer = useRef<number | null>(null);
  const editCount = useRef(0);
  const loadCount = useRef(0);
  const validateCount = useRef(0);
  const saved = useRef<ListedFields | null>(null);
  // Saves run one after another; the chain itself never rejects.
  const saveChain = useRef<Promise<void>>(Promise.resolve());
  const loadDiscard = useRef<() => void>(() => undefined);

  const validate = useCallback(
    (scenario: Scenario) => {
      const request = ++validateCount.current;
      api
        .scenarioValidate(scenario)
        .then((next) => {
          if (validateCount.current === request) {
            setIssues((current) => (sameIssues(current, next) ? current : next));
          }
        })
        .catch(showError);
    },
    [showError],
  );

  /** Saves pending edits right away; rejects when the core refuses them (they stay pending). */
  const flush = useCallback(async () => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    const snapshot = latest.current;
    if (!dirty.current || !snapshot) {
      await saveChain.current;
      return;
    }
    dirty.current = false;
    validate(snapshot);
    const save = saveChain.current.then(async () => {
      try {
        const stored = await api.scenarioSave(snapshot);
        const current = latest.current;
        if (current && current.id === stored.id) {
          const next = { ...current, revision: stored.revision };
          latest.current = next;
          setDraft(next);
        }
      } catch (error) {
        if (toAppError(error).code === "stale") {
          dirty.current = false;
          window.setTimeout(() => loadDiscard.current(), 0);
        } else {
          dirty.current = true;
        }
        throw error;
      }
      const listed = listedFields(snapshot);
      const before = saved.current;
      saved.current = listed;
      if (
        !before ||
        before.name !== listed.name ||
        before.mode !== listed.mode ||
        before.stepCount !== listed.stepCount
      ) {
        touch("scenarios");
      }
    });
    saveChain.current = save.catch(() => undefined);
    await save;
  }, [validate, touch]);

  const load = useCallback(
    async (discardPending: boolean) => {
      const request = ++loadCount.current;
      if (discardPending) {
        if (timer.current !== null) {
          window.clearTimeout(timer.current);
          timer.current = null;
        }
        dirty.current = false;
      } else {
        try {
          await flush();
        } catch (error) {
          // The edits could not be written; reading the file now would throw them away.
          showError(error);
          return;
        }
      }
      const edits = editCount.current;
      try {
        const scenario = await api.scenarioGet(id);
        // A newer load or an edit made meanwhile wins over this result.
        if (loadCount.current !== request || editCount.current !== edits) {
          return;
        }
        latest.current = scenario;
        saved.current = listedFields(scenario);
        setDraft(scenario);
        setFailed(false);
        validate(scenario);
      } catch (error) {
        if (loadCount.current === request) {
          setFailed(latest.current === null);
          showError(error);
        }
      }
    },
    [id, flush, validate, showError],
  );

  const change = useCallback(
    (fn: (scenario: Scenario) => Scenario) => {
      const current = latest.current;
      if (!current) {
        return;
      }
      const next = fn(current);
      if (next === current) {
        return;
      }
      latest.current = next;
      editCount.current += 1;
      dirty.current = true;
      setDraft(next);
      if (timer.current !== null) {
        window.clearTimeout(timer.current);
      }
      timer.current = window.setTimeout(() => {
        timer.current = null;
        flush().catch(showError);
      }, SAVE_DELAY_MS);
    },
    [flush, showError],
  );

  const peek = useCallback(() => latest.current, []);
  const reload = useCallback(() => void load(false), [load]);
  loadDiscard.current = () => void load(true);

  useAppEvent("harness:scenario", (event) => {
    if (event.id === id) {
      void load(true);
    }
  });

  useEffect(() => {
    void load(true);
  }, [load]);

  useEffect(() => registerFlush(flush), [registerFlush, flush]);

  // Leaving the editor (another scenario, another page) writes what is pending.
  useEffect(
    () => () => {
      flush().catch(showError);
    },
    [flush, showError],
  );

  // Recording appended steps on disk; nothing typed here is newer than that.
  useAppEvent("record:finished", (finished) => {
    if (finished.scenarioId === id) {
      void load(true);
    }
  });

  useAppEvent("data:changed", ({ kind }) => {
    if (kind === "scenarios") {
      void load(false);
    }
  });

  const wasLocked = useRef(locked);
  useEffect(() => {
    if (wasLocked.current && !locked) {
      void load(false);
    }
    wasLocked.current = locked;
  }, [locked, load]);

  return { draft, failed, issues, change, peek, reload };
}
