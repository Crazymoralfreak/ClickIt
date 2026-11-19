import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { Issue, RunOptions } from "../../types/runtime";
import { MAX_DEPTH, MAX_STEPS, type Graph, type Scenario, type Step } from "../../types/scenario";
import { Button, Field, Modal, PauseInput, RepeatModeInput, Section, cx } from "../../ui";
import { CanvasEditor } from "../canvas/CanvasEditor";
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
  /** Shows a scenario the editor created (a canvas copy of a list) in the list and opens it. */
  onAdopt: (scenario: Scenario) => void;
  /** The list of scenarios beside a canvas is hidden to give it more room. */
  listHidden: boolean;
  onToggleList: () => void;
}

/** Editor of one scenario; mount it with `key={scenarioId}`. */
export function ScenarioEditor({ scenarioId, revision, onAdopt, listHidden, onToggleList }: ScenarioEditorProps) {
  const { t } = useT();
  const { draft, failed, issues, change, peek, reload } = useScenarioDraft(scenarioId);

  const seenRevision = useRef(revision);
  useEffect(() => {
    if (seenRevision.current !== revision) {
      seenRevision.current = revision;
      reload();
    }
  }, [revision, reload]);

  const mode = draft?.mode;
  const target = draft?.target;
  const env = useMemo<EditorEnv | null>(
    () => (mode && target ? { scenarioId, mode, target } : null),
    [scenarioId, mode, target],
  );

  if (!draft || !env) {
    return <div className="editor-status">{failed ? t("editor.loadFailed") : t("common.loading")}</div>;
  }
  return (
    <EditorEnvContext.Provider value={env}>
      {draft.graph ? (
        <GraphEditor
          draft={draft}
          graph={draft.graph}
          issues={issues}
          change={change}
          reload={reload}
          listHidden={listHidden}
          onToggleList={onToggleList}
        />
      ) : (
        <ListEditor draft={draft} issues={issues} change={change} peek={peek} reload={reload} onAdopt={onAdopt} />
      )}
    </EditorEnvContext.Provider>
  );
}

function sameIssueList(a: ReadonlyArray<Issue>, b: ReadonlyArray<Issue>): boolean {
  return (
    a.length === b.length &&
    a.every((issue, index) => issue.code === b[index].code && issue.severity === b[index].severity)
  );
}

/** Splits issues into the ones of the scenario and the ones of each step or block. */
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

/** The banner of a run in progress and the issues of the scenario as a whole. */
function EditorNotices({ general, locked }: { general: ReadonlyArray<Issue>; locked: boolean }) {
  const { t, td } = useT();
  return (
    <>
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
    </>
  );
}

interface ScenarioSettingsProps {
  draft: Scenario;
  change: (fn: (scenario: Scenario) => Scenario) => void;
  locked: boolean;
}

/** Name, mode, window and repeat: the same for a list and for a canvas. */
function ScenarioSettings({ draft, change, locked }: ScenarioSettingsProps) {
  const { t } = useT();
  const pause = draft.pauseBetweenRounds;
  const showRoundPause = draft.repeat.kind !== "once" || pause.fromMs !== 0 || pause.toMs !== 0;
  return (
    <>
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
    </>
  );
}

interface ListEditorProps {
  draft: Scenario;
  issues: ReadonlyArray<Issue>;
  change: (fn: (scenario: Scenario) => Scenario) => void;
  peek: () => Scenario | null;
  reload: () => void;
  onAdopt: (scenario: Scenario) => void;
}

function ListEditor({ draft, issues, change, peek, reload, onAdopt }: ListEditorProps) {
  const { t } = useT();
  const { locked, start, flushAll, showError, notify, confirm, touch } = useApp();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [converting, setConverting] = useState(false);
  const selectedRef = useRef(selectedId);
  selectedRef.current = selectedId;

  const { general, byStep } = useGroupedIssues(issues);
  const total = useMemo(() => countSteps(draft.steps), [draft.steps]);
  const full = total >= MAX_STEPS;

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

  /** Turns the list into a graph: this scenario, or a new one next to it. */
  const convert = async (copy: boolean) => {
    setConverting(false);
    try {
      // The core converts what is on disk.
      await flushAll();
      if (copy) {
        const created = await api.scenarioToCanvas(draft.id, t("scenarios.canvasCopyName", { name: draft.name }));
        onAdopt(created);
        notify(t("editor.convertedCopy", { name: created.name }));
      } else {
        await api.scenarioToCanvas(draft.id);
        touch("scenarios");
        reload();
      }
    } catch (error) {
      showError(error);
    }
  };

  return (
    <div className="editor">
      <EditorNotices general={general} locked={locked} />
      <ScenarioSettings draft={draft} change={change} locked={locked} />
      <Section title={t("editor.runSection")}>
        <div className="editor-run">
          <Button variant="primary" disabled={locked} onClick={() => void start({ kind: "scenario", id: draft.id })}>
            {t("editor.start")}
          </Button>
          <Button disabled={locked} onClick={() => void record()}>
            {t("record.start")}
          </Button>
          <Button disabled={locked} onClick={() => setConverting(true)}>
            {t("editor.openCanvas")}
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
      {converting ? (
        <Modal
          title={t("editor.openCanvas")}
          onClose={() => setConverting(false)}
          footer={
            <>
              <Button onClick={() => setConverting(false)}>{t("common.cancel")}</Button>
              <Button onClick={() => void convert(true)}>{t("editor.convertCopy")}</Button>
              <Button variant="primary" onClick={() => void convert(false)}>
                {t("editor.convertHere")}
              </Button>
            </>
          }
        >
          <p className="modal-text">{t("editor.convertText")}</p>
        </Modal>
      ) : null}
    </div>
  );
}

