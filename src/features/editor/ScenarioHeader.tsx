import { useEffect, useState } from "react";
import { useT } from "../../i18n";
import type { Scenario, ScenarioMode, TitleMatch, WindowTarget } from "../../types/scenario";
import { Button, Field, Section, Segmented, Select, TextInput, type Option } from "../../ui";
import { WindowPicker } from "./WindowPicker";

interface ScenarioHeaderProps {
  draft: Scenario;
  change: (fn: (scenario: Scenario) => Scenario) => void;
  disabled: boolean;
}

interface NameFieldProps {
  name: string;
  disabled: boolean;
  onChange: (name: string) => void;
}

/** The draft never gets an empty name: the field keeps the text and shows the problem instead. */
function NameField({ name, disabled, onChange }: NameFieldProps) {
  const { t } = useT();
  const [text, setText] = useState(name);

  useEffect(() => {
    setText((current) => (current.trim() === name ? current : name));
  }, [name]);

  const empty = text.trim() === "";
  return (
    <Field label={t("editor.name")} error={empty ? t("issues.nameEmpty") : undefined}>
      <TextInput
        className="editor-wide-input"
        value={text}
        maxLength={80}
        disabled={disabled}
        invalid={empty}
        onChange={(next) => {
          setText(next);
          const trimmed = next.trim();
          if (trimmed !== "") {
            onChange(trimmed);
          }
        }}
        onBlur={() => setText(name)}
      />
    </Field>
  );
}

/** Name, mode and window target of the scenario. */
export function ScenarioHeader({ draft, change, disabled }: ScenarioHeaderProps) {
  const { t } = useT();
  const [picking, setPicking] = useState(false);

  const modeOptions: Array<Option<ScenarioMode>> = [
    { value: "screen", label: t("scenarios.modeScreen") },
    { value: "window", label: t("scenarios.modeWindow") },
  ];
  const matchOptions: Array<Option<TitleMatch>> = [
    { value: "contains", label: t("editor.titleContains") },
    { value: "exact", label: t("editor.titleExact") },
  ];

  const setTarget = (patch: Partial<WindowTarget>) =>
    change((scenario) => ({ ...scenario, target: { ...scenario.target, ...patch } }));

  return (
    <Section>
      <NameField
        name={draft.name}
        disabled={disabled}
        onChange={(name) => change((scenario) => (scenario.name === name ? scenario : { ...scenario, name }))}
      />
      <Field
        label={t("editor.mode")}
        hint={
          <>
            <span className="editor-hint-line">
              {draft.mode === "screen" ? t("editor.modeScreenHint") : t("editor.modeWindowHint")}
            </span>
            <span className="editor-hint-line">{t("editor.modeChangeHint")}</span>
          </>
        }
      >
        <Segmented
          value={draft.mode}
          options={modeOptions}
          disabled={disabled}
          onChange={(mode) => change((scenario) => (scenario.mode === mode ? scenario : { ...scenario, mode }))}
        />
      </Field>
      {draft.mode === "window" ? (
        <>
          <Field label={t("editor.targetTitle")}>
            <TextInput
              className="editor-wide-input"
              value={draft.target.title}
              disabled={disabled}
              onChange={(title) => setTarget({ title })}
            />
            <Select
              value={draft.target.titleMatch}
              options={matchOptions}
              disabled={disabled}
              onChange={(titleMatch) => setTarget({ titleMatch })}
            />
          </Field>
          <Field
            label={t("editor.targetProcess")}
            hint={
              <>
                <span className="editor-hint-line">{t("editor.targetProcessHint")}</span>
                <span className="editor-hint-line">{t("editor.targetHint")}</span>
              </>
            }
          >
            <TextInput
              className="editor-wide-input"
              value={draft.target.process}
              spellCheck={false}
              disabled={disabled}
              onChange={(process) => setTarget({ process })}
            />
            <Button disabled={disabled} onClick={() => setPicking(true)}>
              {t("editor.pickWindow")}
            </Button>
          </Field>
        </>
      ) : null}
      {picking ? (
        <WindowPicker
          onClose={() => setPicking(false)}
          onPick={(window) => {
            setTarget({ title: window.title, process: window.process });
            setPicking(false);
          }}
        />
      ) : null}
    </Section>
  );
}
