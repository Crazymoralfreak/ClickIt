import { useT } from "../../i18n";
import { MAX_STEP_REPEAT, type RepeatableStep, type Step } from "../../types/scenario";
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
function withRepeatTimes(step: RepeatableStep, times: number): RepeatableStep {
  const next = { ...step };
  if (times === 1) {
    delete next.repeat;
  } else {
    next.repeat = { times, pauseBetween: step.repeat?.pauseBetween ?? noPause() };
  }
  return next;
}

function RepeatFields({ step, onChange, disabled }: StepFormProps & { step: RepeatableStep }) {
  const { t } = useT();
  const repeat = step.repeat;
  const times = repeat?.times ?? 1;
  return (
    <>
      <Field label={t("steps.fields.repeat")} hint={t("steps.fields.repeatHint")}>
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

/** Settings of one step, shown under its row. */
export function StepForm({ step, onChange, disabled }: StepFormProps) {
  const { t } = useT();
  const env = useEditorEnv();

  const ownFields = () => {
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
        return <WaitForm step={step} onChange={onChange} disabled={disabled} env={env} />;
      case "if":
        return <IfForm step={step} onChange={onChange} disabled={disabled} env={env} />;
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
  };

  return (
    <div className="step-form">
      {ownFields()}
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
