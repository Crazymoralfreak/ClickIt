import { useCallback, useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import { Button, Modal, TextInput } from "../../ui";

const MAX_NAME_LENGTH = 260;

/** The "Do not run while running" list: executable names typed by hand or picked from open windows. */
export function BlockedProcesses() {
  const { t } = useT();
  const { settings, saveSettings, showError } = useApp();
  const [text, setText] = useState("");
  const [saving, setSaving] = useState(false);
  /** Process names of the open windows while the picker is shown. */
  const [picker, setPicker] = useState<string[] | null>(null);

  const list = settings.blockedProcesses;
  const listed = (name: string) => list.some((item) => item.toLowerCase() === name.toLowerCase());

  const add = async (name: string): Promise<boolean> => {
    setSaving(true);
    const saved = await saveSettings({ ...settings, blockedProcesses: [...list, name] });
    setSaving(false);
    return saved;
  };

  const addTyped = async () => {
    const name = text.trim();
    if (name === "" || saving) {
      return;
    }
    if (listed(name) || (await add(name))) {
      setText("");
    }
  };

  const remove = async (name: string) => {
    setSaving(true);
    await saveSettings({ ...settings, blockedProcesses: list.filter((item) => item !== name) });
    setSaving(false);
  };

  const openPicker = async () => {
    try {
      const windows = await api.windowsList();
      const seen = new Set<string>();
      const names: string[] = [];
      for (const info of windows) {
        const name = info.process.trim();
        const key = name.toLowerCase();
        if (name !== "" && !seen.has(key)) {
          seen.add(key);
          names.push(name);
        }
      }
      names.sort((a, b) => a.localeCompare(b));
      setPicker(names);
    } catch (error) {
      showError(error);
    }
  };

  const closePicker = useCallback(() => setPicker(null), []);

  return (
    <>
      <div className="settings-blocked-add">
        <TextInput
          value={text}
          onChange={setText}
          placeholder={t("settings.blockedPlaceholder")}
          aria-label={t("settings.blockedNameLabel")}
          maxLength={MAX_NAME_LENGTH}
          spellCheck={false}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              void addTyped();
            }
          }}
        />
        <Button disabled={saving || text.trim() === ""} onClick={() => void addTyped()}>
          {t("common.add")}
        </Button>
        <Button onClick={() => void openPicker()}>{t("settings.blockedPick")}</Button>
      </div>
      {list.length === 0 ? (
        <div className="settings-empty">{t("settings.blockedEmpty")}</div>
      ) : (
        <ul className="settings-list">
          {list.map((name) => (
            <li key={name} className="settings-list-row">
              <span className="settings-list-name">{name}</span>
              <Button small variant="ghost" disabled={saving} onClick={() => void remove(name)}>
                {t("settings.blockedRemove")}
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="field-hint">{t("settings.blockedHint")}</div>
      {picker ? (
        <Modal
          title={t("settings.blockedPickTitle")}
          onClose={closePicker}
          footer={<Button onClick={closePicker}>{t("common.close")}</Button>}
        >
          {picker.length === 0 ? (
            <p className="modal-text">{t("settings.blockedPickEmpty")}</p>
          ) : (
            <ul className="settings-list">
              {picker.map((name) => (
                <li key={name} className="settings-list-row">
                  <span className="settings-list-name">{name}</span>
                  {listed(name) ? (
                    <span className="settings-list-added">{t("settings.blockedAdded")}</span>
                  ) : (
                    <Button small disabled={saving} onClick={() => void add(name)}>
                      {t("common.add")}
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </Modal>
      ) : null}
    </>
  );
}
