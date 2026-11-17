import { useCallback, useEffect, useState, type ReactNode } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import { MAX_COUNTDOWN_SEC, type Language } from "../../types/scenario";
import { Button, Checkbox, Field, NumberInput, PageFrame, Section, Select } from "../../ui";
import { BlockedProcesses } from "./BlockedProcesses";
import { HotkeyInput } from "./HotkeyInput";
import "./settings.css";

const DEFAULT_HOTKEY_START_STOP = "F6";
const DEFAULT_HOTKEY_CAPTURE = "F8";

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
  onChange: (hotkey: string) => void;
}

function HotkeyField({ label, hint, value, fallback, disabled, onChange }: HotkeyFieldProps) {
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
    </Field>
  );
}

export function SettingsPage() {
  const { t } = useT();
  const { info, settings, saveSettings, locked, showError } = useApp();
  /** Null until the registry entry has been read. */
  const [autostart, setAutostart] = useState<boolean | null>(null);

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
      <Section title={t("settings.general")}>
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

      <Section title={t("settings.hotkeys")}>
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
        <div className="field-hint">{locked ? t("settings.hotkeysLocked") : t("settings.hotkeyHint")}</div>
      </Section>

      <Section title={t("settings.startStop")}>
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

      <Section title={t("settings.app")}>
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

      <Section title={t("settings.blocked")}>
        <BlockedProcesses />
      </Section>

      <Section title={t("settings.noticeTitle")}>
        <p className="settings-text">{t("settings.noticePresence")}</p>
        <p className="settings-text">{t("settings.noticeAdmin")}</p>
      </Section>

      <Section title={t("settings.data")}>
        <Field label={t("settings.dataDir")}>
          <span className="settings-path">{info.dataDir}</span>
        </Field>
      </Section>
    </PageFrame>
  );
}
