// The fields of a command: the same for a step of a list and for a block of a graph.

import { useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import {
  MAX_COMMAND_FIELD_CHARS,
  MAX_SCRIPT_CHARS,
  type CommandRun,
  type CommandSpec,
  type ShellKind,
} from "../../types/scenario";
import { Button, Checkbox, Field, NumberInput, Segmented, Select, TextInput, cx, type Option } from "../../ui";
import { emptyCommandRun } from "./defaults";

interface CommandFieldsProps {
  value: CommandSpec;
  onChange: (value: CommandSpec) => void;
  disabled: boolean;
}

export function CommandFields({ value, onChange, disabled }: CommandFieldsProps) {
  const { t } = useT();
  const { settings, navigate, showError } = useApp();
  const run = value.run;
  // What was typed for the other kind comes back when the user switches again.
  const [other, setOther] = useState<CommandRun | null>(null);

  const kinds: Array<Option<CommandRun["kind"]>> = [
    { value: "program", label: t("steps.command.kindProgram") },
    { value: "script", label: t("steps.command.kindScript") },
  ];
  const shells: Array<Option<ShellKind>> = [
    { value: "cmd", label: t("steps.command.shellCmd") },
    { value: "powershell", label: t("steps.command.shellPowerShell") },
  ];

  const setKind = (kind: CommandRun["kind"]) => {
    if (kind === run.kind) {
      return;
    }
    setOther(run);
    onChange({ ...value, run: other !== null && other.kind === kind ? other : emptyCommandRun(kind) });
  };

  const pick = (ask: () => Promise<string | null>, apply: (picked: string) => void) => {
    ask()
      .then((picked) => {
        if (picked !== null) {
          apply(picked);
        }
      })
      .catch(showError);
  };

  return (
    <>
      {settings.allowCommands ? null : (
        <div className="command-off" role="note">
          <span>{t("steps.command.off")}</span>
          <Button small onClick={() => navigate("settings")}>
            {t("steps.command.offOpen")}
          </Button>
        </div>
      )}
      <Field label={t("steps.command.kind")}>
        <Segmented value={run.kind} options={kinds} disabled={disabled} onChange={setKind} />
      </Field>
      {run.kind === "program" ? (
        <>
          <Field label={t("steps.command.path")} hint={t("steps.command.pathHint")}>
            <TextInput
              className="editor-wide-input"
              value={run.path}
              spellCheck={false}
              disabled={disabled}
              invalid={run.path.trim() === "" || run.path.length > MAX_COMMAND_FIELD_CHARS}
              onChange={(path) => onChange({ ...value, run: { ...run, path } })}
            />
            <Button
              disabled={disabled}
              onClick={() => pick(api.commandPickFile, (path) => onChange({ ...value, run: { ...run, path } }))}
            >
              {t("steps.command.browse")}
            </Button>
          </Field>
          <Field label={t("steps.command.args")} hint={t("steps.command.argsHint")}>
            <TextInput
              className="editor-wide-input"
              value={run.args}
              spellCheck={false}
              disabled={disabled}
              invalid={run.args.length > MAX_COMMAND_FIELD_CHARS}
              onChange={(args) => onChange({ ...value, run: { ...run, args } })}
            />
          </Field>
        </>
      ) : (
        <>
          <Field label={t("steps.command.shell")}>
            <Select
              value={run.shell}
              options={shells}
              disabled={disabled}
              onChange={(shell) => onChange({ ...value, run: { ...run, shell } })}
            />
          </Field>
          <Field
            label={t("steps.command.text")}
            hint={run.shell === "cmd" ? t("steps.command.textHintCmd") : t("steps.command.textHintPowerShell")}
          >
            <textarea
              className={cx(
                "input",
                "step-textarea",
                "command-script",
                (run.text.trim() === "" || run.text.length > MAX_SCRIPT_CHARS) && "input-invalid",
              )}
              value={run.text}
              rows={6}
              spellCheck={false}
              disabled={disabled}
              onChange={(event) => onChange({ ...value, run: { ...run, text: event.target.value } })}
            />
          </Field>
        </>
      )}
      <Field label={t("steps.command.workDir")} hint={t("steps.command.workDirHint")}>
        <TextInput
          className="editor-wide-input"
          value={value.workDir}
          spellCheck={false}
          disabled={disabled}
          invalid={value.workDir.length > MAX_COMMAND_FIELD_CHARS}
          onChange={(workDir) => onChange({ ...value, workDir })}
        />
        <Button
          disabled={disabled}
          onClick={() => pick(api.commandPickFolder, (workDir) => onChange({ ...value, workDir }))}
        >
          {t("steps.command.browse")}
        </Button>
      </Field>
      <Field label={t("steps.command.window")} hint={t("steps.command.hiddenHint")}>
        <Checkbox
          checked={value.hidden}
          disabled={disabled}
          label={t("steps.command.hidden")}
          onChange={(hidden) => onChange({ ...value, hidden })}
        />
      </Field>
      <Field label={t("steps.command.end")} hint={t("steps.command.waitHint")}>
        <Checkbox
          checked={value.wait}
          disabled={disabled}
          label={t("steps.command.wait")}
          onChange={(wait) => onChange({ ...value, wait })}
        />
      </Field>
      {value.wait ? (
        <Field label={t("steps.fields.timeout")} hint={t("steps.command.timeoutHint")}>
          <NumberInput
            value={value.timeoutMs}
            min={0}
            wide
            suffix={t("common.ms")}
            disabled={disabled}
            invalid={value.timeoutMs < 0}
            onChange={(timeoutMs) => onChange({ ...value, timeoutMs })}
          />
        </Field>
      ) : null}
    </>
  );
}
