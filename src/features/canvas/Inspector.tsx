import { useState } from "react";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { Issue } from "../../types/runtime";
import {
  MAX_STEP_REPEAT,
  MIN_INTERCEPT_MS,
  type FocusNode,
  type GraphNode,
  type GraphNote,
  type WindowTarget,
  type IfStep,
  type ScenarioSummary,
  type Step,
  type WaitStep,
} from "../../types/scenario";
import { Button, Checkbox, Field, NumberInput, PauseInput, Segmented, Select, TextInput, cx, type Option } from "../../ui";
import { ConditionEditor } from "../editor/ConditionEditor";
import { useEditorEnv } from "../editor/editorContext";
import { WindowPicker } from "../editor/WindowPicker";
import { RepeatFields, StepOwnFields } from "../editor/StepForm";
import { portLabel, typeName } from "./blockText";
import { NOTE_COLORS } from "./canvasContext";
import { BlockIcon } from "./icons";
import { useNodeLive, type TraceStore } from "./useRunTrace";

/** Below this an intercept looks at the screen so often that the scenario slows down. */
const INTERCEPT_SLOW_MS = 1000;

interface BlockFieldsProps {
  node: GraphNode;
  onChange: (node: GraphNode) => void;
  disabled: boolean;
  scenarios: ReadonlyArray<ScenarioSummary>;
}

function FocusFields({
  node,
  disabled,
  onChange,
}: {
  node: FocusNode;
  disabled: boolean;
  onChange: (node: FocusNode) => void;
}) {
  const { t } = useT();
  const [picking, setPicking] = useState(false);
  const target = node.target;
  const setTarget = (patch: Partial<WindowTarget>) => onChange({ ...node, target: { ...target, ...patch } });
  const matchOptions: Array<Option<WindowTarget["titleMatch"]>> = [
    { value: "contains", label: t("editor.titleContains") },
    { value: "exact", label: t("editor.titleExact") },
  ];
  return (
    <>
      <p className="field-hint">{t("canvas.inspector.focusHint")}</p>
      <Field label={t("editor.targetTitle")}>
        <TextInput
          className="inspector-wide"
          value={target.title}
          disabled={disabled}
          onChange={(title) => setTarget({ title })}
        />
        <Select
          value={target.titleMatch}
          options={matchOptions}
          disabled={disabled}
          onChange={(titleMatch) => setTarget({ titleMatch })}
        />
      </Field>
      <Field label={t("editor.targetProcess")} hint={t("editor.targetProcessHint")}>
        <TextInput
          className="inspector-wide"
          value={target.process}
          spellCheck={false}
          disabled={disabled}
          onChange={(process) => setTarget({ process })}
        />
        <Button disabled={disabled} onClick={() => setPicking(true)}>
          {t("editor.pickWindow")}
        </Button>
      </Field>
      {picking ? (
        <WindowPicker
          onClose={() => setPicking(false)}
          onPick={(window) => {
            setTarget({ title: window.title, process: window.process });
            setPicking(false);
          }}
        />
      ) : null}
    </>
  );
}

