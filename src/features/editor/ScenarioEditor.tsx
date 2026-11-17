import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { Issue } from "../../types/runtime";
import { MAX_DEPTH, MAX_STEPS, type Scenario, type Step } from "../../types/scenario";
import { Button, Field, PauseInput, RepeatModeInput, Section, cx } from "../../ui";
import { createStep, isBlockType } from "./defaults";
import { EditorEnvContext, type EditorEnv, type IssueMap, type StepActions } from "./editorContext";
import { ScenarioHeader } from "./ScenarioHeader";
import { StepList } from "./StepList";
import {
  blockLevels,
  cloneStep,
  containsStep,
  countSteps,
  depthOfList,
  insertAfter,
  insertStep,
  locateStep,
  moveStep,
  removeStep,
  stepsOfList,
  updateStep,
} from "./tree";
import { useScenarioDraft } from "./useScenarioDraft";
import "./editor.css";

interface ScenarioEditorProps {
  scenarioId: string;
  /** Grows when the scenario was changed on disk by the scenario list (a rename). */
  revision: number;
}

/** Editor of one scenario; mount it with `key={scenarioId}`. */
export function ScenarioEditor({ scenarioId, revision }: ScenarioEditorProps) {
  const { t } = useT();
  const { draft, failed, issues, change, peek, reload } = useScenarioDraft(scenarioId);

  const seenRevision = useRef(revision);
  useEffect(() => {
    if (seenRevision.current !== revision) {
      seenRevision.current = revision;
      reload();
    }
  }, [revision, reload]);

  if (!draft) {
    return <div className="editor-status">{failed ? t("editor.loadFailed") : t("common.loading")}</div>;
  }
  return <EditorBody draft={draft} issues={issues} change={change} peek={peek} />;
}

function sameIssueList(a: ReadonlyArray<Issue>, b: ReadonlyArray<Issue>): boolean {
  return (
    a.length === b.length &&
    a.every((issue, index) => issue.code === b[index].code && issue.severity === b[index].severity)
  );
}

/** Splits issues into the ones of the scenario and the ones of each step. */
function useGroupedIssues(issues: ReadonlyArray<Issue>): { general: Issue[]; byStep: IssueMap } {
  const previous = useRef<IssueMap>(new Map());
  return useMemo(() => {
    const general: Issue[] = [];
    const byStep = new Map<string, ReadonlyArray<Issue>>();
    for (const issue of issues) {
      if (issue.stepId === null) {
        general.push(issue);
      } else {
        byStep.set(issue.stepId, [...(byStep.get(issue.stepId) ?? []), issue]);
      }
    }
    // Unchanged lists keep their identity so that the rows of those steps do not re-render.
    for (const [stepId, list] of byStep) {
      const before = previous.current.get(stepId);
      if (before && sameIssueList(before, list)) {
        byStep.set(stepId, before);
      }
    }
    previous.current = byStep;
    return { general, byStep };
  }, [issues]);
}

interface EditorBodyProps {
  draft: Scenario;
  issues: ReadonlyArray<Issue>;
  change: (fn: (scenario: Scenario) => Scenario) => void;
  peek: () => Scenario | null;
}

