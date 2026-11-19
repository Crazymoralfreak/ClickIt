import { useCallback, useEffect, useState, type ReactNode } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import { MAX_COUNTDOWN_SEC, type Language } from "../../types/scenario";
import { Button, Checkbox, Field, NumberInput, PageFrame, Section, Select, TextInput } from "../../ui";
import { BlockedProcesses } from "./BlockedProcesses";
import { DataCleanup } from "./DataCleanup";
import { HotkeyInput } from "./HotkeyInput";
import "./settings.css";

const DEFAULT_HOTKEY_START_STOP = "F6";
const DEFAULT_HOTKEY_CAPTURE = "F8";
const DEFAULT_HOTKEY_STEP = "F10";
const DEFAULT_HOTKEY_CONTINUE = "F9";
const DEFAULT_HOTKEY_RECORD_PAUSE = "F7";
const DEFAULT_HOTKEY_RECORD_WAIT_IMAGE = "Shift+F8";
const DEFAULT_HOTKEY_RECORD_CLICK_IMAGE = "Ctrl+F8";
const OLLAMA_ENDPOINT = "http://127.0.0.1:11434/v1";
const LM_STUDIO_ENDPOINT = "http://127.0.0.1:1234/v1";

interface CheckRowProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  hint?: ReactNode;
  disabled?: boolean;
}

/** Checkbox on its own line with an optional explanation under it. */
function CheckRow({ checked, onChange, label, hint, disabled }: CheckRowProps) {
  return (
    <div className="settings-check">
      <Checkbox checked={checked} onChange={onChange} label={label} disabled={disabled} />
      {hint ? <div className="field-hint settings-check-hint">{hint}</div> : null}
    </div>
  );
}

interface HotkeyFieldProps {
  label: string;
  hint?: string;
  value: string;
  fallback: string;
  disabled: boolean;
  /** An empty value is allowed: the hotkey is not assigned. */
  clearable?: boolean;
  onChange: (hotkey: string) => void;
}

function HotkeyField({ label, hint, value, fallback, disabled, clearable, onChange }: HotkeyFieldProps) {
  const { t } = useT();
  return (
    <Field label={label} hint={hint}>
      <HotkeyInput value={value} label={label} disabled={disabled} onChange={onChange} />
      <Button
        disabled={disabled || value === fallback}
        title={t("settings.hotkeyResetTitle", { hotkey: fallback })}
        onClick={() => onChange(fallback)}
      >
        {t("settings.hotkeyReset")}
      </Button>
      {clearable ? (
        <Button disabled={disabled || value === ""} title={t("settings.hotkeyClearTitle")} onClick={() => onChange("")}>
          {t("settings.hotkeyClear")}
        </Button>
      ) : null}
    </Field>
  );
}