/** A group that is closed until its title is clicked; the canvas below gets the room. */
function Fold({ title, children }: { title: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="editor-fold">
      <button type="button" className="section-toggle editor-fold-toggle" aria-expanded={open} onClick={() => setOpen((now) => !now)}>
        <span className={open ? "section-mark section-mark-open" : "section-mark"} aria-hidden="true" />
        <span className="editor-fold-title">{title}</span>
      </button>
      {open ? <div className="editor-fold-body">{children}</div> : null}
    </div>
  );
}

interface GraphEditorProps {
  draft: Scenario;
  graph: Graph;
  issues: ReadonlyArray<Issue>;
  change: (fn: (scenario: Scenario) => Scenario) => void;
  reload: () => void;
  listHidden: boolean;
  onToggleList: () => void;
}

const NO_BREAKPOINTS: ReadonlySet<string> = new Set();

function GraphEditor({ draft, graph, issues, change, reload, listHidden, onToggleList }: GraphEditorProps) {
  const { t } = useT();
  const { locked, start, settings, flushAll, showError, confirm, touch, dataVersion } = useApp();
  const { general, byStep } = useGroupedIssues(issues);
  // Breakpoints live as long as the editor is open and belong to the runs started from it.
  const [breakpoints, setBreakpoints] = useState<ReadonlySet<string>>(NO_BREAKPOINTS);
  /** The list this graph was made from is still there. */
  const [hasBackup, setHasBackup] = useState(false);

  useEffect(() => {
    let cancelled = false;
    api
      .scenarioMeta(draft.id)
      .then((meta) => {
        if (!cancelled) {
          setHasBackup(meta.hasBackup);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setHasBackup(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [draft.id, dataVersion.scenarios]);

  const toggleBreakpoint = (id: string) =>
    setBreakpoints((current) => {
      const next = new Set(current);
      if (!next.delete(id)) {
        next.add(id);
      }
      return next;
    });

  const run = (options: { startNode?: string; step: boolean }) => {
    const live = graph.nodes.filter((node) => breakpoints.has(node.id)).map((node) => node.id);
    const runOptions: RunOptions = {};
    if (options.startNode !== undefined) {
      runOptions.startNode = options.startNode;
    }
    if (options.step || live.length > 0) {
      runOptions.debug = { breakpoints: live, step: options.step };
    }
    void start({ kind: "scenario", id: draft.id }, runOptions);
  };

  const record = async () => {
    try {
      await flushAll();
      await api.recordStart(draft.id);
    } catch (error) {
      showError(error);
    }
  };

  const returnList = async () => {
    const ok = await confirm({
      title: t("editor.returnListTitle"),
      text: t("editor.returnListText"),
      okLabel: t("editor.returnList"),
      danger: true,
    });
    if (!ok) {
      return;
    }
    try {
      // Nothing may be left to save: a late save of the graph would be refused.
      await flushAll();
      await api.scenarioToList(draft.id);
      touch("scenarios");
      reload();
    } catch (error) {
      showError(error);
    }
  };

  return (
    <div className="editor editor-canvas">
      <EditorNotices general={general} locked={locked} />
      <Fold title={`${t("editor.settingsSection")}: ${draft.name}`}>
        <ScenarioSettings draft={draft} change={change} locked={locked} />
      </Fold>
      <div className="editor-run editor-run-canvas">
        <Button variant="primary" disabled={locked} onClick={() => run({ step: false })}>
          {t("editor.start")}
        </Button>
        <Button
          disabled={locked}
          title={t("editor.startDebugTitle", { step: settings.hotkeyStep, go: settings.hotkeyContinue })}
          onClick={() => run({ step: true })}
        >
          {t("editor.startDebug")}
        </Button>
        <Button disabled={locked} title={t("editor.recordCanvasHint")} onClick={() => void record()}>
          {t("record.start")}
        </Button>
        {hasBackup ? (
          <Button disabled={locked} onClick={() => void returnList()}>
            {t("editor.returnList")}
          </Button>
        ) : null}
      </div>
      <CanvasEditor
        draft={draft}
        graph={graph}
        change={change}
        reload={reload}
        issues={byStep}
        locked={locked}
        breakpoints={breakpoints}
        onToggleBreakpoint={toggleBreakpoint}
        onRunFrom={(id) => run({ startNode: id, step: false })}
        listHidden={listHidden}
        onToggleList={onToggleList}
      />
    </div>
  );
}
