import { useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { CheckResult } from "../../types/runtime";
import type { Condition, ConditionKind, ScenarioMode, TitleMatch, WindowTarget } from "../../types/scenario";
import { Button, Field, NumberInput, Segmented, Select, TextInput, cx, type Option } from "../../ui";
import { CONDITION_KINDS, defaultCondition, switchCondition } from "./defaults";
import { ImageConditionFields } from "./ImageConditionFields";
import { PositionInput } from "./PositionInput";
import { CONDITION_KIND_KEYS } from "./summary";

interface ConditionEditorProps {
  value: Condition;
  onChange: (value: Condition) => void;
  mode: ScenarioMode;
  target: WindowTarget;
  scenarioId: string;
  disabled?: boolean;
  /** A check inside "and" / "or". It stays one check and is not tried on its own. */
  nested?: boolean;
}

const COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;

/** What "Check now" has to say; the answer belongs to the condition it was asked about. */
type Check = { state: "idle" } | { state: "running" } | { state: "done"; about: Condition; result: CheckResult };

interface CheckNowProps {
  value: Condition;
  mode: ScenarioMode;
  target: WindowTarget;
  scenarioId: string;
}

/** One look at the condition, the way a run does it. Nothing is clicked. */
function CheckNow({ value, mode, target, scenarioId }: CheckNowProps) {
  const { t } = useT();
  const { locked, showError } = useApp();
  const [check, setCheck] = useState<Check>({ state: "idle" });

  const run = async () => {
    setCheck({ state: "running" });
    try {
      const result = await api.conditionCheck(scenarioId, mode, target, value);
      setCheck({ state: "done", about: value, result });
    } catch (error) {
      setCheck({ state: "idle" });
      showError(error);
    }
  };

  // An answer about a condition that was edited since says nothing about this one.
  const result = check.state === "done" && check.about === value ? check.result : null;
  const lines: string[] = [];
  if (result) {
    const percent = (score: number) => Math.round(score * 100);
    if (result.found === true && result.x !== null && result.y !== null) {
      const inWindow = mode === "window" && result.clientX !== null && result.clientY !== null;
      lines.push(
        t("conditions.checkFound", {
          x: inWindow && result.clientX !== null ? result.clientX : result.x,
          y: inWindow && result.clientY !== null ? result.clientY : result.y,
          score: result.score === null ? "—" : percent(result.score),
        }),
      );
    } else if (result.found === false) {
      lines.push(t("conditions.checkNotFound"));
      if (result.wouldMatchAt !== null) {
        lines.push(t("conditions.checkWould", { score: percent(result.wouldMatchAt) }));
      }
    }
    if (result.color !== null) {
      lines.push(t("conditions.checkColor", { color: result.color }));
    }
    lines.push(t("conditions.checkTime", { ms: result.ms }));
  }

  return (
    <Field label="" hint={result ? undefined : t("conditions.checkHint")}>
      <Button disabled={locked || check.state === "running"} onClick={() => void run()}>
        {check.state === "running" ? t("conditions.checkRunning") : t("conditions.checkNow")}
      </Button>
      {result ? (
        <div className="condition-check" role="status">
          <span className={cx("condition-check-verdict", result.holds ? "condition-check-yes" : "condition-check-no")}>
            {result.holds ? t("conditions.checkHolds") : t("conditions.checkFails")}
          </span>
          {lines.map((line) => (
            <span key={line}>{line}</span>
          ))}
        </div>
      ) : null}
    </Field>
  );
}

type ConditionShape = "one" | "all" | "any";

/** Condition of a "Wait", "If", "While" or "Repeat while" step. */
export function ConditionEditor({ value, onChange, mode, target, scenarioId, disabled, nested }: ConditionEditorProps) {
  const { t } = useT();

  const kindOptions: Array<Option<ConditionKind>> = CONDITION_KINDS.map((kind) => ({
    value: kind,
    label: t(CONDITION_KIND_KEYS[kind]),
  }));
  const matchOptions: Array<Option<TitleMatch>> = [
    { value: "contains", label: t("editor.titleContains") },
    { value: "exact", label: t("editor.titleExact") },
  ];

  const fields = () => {
    switch (value.kind) {
      case "pixel": {
        const colorValid = COLOR_PATTERN.test(value.color);
        return (
          <>
            <Field label={t("conditions.point")}>
              <PositionInput
                value={value.point}
                mode={mode}
                target={target}
                allowCursor={false}
                disabled={disabled}
                onChange={(point) => onChange({ ...value, point })}
                onCapture={(point, result) => onChange({ ...value, point, color: result.color.toUpperCase() })}
              />
            </Field>
            <Field label={t("conditions.color")} hint={t("conditions.colorHint")}>
              <TextInput
                className="color-input"
                value={value.color}
                maxLength={7}
                spellCheck={false}
                disabled={disabled}
                invalid={!colorValid}
                onChange={(color) => onChange({ ...value, color: color.trim() })}
              />
              <span
                className="color-swatch"
                style={colorValid ? { background: value.color } : undefined}
                aria-hidden="true"
              />
            </Field>
            <Field label={t("conditions.tolerance")} hint={t("conditions.toleranceHint")}>
              <NumberInput
                value={value.tolerance}
                min={0}
                max={255}
                disabled={disabled}
                onChange={(tolerance) => onChange({ ...value, tolerance })}
              />
            </Field>
          </>
        );
      }
      case "imageFound":
      case "imageGone":
        return (
          <ImageConditionFields
            value={value}
            onChange={onChange}
            mode={mode}
            target={target}
            scenarioId={scenarioId}
            disabled={disabled}
          />
        );
      case "window":
        return (
          <Field label={t("conditions.windowTitle")}>
            <TextInput
              className="editor-wide-input"
              value={value.title}
              disabled={disabled}
              onChange={(title) => onChange({ ...value, title })}
            />
            <Select
              value={value.titleMatch}
              options={matchOptions}
              disabled={disabled}
              onChange={(titleMatch) => onChange({ ...value, titleMatch })}
            />
          </Field>
        );
      case "process":
        return (
          <Field label={t("conditions.processName")} hint={t("conditions.processHint")}>
            <TextInput
              className="editor-wide-input"
              value={value.name}
              spellCheck={false}
              disabled={disabled}
              onChange={(name) => onChange({ ...value, name })}
            />
          </Field>
        );
      case "all":
      case "any":
        return null;
      default: {
        const unreachable: never = value;
        throw new Error(`Unknown condition: ${String(unreachable)}`);
      }
    }
  };

  const shape: ConditionShape = value.kind === "all" || value.kind === "any" ? value.kind : "one";
  const setShape = (next: ConditionShape) => {
    if (next === shape) {
      return;
    }
    if (next === "one") {
      const first = value.kind === "all" || value.kind === "any" ? value.items[0] : undefined;
      onChange(first !== undefined && first.kind !== "all" && first.kind !== "any" ? first : defaultCondition("imageFound", mode));
      return;
    }
    if (value.kind === "all" || value.kind === "any") {
      onChange({ ...value, kind: next });
      return;
    }
    onChange({ kind: next, items: [value, defaultCondition("imageFound", mode)] });
  };
  const shapeField =
    nested && shape === "one" ? null : (
      <Field label={nested ? undefined : t("conditions.shape")} hint={nested ? undefined : t("conditions.shapeHint")}>
        <Segmented<ConditionShape>
          value={shape}
          disabled={disabled}
          options={[
            { value: "one", label: t("conditions.shapeOne") },
            { value: "all", label: t("conditions.shapeAll") },
            { value: "any", label: t("conditions.shapeAny") },
          ]}
          onChange={setShape}
        />
      </Field>
    );

  if (value.kind === "all" || value.kind === "any") {
    const kind = value.kind;
    const first = value.items[0] ?? defaultCondition("imageFound", mode);
    const second = value.items[1] ?? defaultCondition("imageFound", mode);
    return (
      <>
        {shapeField}
        <div className="condition-combine">
          <ConditionEditor
            nested
            value={first}
            mode={mode}
            target={target}
            scenarioId={scenarioId}
            disabled={disabled}
            onChange={(item) => onChange({ kind, items: [item, second] })}
          />
          <ConditionEditor
            nested
            value={second}
            mode={mode}
            target={target}
            scenarioId={scenarioId}
            disabled={disabled}
            onChange={(item) => onChange({ kind, items: [first, item] })}
          />
        </div>
        {nested ? null : <CheckNow value={value} mode={mode} target={target} scenarioId={scenarioId} />}
      </>
    );
  }

  return (
    <>
      {shapeField}
      <Field label={t("conditions.kind")}>
        <Select
          value={value.kind}
          options={kindOptions}
          disabled={disabled}
          onChange={(kind) => onChange(switchCondition(value, kind, mode))}
        />
      </Field>
      {fields()}
      {nested ? null : <CheckNow value={value} mode={mode} target={target} scenarioId={scenarioId} />}
    </>
  );
}