export function SettingsPage() {
  const { t } = useT();
  const { info, settings, saveSettings, locked, showError } = useApp();
  /** Null until the registry entry has been read. */
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [endpoint, setEndpoint] = useState(settings.agentEndpoint);
  const [model, setModel] = useState(settings.agentModel);
  const [key, setKey] = useState("");
  const [keySet, setKeySet] = useState(false);

  useEffect(() => {
    setEndpoint(settings.agentEndpoint);
    setModel(settings.agentModel);
  }, [settings.agentEndpoint, settings.agentModel]);

  useEffect(() => {
    api
      .agentInfo()
      .then((info) => setKeySet(info.keySet))
      .catch(showError);
  }, [showError]);

  const saveAgent = (agentEndpoint: string, agentModel: string) => {
    const nextEndpoint = agentEndpoint.trim();
    const nextModel = agentModel.trim();
    setEndpoint(nextEndpoint);
    setModel(nextModel);
    if (nextEndpoint !== settings.agentEndpoint || nextModel !== settings.agentModel) {
      void saveSettings({ ...settings, agentEndpoint: nextEndpoint, agentModel: nextModel });
    }
  };

  const readAutostart = useCallback(
    () => api.autostartGet().then(setAutostart).catch(showError),
    [showError],
  );

  useEffect(() => {
    void readAutostart();
  }, [readAutostart]);

  const changeAutostart = async (enabled: boolean) => {
    setAutostart(enabled);
    try {
      await api.autostartSet(enabled);
    } catch (error) {
      showError(error);
    }
    // The registry is the source of truth, whatever the command answered.
    await readAutostart();
  };

  return (
    <PageFrame title={t("nav.settings")}>
      <Section title={t("settings.general")} collapsible defaultOpen={false}>
        <Field label={t("settings.language")}>
          <Select<Language>
            value={settings.language ?? info.systemLanguage}
            options={[
              { value: "ru", label: t("settings.languageRu") },
              { value: "en", label: t("settings.languageEn") },
            ]}
            onChange={(language) => void saveSettings({ ...settings, language })}
          />
        </Field>
      </Section>

      <Section title={t("settings.hotkeys")} collapsible defaultOpen={false}>
        <HotkeyField
          label={t("settings.hotkeyStartStop")}
          value={settings.hotkeyStartStop}
          fallback={DEFAULT_HOTKEY_START_STOP}
          disabled={locked}
          onChange={(hotkeyStartStop) => void saveSettings({ ...settings, hotkeyStartStop })}
        />
        <HotkeyField
          label={t("settings.hotkeyCapture")}
          hint={t("settings.hotkeyCaptureHint")}
          value={settings.hotkeyCapture}
          fallback={DEFAULT_HOTKEY_CAPTURE}
          disabled={locked}
          onChange={(hotkeyCapture) => void saveSettings({ ...settings, hotkeyCapture })}
        />
        <HotkeyField
          label={t("settings.hotkeyStep")}
          value={settings.hotkeyStep}
          fallback={DEFAULT_HOTKEY_STEP}
          disabled={locked}
          onChange={(hotkeyStep) => void saveSettings({ ...settings, hotkeyStep })}
        />
        <HotkeyField
          label={t("settings.hotkeyContinue")}
          hint={t("settings.hotkeyDebugHint")}
          value={settings.hotkeyContinue}
          fallback={DEFAULT_HOTKEY_CONTINUE}
          disabled={locked}
          onChange={(hotkeyContinue) => void saveSettings({ ...settings, hotkeyContinue })}
        />
        <div className="field-hint settings-hotkey-group">{t("settings.hotkeyRecordGroup")}</div>
        <HotkeyField
          label={t("settings.hotkeyRecordPause")}
          hint={t("settings.hotkeyRecordPauseHint")}
          value={settings.hotkeyRecordPause}
          fallback={DEFAULT_HOTKEY_RECORD_PAUSE}
          disabled={locked}
          clearable
          onChange={(hotkeyRecordPause) => void saveSettings({ ...settings, hotkeyRecordPause })}
        />
        <HotkeyField
          label={t("settings.hotkeyRecordWaitImage")}
          hint={t("settings.hotkeyRecordWaitImageHint")}
          value={settings.hotkeyRecordWaitImage}
          fallback={DEFAULT_HOTKEY_RECORD_WAIT_IMAGE}
          disabled={locked}
          clearable
          onChange={(hotkeyRecordWaitImage) => void saveSettings({ ...settings, hotkeyRecordWaitImage })}
        />
        <HotkeyField
          label={t("settings.hotkeyRecordClickImage")}
          hint={t("settings.hotkeyRecordClickImageHint")}
          value={settings.hotkeyRecordClickImage}
          fallback={DEFAULT_HOTKEY_RECORD_CLICK_IMAGE}
          disabled={locked}
          clearable
          onChange={(hotkeyRecordClickImage) => void saveSettings({ ...settings, hotkeyRecordClickImage })}
        />
        <div className="field-hint">{locked ? t("settings.hotkeysLocked") : t("settings.hotkeyHint")}</div>
      </Section>

      <Section title={t("settings.startStop")} collapsible defaultOpen={false}>
        <Field
          label={t("settings.countdown")}
          hint={t("settings.countdownHint", { max: MAX_COUNTDOWN_SEC })}
        >
          <NumberInput
            value={settings.countdownSec}
            min={0}
            max={MAX_COUNTDOWN_SEC}
            suffix={t("common.sec")}
            onChange={(value) => {
              const countdownSec = Math.min(MAX_COUNTDOWN_SEC, Math.max(0, value));
              if (countdownSec !== settings.countdownSec) {
                void saveSettings({ ...settings, countdownSec });
              }
            }}
          />
        </Field>
        <CheckRow
          checked={settings.cornerStop}
          label={t("settings.cornerStop")}
          onChange={(cornerStop) => void saveSettings({ ...settings, cornerStop })}
        />
      </Section>

      <Section title={t("settings.app")} collapsible defaultOpen={false}>
        <CheckRow
          checked={autostart ?? false}
          disabled={autostart === null}
          label={t("settings.autostart")}
          hint={t("settings.autostartHint")}
          onChange={(enabled) => void changeAutostart(enabled)}
        />
        <CheckRow
          checked={settings.closeExits}
          label={t("settings.closeExits")}
          hint={t("settings.closeExitsHint")}
          onChange={(closeExits) => void saveSettings({ ...settings, closeExits })}
        />
        <CheckRow
          checked={settings.catchUp}
          label={t("settings.catchUp")}
          hint={t("settings.catchUpHint")}
          onChange={(catchUp) => void saveSettings({ ...settings, catchUp })}
        />
        <CheckRow
          checked={settings.checkUpdates}
          label={t("settings.checkUpdates")}
          hint={t("settings.checkUpdatesHint")}
          onChange={(checkUpdates) => void saveSettings({ ...settings, checkUpdates })}
        />
      </Section>

      <Section title={t("settings.harnessTitle")} collapsible defaultOpen={false}>
        <CheckRow
          checked={settings.harness}
          label={t("settings.harness")}
          hint={t("settings.harnessHint")}
          onChange={(harness) => void saveSettings({ ...settings, harness })}
        />
        <CheckRow
          checked={settings.harnessAutoRun}
          label={t("settings.harnessAutoRun")}
          hint={t("settings.harnessAutoRunHint")}
          onChange={(harnessAutoRun) => void saveSettings({ ...settings, harnessAutoRun })}
        />
        <Field label={t("settings.agentEndpoint")} hint={t("settings.agentKeyHint")}>
          <TextInput
            value={endpoint}
            onChange={setEndpoint}
            onBlur={() => saveAgent(endpoint, model)}
            spellCheck={false}
          />
        </Field>
        <div className="settings-presets">
          <Button
            small
            onClick={() => saveAgent(OLLAMA_ENDPOINT, model)}
          >
            {t("settings.agentPresetOllama")}
          </Button>
          <Button
            small
            onClick={() => saveAgent(LM_STUDIO_ENDPOINT, model)}
          >
            {t("settings.agentPresetLmStudio")}
          </Button>
        </div>
        <Field label={t("settings.agentModel")}>
          <TextInput
            value={model}
            onChange={setModel}
            onBlur={() => saveAgent(endpoint, model)}
            spellCheck={false}
          />
        </Field>
        <Field label={t("settings.agentKey")} hint={keySet ? t("settings.agentKeySaved") : undefined}>
          <TextInput
            type="password"
            value={key}
            onChange={setKey}
            spellCheck={false}
            autoComplete="off"
            onBlur={() => {
              const next = key.trim();
              if (!next) {
                return;
              }
              void api
                .agentKeySet(next)
                .then(() => {
                  setKey("");
                  setKeySet(true);
                })
                .catch(showError);
            }}
          />
        </Field>
        {keySet ? (
          <Button
            small
            onClick={() => {
              void api
                .agentKeySet("")
                .then(() => setKeySet(false))
                .catch(showError);
            }}
          >
            {t("settings.agentKeyClear")}
          </Button>
        ) : null}
      </Section>

      <Section title={t("settings.blocked")} collapsible defaultOpen={false}>
        <BlockedProcesses />
      </Section>

      <Section title={t("settings.noticeTitle")} collapsible defaultOpen={false}>
        <p className="settings-text">{t("settings.noticePresence")}</p>
        <p className="settings-text">{t("settings.noticeAdmin")}</p>
      </Section>

      <Section title={t("settings.data")} collapsible defaultOpen={false}>
        <Field label={t("settings.dataDir")}>
          <span className="settings-path">{info.dataDir}</span>
        </Field>
        <DataCleanup />
      </Section>
    </PageFrame>
  );
}