/** The fields that belong to the type of the block. */
function BlockFields({ node, onChange, disabled, scenarios }: BlockFieldsProps) {
  const { t } = useT();
  const env = useEditorEnv();
  const { selectScenario } = useApp();

  switch (node.type) {
    case "start":
      return <p className="step-form-info">{t("canvas.inspector.startInfo")}</p>;
    case "click":
    case "move":
    case "drag":
    case "scroll":
    case "key":
    case "text":
    case "pause":
    case "stop": {
      // An action block has the fields of a step; the forms keep what else the block carries.
      const step: Step = node;
      return <StepOwnFields step={step} disabled={disabled} onChange={(next) => onChange(next as GraphNode)} />;
    }
    case "wait": {
      const step: WaitStep = { ...node, onTimeout: "stop" };
      return (
        <>
          <StepOwnFields
            block
            step={step}
            disabled={disabled}
            onChange={(next) => {
              if (next.type === "wait") {
                const { onTimeout: _onTimeout, ...rest } = next;
                onChange(rest);
              }
            }}
          />
          <p className="field-hint">{t("canvas.inspector.waitHint")}</p>
        </>
      );
    }
    case "branch": {
      const step: IfStep = { ...node, type: "if", thenSteps: [], elseSteps: [] };
      return (
        <StepOwnFields
          step={step}
          disabled={disabled}
          timeoutHint={t("canvas.inspector.branchTimeoutHint")}
          onChange={(next) => {
            if (next.type === "if") {
              const { thenSteps: _thenSteps, elseSteps: _elseSteps, ...rest } = next;
              onChange({ ...rest, type: "branch" });
            }
          }}
        />
      );
    }
    case "while":
      return (
        <>
          <Field label={t("canvas.inspector.whileModeLabel")}>
            <Segmented<"while" | "until">
              value={node.until ? "until" : "while"}
              disabled={disabled}
              options={[
                { value: "while", label: t("canvas.inspector.whileMode") },
                { value: "until", label: t("canvas.inspector.untilMode") },
              ]}
              onChange={(mode) => onChange({ ...node, until: mode === "until" })}
            />
          </Field>
          <p className="field-hint">{t("canvas.inspector.whileHint")}</p>
          <ConditionEditor
            value={node.condition}
            mode={env.mode}
            target={env.target}
            scenarioId={env.scenarioId}
            disabled={disabled}
            onChange={(condition) => onChange({ ...node, condition })}
          />
          <Field label={t("canvas.inspector.counterPause")}>
            <PauseInput
              value={node.pauseBetween}
              disabled={disabled}
              onChange={(pauseBetween) => onChange({ ...node, pauseBetween })}
            />
          </Field>
        </>
      );
    case "focus":
      return <FocusFields node={node} disabled={disabled} onChange={onChange} />;
    case "counter":
      return (
        <>
          <Field label={t("canvas.inspector.counterTimes")} hint={t("canvas.inspector.counterHint")}>
            <NumberInput
              value={node.times}
              min={1}
              max={MAX_STEP_REPEAT}
              suffix={t("common.times")}
              disabled={disabled}
              invalid={node.times < 1 || node.times > MAX_STEP_REPEAT}
              onChange={(times) => onChange({ ...node, times })}
            />
          </Field>
          <Field label={t("canvas.inspector.counterPause")}>
            <PauseInput
              value={node.pauseBetween}
              disabled={disabled}
              onChange={(pauseBetween) => onChange({ ...node, pauseBetween })}
            />
          </Field>
        </>
      );
    case "call": {
      const options: Array<Option<string>> = [
        { value: "", label: t("canvas.inspector.callNone") },
        ...scenarios.filter((item) => item.id !== env.scenarioId).map((item) => ({ value: item.id, label: item.name })),
      ];
      const known = node.scenarioId === "" || options.some((option) => option.value === node.scenarioId);
      if (!known) {
        options.push({ value: node.scenarioId, label: t("canvas.summary.callGone"), disabled: true });
      }
      return (
        <>
          <Field label={t("canvas.inspector.callScenario")} hint={t("canvas.inspector.callHint")}>
            <Select
              value={node.scenarioId}
              options={options}
              disabled={disabled}
              invalid={node.scenarioId === "" || !known}
              onChange={(scenarioId) => onChange({ ...node, scenarioId })}
            />
            <Button disabled={node.scenarioId === "" || !known} onClick={() => selectScenario(node.scenarioId)}>
              {t("common.open")}
            </Button>
          </Field>
          <RepeatFields block step={node} disabled={disabled} onChange={onChange} />
        </>
      );
    }
    case "intercept":
      return (
        <>
          <ConditionEditor
            value={node.condition}
            mode={env.mode}
            target={env.target}
            scenarioId={env.scenarioId}
            disabled={disabled}
            onChange={(condition) => onChange({ ...node, condition })}
          />
          <Field
            label={t("canvas.inspector.interceptInterval")}
            hint={t("canvas.inspector.interceptHint")}
            error={
              node.minIntervalMs >= MIN_INTERCEPT_MS && node.minIntervalMs < INTERCEPT_SLOW_MS
                ? t("canvas.inspector.interceptFast")
                : undefined
            }
          >
            <NumberInput
              value={node.minIntervalMs}
              min={MIN_INTERCEPT_MS}
              wide
              suffix={t("common.ms")}
              disabled={disabled}
              invalid={node.minIntervalMs < MIN_INTERCEPT_MS}
              onChange={(minIntervalMs) => onChange({ ...node, minIntervalMs })}
            />
          </Field>
        </>
      );
    default: {
      const unreachable: never = node;
      throw new Error(`Unknown block: ${String(unreachable)}`);
    }
  }
}

/** The name is stored only when the user gave one. */
function withTitle(node: GraphNode, title: string): GraphNode {
  const next = { ...node };
  if (title.trim() === "") {
    delete next.title;
  } else {
    next.title = title;
  }
  return next;
}

