import { useT } from "../../i18n";
import type { Condition, ConditionKind, ScenarioMode, TitleMatch, WindowTarget } from "../../types/scenario";
import { Field, NumberInput, Select, TextInput, type Option } from "../../ui";
import { CONDITION_KINDS, switchCondition } from "./defaults";
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
}

const COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;

/** Condition of a "Wait", "If" or "Repeat while" step. */
export function ConditionEditor({ value, onChange, mode, target, scenarioId, disabled }: ConditionEditorProps) {
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
      default: {
        const unreachable: never = value;
        throw new Error(`Unknown condition: ${String(unreachable)}`);
      }
    }
  };

  return (
    <>
      <Field label={t("conditions.kind")}>
        <Select
          value={value.kind}
          options={kindOptions}
          disabled={disabled}
          onChange={(kind) => onChange(switchCondition(value, kind, mode))}
        />
      </Field>
      {fields()}
    </>
  );
}
