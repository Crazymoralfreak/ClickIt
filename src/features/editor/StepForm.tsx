import { useT } from "../../i18n";
import { MAX_STEP_REPEAT, type Repeat, type Step } from "../../types/scenario";
import { Field, NumberInput, PauseInput } from "../../ui";
import { isRepeatable, noPause } from "./defaults";
import { useEditorEnv } from "./editorContext";
import { IfForm, LoopForm, WaitForm } from "./FlowStepForms";
import { ClickForm, DragForm, KeyForm, MoveForm, PauseForm, ScrollForm, TextForm } from "./InputStepForms";

interface StepFormProps {
  step: Step;
  onChange: (step: Step) => void;
  disabled: boolean;
}

/** `repeat` is stored only when the step runs more than once. */
function withRepeatTimes<T extends { repeat?: Repeat }>(step: T, times: number): T {
  const next = { ...step };
  if (times === 1) {
    delete next.repeat;
  } else {
    next.repeat = { times, pauseBetween: step.repeat?.pauseBetween ?? noPause() };
  }
  return next;
}

interface RepeatFieldsProps<T extends { repeat?: Repeat }> {
  step: T;
  onChange: (step: T) => void;
  disabled: boolean;
  /** The wording of the canvas: "block" instead of "step". */
  block?: boolean;
}

/** Repeat of a single step of a list or of a block of a graph. */
export function RepeatFields<T extends { repeat?: Repeat }>({ step, onChange, disabled, block }: RepeatFieldsProps<T>) {
  const { t } = useT();
  const repeat = step.repeat;
  const times = repeat?.times ?? 1;
  return (
    <>
      <Field
        label={block ? t("steps.fields.repeatBlock") : t("steps.fields.repeat")}
        hint={block ? t("steps.fields.repeatBlockHint") : t("steps.fields.repeatHint")}
      >
        <NumberInput
          value={times}
          min={1}
          max={MAX_STEP_REPEAT}
          suffix={t("common.times")}
          disabled={disabled}
          invalid={times < 1 || times > MAX_STEP_REPEAT}
          onChange={(next) => onChange(withRepeatTimes(step, next))}
        />
      </Field>
      {repeat && repeat.times > 1 ? (
        <Field label={t("steps.fields.repeatPause")}>
          <PauseInput
            value={repeat.pauseBetween}
            disabled={disabled}
            onChange={(pauseBetween) => onChange({ ...step, repeat: { ...repeat, pauseBetween } })}
          />
        </Field>
      ) : null}
    </>
  );
}

interface StepOwnFieldsProps extends StepFormProps {
  /** A block of a graph: its outputs say what a timeout does, so the form does not ask. */
  block?: boolean;
  /** Replaces the hint under the timeout of an "If" step. */
  timeoutHint?: string;
}

/** The fields that belong to the type of the step, without repeat and pause. */
export function StepOwnFields({ step, onChange, disabled, block, timeoutHint }: StepOwnFieldsProps) {
  const { t } = useT();
  const env = useEditorEnv();
  switch (step.type) {
    case "click":
      return <ClickForm step={step} onChange={onChange} disabled={disabled} env={env} />;
    case "move":
      return <MoveForm step={step} onChange={onChange} disabled={disabled} env={env} />;
    case "drag":
      return <DragForm step={step} onChange={onChange} disabled={disabled} env={env} />;
    case "scroll":
      return <ScrollForm step={step} onChange={onChange} disabled={disabled} env={env} />;
    case "key":
      return <KeyForm step={step} onChange={onChange} disabled={disabled} env={env} />;
    case "text":
      return <TextForm step={step} onChange={onChange} disabled={disabled} env={env} />;
    case "pause":
      return <PauseForm step={step} onChange={onChange} disabled={disabled} env={env} />;
    case "wait":
      return <WaitForm step={step} onChange={onChange} disabled={disabled} env={env} hideOnTimeout={block} />;
    case "if":
      return <IfForm step={step} onChange={onChange} disabled={disabled} env={env} timeoutHint={timeoutHint} />;
    case "loop":
      return <LoopForm step={step} onChange={onChange} disabled={disabled} env={env} />;
    case "breakLoop":
      return <p className="step-form-info">{t("steps.fields.breakLoopInfo")}</p>;
    case "stop":
      return <p className="step-form-info">{t("steps.fields.stopInfo")}</p>;
    default: {
      const unreachable: never = step;
      throw new Error(`Unknown step: ${String(unreachable)}`);
    }
  }
}

/** Settings of one step, shown under its row. */
export function StepForm({ step, onChange, disabled }: StepFormProps) {
  const { t } = useT();

  return (
    <div className="step-form">
      <StepOwnFields step={step} onChange={onChange} disabled={disabled} />
      {isRepeatable(step) ? <RepeatFields step={step} onChange={onChange} disabled={disabled} /> : null}
      <Field label={t("steps.fields.pauseAfter")}>
        <PauseInput
          value={step.pauseAfter}
          disabled={disabled}
          onChange={(pauseAfter) => onChange({ ...step, pauseAfter })}
        />
      </Field>
    </div>
  );
}
