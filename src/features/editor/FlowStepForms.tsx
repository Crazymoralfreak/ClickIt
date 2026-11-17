// Forms of the steps that check conditions or hold other steps: wait, if, loop.

import { useState } from "react";
import { useT } from "../../i18n";
import type { IfStep, LoopMode, LoopStep, OnTimeout, WaitStep } from "../../types/scenario";
import { Field, NumberInput, PauseInput, Select, type Option } from "../../ui";
import { ConditionEditor } from "./ConditionEditor";
import { defaultCondition } from "./defaults";
import { mouseButtonOptions, type StepFormProps } from "./InputStepForms";

const POLL_MIN_MS = 20;
const POLL_MAX_MS = 60_000;

interface TimingFieldsProps {
  pollMs: number;
  timeoutMs: number;
  timeoutHint: string;
  disabled: boolean;
  onPoll: (pollMs: number) => void;
  onTimeout: (timeoutMs: number) => void;
}

function TimingFields({ pollMs, timeoutMs, timeoutHint, disabled, onPoll, onTimeout }: TimingFieldsProps) {
  const { t } = useT();
  return (
    <>
      <Field label={t("steps.fields.pollMs")} hint={t("steps.fields.pollHint")}>
        <NumberInput
          value={pollMs}
          min={POLL_MIN_MS}
          max={POLL_MAX_MS}
          suffix={t("common.ms")}
          disabled={disabled}
          invalid={pollMs < POLL_MIN_MS || pollMs > POLL_MAX_MS}
          onChange={onPoll}
        />
      </Field>
      <Field label={t("steps.fields.timeout")} hint={timeoutHint}>
        <NumberInput
          value={timeoutMs}
          min={0}
          wide
          suffix={t("common.ms")}
          disabled={disabled}
          invalid={timeoutMs < 0}
          onChange={onTimeout}
        />
      </Field>
    </>
  );
}

type FoundChoice = "none" | "center" | "offset";

export function WaitForm({ step, onChange, disabled, env }: StepFormProps<WaitStep>) {
  const translator = useT();
  const { t } = translator;
  const found = step.onFound;
  const hasOffset = found.kind === "click" && (found.offsetX !== 0 || found.offsetY !== 0);
  // "Centre" and "offset 0, 0" are the same data, so the open offset fields are remembered here.
  const [offsetOpen, setOffsetOpen] = useState(hasOffset);

  const button = found.kind === "click" ? found.button : "left";
  const offsetX = found.kind === "click" ? found.offsetX : 0;
  const offsetY = found.kind === "click" ? found.offsetY : 0;
  const choice: FoundChoice = found.kind === "none" ? "none" : offsetOpen || hasOffset ? "offset" : "center";

  const timeoutOptions: Array<Option<OnTimeout>> = [
    { value: "stop", label: t("steps.fields.onTimeoutStop") },
    { value: "continue", label: t("steps.fields.onTimeoutContinue") },
  ];
  const foundOptions: Array<Option<FoundChoice>> = [
    { value: "none", label: t("steps.fields.onFoundNone") },
    { value: "center", label: t("steps.fields.onFoundCenter") },
    { value: "offset", label: t("steps.fields.onFoundOffset") },
  ];

  const setChoice = (next: FoundChoice) => {
    switch (next) {
      case "none":
        setOffsetOpen(false);
        onChange({ ...step, onFound: { kind: "none" } });
        break;
      case "center":
        setOffsetOpen(false);
        onChange({ ...step, onFound: { kind: "click", button, offsetX: 0, offsetY: 0 } });
        break;
      case "offset":
        setOffsetOpen(true);
        onChange({ ...step, onFound: { kind: "click", button, offsetX, offsetY } });
        break;
      default: {
        const unreachable: never = next;
        throw new Error(`Unknown choice: ${String(unreachable)}`);
      }
    }
  };

  return (
    <>
      <ConditionEditor
        value={step.condition}
        mode={env.mode}
        target={env.target}
        scenarioId={env.scenarioId}
        disabled={disabled}
        onChange={(condition) =>
          // Only a found image has a place to click.
          onChange({
            ...step,
            condition,
            onFound: condition.kind === "imageFound" ? step.onFound : { kind: "none" },
          })
        }
      />
      {step.condition.kind === "imageFound" ? (
        <>
          <Field label={t("steps.fields.onFound")}>
            <Select value={choice} options={foundOptions} disabled={disabled} onChange={setChoice} />
            {found.kind === "click" ? (
              <Select
                value={found.button}
                options={mouseButtonOptions(translator)}
                disabled={disabled}
                title={t("steps.fields.button")}
                onChange={(nextButton) => onChange({ ...step, onFound: { ...found, button: nextButton } })}
              />
            ) : null}
          </Field>
          {found.kind === "click" && choice === "offset" ? (
            <Field label={t("steps.fields.offset")} hint={t("steps.fields.offsetHint")}>
              <label className="position-input-axis">
                <span className="pause-input-word">{t("steps.fields.offsetX")}</span>
                <NumberInput
                  value={found.offsetX}
                  disabled={disabled}
                  onChange={(next) => onChange({ ...step, onFound: { ...found, offsetX: next } })}
                />
              </label>
              <label className="position-input-axis">
                <span className="pause-input-word">{t("steps.fields.offsetY")}</span>
                <NumberInput
                  value={found.offsetY}
                  suffix={t("steps.fields.px")}
                  disabled={disabled}
                  onChange={(next) => onChange({ ...step, onFound: { ...found, offsetY: next } })}
                />
              </label>
            </Field>
          ) : null}
        </>
      ) : null}
      <TimingFields
        pollMs={step.pollMs}
        timeoutMs={step.timeoutMs}
        timeoutHint={t("steps.fields.waitTimeoutHint")}
        disabled={disabled}
        onPoll={(pollMs) => onChange({ ...step, pollMs })}
        onTimeout={(timeoutMs) => onChange({ ...step, timeoutMs })}
      />
      <Field label={t("steps.fields.onTimeout")}>
        <Select
          value={step.onTimeout}
          options={timeoutOptions}
          disabled={disabled || step.timeoutMs === 0}
          onChange={(onTimeout) => onChange({ ...step, onTimeout })}
        />
      </Field>
    </>
  );
}