function EditorBody({ draft, issues, change, peek }: EditorBodyProps) {
  const { t, td } = useT();
  const { locked, start, flushAll, showError, notify, confirm } = useApp();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selectedRef = useRef(selectedId);
  selectedRef.current = selectedId;

  const { general, byStep } = useGroupedIssues(issues);
  const total = useMemo(() => countSteps(draft.steps), [draft.steps]);
  const full = total >= MAX_STEPS;

  const env = useMemo<EditorEnv>(
    () => ({ scenarioId: draft.id, mode: draft.mode, target: draft.target }),
    [draft.id, draft.mode, draft.target],
  );

  const actions = useMemo<StepActions>(() => {
    const edit = (fn: (steps: Step[]) => Step[]) =>
      change((scenario) => {
        const steps = fn(scenario.steps);
        return steps === scenario.steps ? scenario : { ...scenario, steps };
      });

    return {
      toggle: (id) => setSelectedId((current) => (current === id ? null : id)),
      update: (id, fn) => edit((steps) => updateStep(steps, id, fn)),
      move: (id, delta) => edit((steps) => moveStep(steps, id, delta)),
      duplicate: (id) => {
        const scenario = peek();
        const found = scenario ? locateStep(scenario.steps, id) : null;
        if (!scenario || !found) {
          return;
        }
        if (countSteps(scenario.steps) + countSteps([found.step]) > MAX_STEPS) {
          notify(t("editor.duplicateTooMany", { max: MAX_STEPS }), "error");
          return;
        }
        if (found.depth + blockLevels(found.step) > MAX_DEPTH) {
          notify(t("editor.duplicateTooDeep", { max: MAX_DEPTH }), "error");
          return;
        }
        const copy = cloneStep(found.step);
        edit((steps) => insertAfter(steps, id, copy));
        setSelectedId(copy.id);
      },
      remove: (id) => {
        const scenario = peek();
        const found = scenario ? locateStep(scenario.steps, id) : null;
        if (!found) {
          return;
        }
        const removeNow = () => {
          edit((steps) => removeStep(steps, id));
          setSelectedId((current) => (current !== null && containsStep(found.step, current) ? null : current));
        };
        const nested = countSteps([found.step]) - 1;
        if (nested === 0) {
          removeNow();
          return;
        }
        // There is no undo, so a block that takes other steps with it asks first.
        void confirm({
          title: t("editor.deleteBlockTitle"),
          text: t("editor.deleteBlockText", { count: nested }),
          okLabel: t("common.delete"),
          danger: true,
        }).then((ok) => {
          if (ok) {
            removeNow();
          }
        });
      },
      add: (list, type) => {
        const scenario = peek();
        if (!scenario) {
          return;
        }
        if (countSteps(scenario.steps) >= MAX_STEPS) {
          notify(t("errors.stepLimit", { max: MAX_STEPS }), "error");
          return;
        }
        const depth = depthOfList(scenario.steps, list);
        const siblings = stepsOfList(scenario.steps, list);
        if (depth === null || siblings === null) {
          return;
        }
        if (isBlockType(type) && depth >= MAX_DEPTH) {
          notify(t("editor.limitDepth", { max: MAX_DEPTH }), "error");
          return;
        }
        const step = createStep(type, scenario.mode);
        // After the selected step when it is in this very list, otherwise at the end.
        const selectedIndex = siblings.findIndex((item) => item.id === selectedRef.current);
        const index = selectedIndex >= 0 ? selectedIndex + 1 : siblings.length;
        edit((steps) => insertStep(steps, list, index, step));
        setSelectedId(step.id);
      },
    };
  }, [change, peek, notify, confirm, t]);

  const record = async () => {
    try {
      await flushAll();
      await api.recordStart(draft.id);
    } catch (error) {
      showError(error);
    }
  };

  const pause = draft.pauseBetweenRounds;
  const showRoundPause = draft.repeat.kind !== "once" || pause.fromMs !== 0 || pause.toMs !== 0;

  return (
    <EditorEnvContext.Provider value={env}>
      <div className="editor">
        {locked ? (
          <div className="editor-banner" role="status">
            {t("editor.lockedBanner")}
          </div>
        ) : null}
        {general.length > 0 ? (
          <ul className="editor-issues">
            {general.map((issue) => (
              <li key={issue.code} className={cx("issue", `issue-${issue.severity}`)}>
                {td(`issues.${issue.code}`)}
              </li>
            ))}
          </ul>
        ) : null}
        <ScenarioHeader draft={draft} change={change} disabled={locked} />
        <Section title={t("editor.repeatSection")}>
          <Field label={t("editor.repeat")}>
            <RepeatModeInput
              value={draft.repeat}
              disabled={locked}
              invalid={draft.repeat.kind === "count" && draft.repeat.times < 1}
              onChange={(repeat) => change((scenario) => ({ ...scenario, repeat }))}
            />
          </Field>
          {showRoundPause ? (
            <Field label={t("editor.pauseBetweenRounds")}>
              <PauseInput
                value={pause}
                disabled={locked}
                onChange={(pauseBetweenRounds) => change((scenario) => ({ ...scenario, pauseBetweenRounds }))}
              />
            </Field>
          ) : null}
        </Section>
        <Section title={t("editor.runSection")}>
          <div className="editor-run">
            <Button variant="primary" disabled={locked} onClick={() => void start({ kind: "scenario", id: draft.id })}>
              {t("editor.start")}
            </Button>
            <Button disabled={locked} onClick={() => void record()}>
              {t("record.start")}
            </Button>
            <span className="editor-run-note">{t("record.storageHint")}</span>
          </div>
          <p className="editor-hint">{t("record.appendHint")}</p>
        </Section>
        <Section title={t("editor.stepsTitle", { count: total, max: MAX_STEPS })}>
          <p className="editor-hint">{t("editor.stepsHint")}</p>
          {full ? <p className="editor-limit">{t("errors.stepLimit", { max: MAX_STEPS })}</p> : null}
          {draft.steps.length === 0 ? <p className="editor-hint">{t("editor.stepsEmpty")}</p> : null}
          <StepList
            steps={draft.steps}
            parentId={null}
            slot="root"
            prefix=""
            depth={0}
            selectedId={selectedId}
            issues={byStep}
            disabled={locked}
            full={full}
            actions={actions}
          />
        </Section>
      </div>
    </EditorEnvContext.Provider>
  );
}
