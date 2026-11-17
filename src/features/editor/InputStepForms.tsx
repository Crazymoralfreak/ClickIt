// Forms of the steps that send input or pause: click, move, drag, scroll, key, text, pause.

import { useT, type Translator } from "../../i18n";
import type {
  ClickAction,
  ClickStep,
  DragStep,
  KeyAction,
  KeyStep,
  MouseButton,
  MoveStep,
  PauseStep,
  ScrollDirection,
  ScrollStep,
  Step,
  TextStep,
} from "../../types/scenario";
import { Checkbox, Field, NumberInput, PauseInput, Select, type Option } from "../../ui";
import type { EditorEnv } from "./editorContext";
import { KeyCaptureInput } from "./KeyCaptureInput";
import { PositionInput } from "./PositionInput";
import { BUTTON_KEYS, MODIFIERS } from "./summary";

export interface StepFormProps<S extends Step> {
  step: S;
  onChange: (step: S) => void;
  disabled: boolean;
  env: EditorEnv;
}

const MOUSE_BUTTONS: ReadonlyArray<MouseButton> = ["left", "right", "middle"];

export function mouseButtonOptions({ t }: Translator): Array<Option<MouseButton>> {
  return MOUSE_BUTTONS.map((button) => ({ value: button, label: t(BUTTON_KEYS[button]) }));
}

export function ClickForm({ step, onChange, disabled, env }: StepFormProps<ClickStep>) {
  const translator = useT();
  const { t } = translator;
  const actions: Array<Option<ClickAction>> = [
    { value: "click", label: t("steps.clickActions.click") },
    { value: "double", label: t("steps.clickActions.double") },
    { value: "down", label: t("steps.clickActions.down") },
    { value: "up", label: t("steps.clickActions.up") },
  ];
  return (
    <>
      <Field label={t("steps.fields.button")}>
        <Select
          value={step.button}
          options={mouseButtonOptions(translator)}
          disabled={disabled}
          onChange={(button) => onChange({ ...step, button })}
        />
      </Field>
      <Field label={t("steps.fields.action")}>
        <Select
          value={step.action}
          options={actions}
          disabled={disabled}
          onChange={(action) => onChange({ ...step, action })}
        />
      </Field>
      <Field label={t("steps.fields.position")}>
        <PositionInput
          value={step.position}
          mode={env.mode}
          target={env.target}
          allowCursor
          disabled={disabled}
          onChange={(position) => onChange({ ...step, position })}
        />
      </Field>
    </>
  );
}

export function MoveForm({ step, onChange, disabled, env }: StepFormProps<MoveStep>) {
  const { t } = useT();
  return (
    <Field label={t("steps.fields.position")}>
      <PositionInput
        value={step.position}
        mode={env.mode}
        target={env.target}
        allowCursor={false}
        disabled={disabled}
        onChange={(position) => onChange({ ...step, position })}
      />
    </Field>
  );
}

export function DragForm({ step, onChange, disabled, env }: StepFormProps<DragStep>) {
  const translator = useT();
  const { t } = translator;
  return (
    <>
      <Field label={t("steps.fields.button")}>
        <Select
          value={step.button}
          options={mouseButtonOptions(translator)}
          disabled={disabled}
          onChange={(button) => onChange({ ...step, button })}
        />
      </Field>
      <Field label={t("steps.fields.dragFrom")}>
        <PositionInput
          value={step.from}
          mode={env.mode}
          target={env.target}
          allowCursor
          disabled={disabled}
          onChange={(from) => onChange({ ...step, from })}
        />
      </Field>
      <Field label={t("steps.fields.dragTo")}>
        <PositionInput
          value={step.to}
          mode={env.mode}
          target={env.target}
          allowCursor={false}
          disabled={disabled}
          onChange={(to) => onChange({ ...step, to })}
        />
      </Field>
    </>
  );
}

export function ScrollForm({ step, onChange, disabled, env }: StepFormProps<ScrollStep>) {
  const { t } = useT();
  const directions: Array<Option<ScrollDirection>> = [
    { value: "up", label: t("steps.directions.up") },
    { value: "down", label: t("steps.directions.down") },
  ];
  return (
    <>
      <Field label={t("steps.fields.direction")}>
        <Select
          value={step.direction}
          options={directions}
          disabled={disabled}
          onChange={(direction) => onChange({ ...step, direction })}
        />
      </Field>
      <Field label={t("steps.fields.notches")} hint={t("steps.fields.notchesHint")}>
        <NumberInput
          value={step.notches}
          min={1}
          max={100}
          disabled={disabled}
          invalid={step.notches < 1 || step.notches > 100}
          onChange={(notches) => onChange({ ...step, notches })}
        />
      </Field>
      <Field label={t("steps.fields.position")}>
        <PositionInput
          value={step.position}
          mode={env.mode}
          target={env.target}
          allowCursor
          disabled={disabled}
          onChange={(position) => onChange({ ...step, position })}
        />
      </Field>
    </>
  );
}

export function KeyForm({ step, onChange, disabled }: StepFormProps<KeyStep>) {
  const { t } = useT();
  const actions: Array<Option<KeyAction>> = [
    { value: "press", label: t("steps.keyActions.press") },
    { value: "down", label: t("steps.keyActions.down") },
    { value: "up", label: t("steps.keyActions.up") },
  ];
  return (
    <>
      <Field label={t("steps.fields.key")} hint={t("steps.keyInput.hint")}>
        <KeyCaptureInput
          value={step.key}
          label={t("steps.fields.key")}
          disabled={disabled}
          invalid={step.key === ""}
          onChange={(key) => onChange({ ...step, key })}
        />
      </Field>
      <Field label={t("steps.fields.action")}>
        <Select
          value={step.action}
          options={actions}
          disabled={disabled}
          onChange={(action) => onChange({ ...step, action })}
        />
      </Field>
      <Field label={t("steps.fields.modifiers")}>
        {MODIFIERS.map((modifier) => (
          <Checkbox
            key={modifier.id}
            label={modifier.name}
            checked={step.modifiers[modifier.id]}
            disabled={disabled}
            onChange={(checked) => onChange({ ...step, modifiers: { ...step.modifiers, [modifier.id]: checked } })}
          />
        ))}
      </Field>
    </>
  );
}

export function TextForm({ step, onChange, disabled }: StepFormProps<TextStep>) {
  const { t } = useT();
  return (
    <Field label={t("steps.fields.text")} hint={t("steps.fields.textHint")}>
      <textarea
        className="input step-textarea"
        value={step.text}
        rows={4}
        spellCheck={false}
        disabled={disabled}
        onChange={(event) => onChange({ ...step, text: event.target.value })}
      />
    </Field>
  );
}

export function PauseForm({ step, onChange, disabled }: StepFormProps<PauseStep>) {
  const { t } = useT();
  return (
    <Field label={t("steps.fields.duration")}>
      <PauseInput
        value={step.duration}
        disabled={disabled}
        onChange={(duration) => onChange({ ...step, duration })}
      />
    </Field>
  );
}