export function IfForm({ step, onChange, disabled, env }: StepFormProps<IfStep>) {
  const { t } = useT();
  return (
    <>
      <ConditionEditor
        value={step.condition}
        mode={env.mode}
        target={env.target}
        scenarioId={env.scenarioId}
        disabled={disabled}
        onChange={(condition) => onChange({ ...step, condition })}
      />
      <TimingFields
        pollMs={step.pollMs}
        timeoutMs={step.timeoutMs}
        timeoutHint={t("steps.fields.ifTimeoutHint")}
        disabled={disabled}
        onPoll={(pollMs) => onChange({ ...step, pollMs })}
        onTimeout={(timeoutMs) => onChange({ ...step, timeoutMs })}
      />
    </>
  );
}

export function LoopForm({ step, onChange, disabled, env }: StepFormProps<LoopStep>) {
  const { t } = useT();
  const mode = step.mode;
  const modeOptions: Array<Option<LoopMode["kind"]>> = [
    { value: "count", label: t("steps.fields.loopCount") },
    { value: "while", label: t("steps.fields.loopWhile") },
    { value: "forever", label: t("steps.fields.loopForever") },
  ];

  const setKind = (kind: LoopMode["kind"]) => {
    if (kind === mode.kind) {
      return;
    }
    switch (kind) {
      case "count":
        onChange({ ...step, mode: { kind: "count", times: 2 } });
        break;
      case "while":
        onChange({ ...step, mode: { kind: "while", condition: defaultCondition("imageFound", env.mode) } });
        break;
      case "forever":
        onChange({ ...step, mode: { kind: "forever" } });
        break;
      default: {
        const unreachable: never = kind;
        throw new Error(`Unknown loop mode: ${String(unreachable)}`);
      }
    }
  };

  return (
    <>
      <Field
        label={t("steps.fields.loopMode")}
        hint={mode.kind === "while" ? t("steps.fields.loopWhileHint") : undefined}
      >
        <Select value={mode.kind} options={modeOptions} disabled={disabled} onChange={setKind} />
        {mode.kind === "count" ? (
          <NumberInput
            value={mode.times}
            min={1}
            suffix={t("common.times")}
            disabled={disabled}
            invalid={mode.times < 1}
            title={t("steps.fields.loopTimes")}
            onChange={(times) => onChange({ ...step, mode: { kind: "count", times } })}
          />
        ) : null}
      </Field>
      {mode.kind === "while" ? (
        <ConditionEditor
          value={mode.condition}
          mode={env.mode}
          target={env.target}
          scenarioId={env.scenarioId}
          disabled={disabled}
          onChange={(condition) => onChange({ ...step, mode: { kind: "while", condition } })}
        />
      ) : null}
      <Field label={t("steps.fields.loopPause")}>
        <PauseInput
          value={step.pauseBetween}
          disabled={disabled}
          onChange={(pauseBetween) => onChange({ ...step, pauseBetween })}
        />
      </Field>
    </>
  );
}