function isRepeatableNode(node: GraphNode): node is Extract<GraphNode, { type: "click" | "move" | "drag" | "scroll" | "key" | "text" | "pause" }> {
  return (
    node.type === "click" ||
    node.type === "move" ||
    node.type === "drag" ||
    node.type === "scroll" ||
    node.type === "key" ||
    node.type === "text" ||
    node.type === "pause"
  );
}

interface BlockPanelProps {
  node: GraphNode;
  number: number;
  issues: ReadonlyArray<Issue>;
  scenarios: ReadonlyArray<ScenarioSummary>;
  locked: boolean;
  store: TraceStore;
  /** False for the only start block of the graph. */
  removable: boolean;
  onChange: (node: GraphNode) => void;
  onRunFrom: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onClose: () => void;
}

function BlockPanel({
  node,
  number,
  issues,
  scenarios,
  locked,
  store,
  removable,
  onChange,
  onRunFrom,
  onDuplicate,
  onDelete,
  onClose,
}: BlockPanelProps) {
  const translator = useT();
  const { t, td } = translator;
  const live = useNodeLive(store, node.id);
  const stop = store.stopOf(node.id);

  return (
    <aside className="canvas-inspector" aria-label={typeName(node.type, translator)}>
      <div className="inspector-head">
        <BlockIcon type={node.type} />
        <span className="inspector-type">{typeName(node.type, translator)}</span>
        <span className="block-number">{number}</span>
        <button
          type="button"
          className="inspector-close"
          title={t("canvas.inspector.close")}
          aria-label={t("canvas.inspector.close")}
          onClick={onClose}
        >
          ×
        </button>
      </div>
      <div className="inspector-body">
        {issues.length > 0 ? (
          <ul className="step-issues inspector-issues">
            {issues.map((issue) => (
              <li key={issue.code} className={cx("issue", `issue-${issue.severity}`)}>
                {td(`issues.${issue.code}`)}
              </li>
            ))}
          </ul>
        ) : null}
        <Field label={t("canvas.inspector.title")}>
          <TextInput
            className="inspector-wide"
            value={node.title ?? ""}
            maxLength={60}
            placeholder={t("canvas.inspector.titlePlaceholder")}
            disabled={locked}
            onChange={(title) => onChange(withTitle(node, title))}
          />
        </Field>
        {node.type === "start" ? null : (
          <div className="inspector-check">
            <Checkbox
              checked={node.enabled}
              disabled={locked}
              label={t("canvas.inspector.enabled")}
              onChange={(enabled) => onChange({ ...node, enabled })}
            />
          </div>
        )}
        <BlockFields node={node} onChange={onChange} disabled={locked} scenarios={scenarios} />
        {isRepeatableNode(node) ? <RepeatFields block step={node} disabled={locked} onChange={onChange} /> : null}
        {node.type === "start" || node.type === "stop" ? null : (
          <Field label={t("steps.fields.pauseAfterBlock")}>
            <PauseInput
              value={node.pauseAfter}
              disabled={locked}
              onChange={(pauseAfter) => onChange({ ...node, pauseAfter })}
            />
          </Field>
        )}
        {live.passes > 0 || stop !== null ? (
          <div className="inspector-trace">
            {live.passes > 0 ? <div>{t("canvas.inspector.lastPasses", { count: live.passes })}</div> : null}
            {live.outcome !== null ? (
              <div>{t("canvas.inspector.lastPort", { port: portLabel(node.type, live.outcome, translator) })}</div>
            ) : null}
            {stop !== null ? (
              <div className={cx(stop.outcome === "failed" && "inspector-trace-failed")}>
                {stop.outcome === "completed"
                  ? t("canvas.block.endedCompleted")
                  : stop.outcome === "stopped"
                    ? t("canvas.block.endedStopped")
                    : t("canvas.block.endedFailed")}
                {stop.reason !== null ? `: ${translator.error(stop.reason)}` : ""}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
      <div className="inspector-actions">
        {node.type === "intercept" ? null : (
          <Button small disabled={locked} title={t("canvas.inspector.runFromHint")} onClick={onRunFrom}>
            {t("canvas.inspector.runFrom")}
          </Button>
        )}
        {node.type === "start" ? null : (
          <Button small disabled={locked} onClick={onDuplicate}>
            {t("common.duplicate")}
          </Button>
        )}
        <Button small disabled={locked || !removable} onClick={onDelete}>
          {t("common.delete")}
        </Button>
      </div>
    </aside>
  );
}

interface NotePanelProps {
  note: GraphNote;
  locked: boolean;
  onChange: (note: GraphNote) => void;
  onDelete: () => void;
  onClose: () => void;
}

function withColor(note: GraphNote, color: string | null): GraphNote {
  const next = { ...note };
  if (color === null) {
    delete next.color;
  } else {
    next.color = color;
  }
  return next;
}

function NotePanel({ note, locked, onChange, onDelete, onClose }: NotePanelProps) {
  const { t } = useT();
  const frame = note.kind === "frame";
  return (
    <aside className="canvas-inspector" aria-label={frame ? t("canvas.toolbar.frame") : t("canvas.toolbar.note")}>
      <div className="inspector-head">
        <span className="inspector-type">{frame ? t("canvas.toolbar.frame") : t("canvas.toolbar.note")}</span>
        <button
          type="button"
          className="inspector-close"
          title={t("canvas.inspector.close")}
          aria-label={t("canvas.inspector.close")}
          onClick={onClose}
        >
          ×
        </button>
      </div>
      <div className="inspector-body">
        {frame ? (
          <Field label={t("canvas.inspector.frameTitle")}>
            <TextInput
              className="inspector-wide"
              value={note.text}
              maxLength={80}
              placeholder={t("canvas.frameDefault")}
              disabled={locked}
              onChange={(text) => onChange({ ...note, text })}
            />
          </Field>
        ) : (
          <Field label={t("canvas.inspector.noteText")}>
            <textarea
              className="input step-textarea"
              value={note.text}
              maxLength={2000}
              placeholder={t("canvas.noteDefault")}
              disabled={locked}
              onChange={(event) => onChange({ ...note, text: event.target.value })}
            />
          </Field>
        )}
        <Field label={t("canvas.inspector.color")}>
          <span className="inspector-colors" role="radiogroup">
            <button
              type="button"
              role="radio"
              aria-checked={note.color === undefined}
              className={cx("inspector-color", "inspector-color-none", note.color === undefined && "inspector-color-on")}
              title={t("canvas.inspector.colorNone")}
              aria-label={t("canvas.inspector.colorNone")}
              disabled={locked}
              onClick={() => onChange(withColor(note, null))}
            />
            {NOTE_COLORS.map((color) => (
              <button
                key={color.name}
                type="button"
                role="radio"
                aria-checked={note.color === color.name}
                aria-label={color.name}
                className={cx("inspector-color", note.color === color.name && "inspector-color-on")}
                style={{ background: color.css }}
                disabled={locked}
                onClick={() => onChange(withColor(note, color.name))}
              />
            ))}
          </span>
        </Field>
        {frame ? (
          <div className="inspector-check">
            <Checkbox
              checked={note.collapsed}
              disabled={locked}
              label={t("canvas.inspector.collapsed")}
              onChange={(collapsed) => onChange({ ...note, collapsed })}
            />
          </div>
        ) : null}
      </div>
      <div className="inspector-actions">
        <Button small disabled={locked} onClick={onDelete}>
          {t("common.delete")}
        </Button>
      </div>
    </aside>
  );
}

export type InspectorTarget =
  | { kind: "node"; node: GraphNode; number: number; removable: boolean }
  | { kind: "note"; note: GraphNote };

interface InspectorProps {
  target: InspectorTarget;
  issues: ReadonlyArray<Issue>;
  scenarios: ReadonlyArray<ScenarioSummary>;
  locked: boolean;
  store: TraceStore;
  onNode: (node: GraphNode) => void;
  onNote: (note: GraphNote) => void;
  onRunFrom: (id: string) => void;
  onDuplicate: (id: string) => void;
  onDelete: (id: string) => void;
  onClose: () => void;
}

/** The panel of the selected block or note, at the right side of the canvas. */
export function Inspector({
  target,
  issues,
  scenarios,
  locked,
  store,
  onNode,
  onNote,
  onRunFrom,
  onDuplicate,
  onDelete,
  onClose,
}: InspectorProps) {
  if (target.kind === "note") {
    const { note } = target;
    return (
      <NotePanel key={note.id} note={note} locked={locked} onChange={onNote} onDelete={() => onDelete(note.id)} onClose={onClose} />
    );
  }
  const { node } = target;
  return (
    <BlockPanel
      key={node.id}
      node={node}
      number={target.number}
      issues={issues}
      scenarios={scenarios}
      locked={locked}
      store={store}
      removable={target.removable}
      onChange={onNode}
      onRunFrom={() => onRunFrom(node.id)}
      onDuplicate={() => onDuplicate(node.id)}
      onDelete={() => onDelete(node.id)}
      onClose={onClose}
    />
  );
}
