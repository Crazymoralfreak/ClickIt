import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT, type TKey } from "../../i18n";
import type { CleanupInfo, CleanupKind } from "../../types/runtime";
import { Button } from "../../ui";

const KINDS: Record<CleanupKind, { name: TKey; text: TKey }> = {
  journal: { name: "settings.cleanup.journal.name", text: "settings.cleanup.journal.text" },
  chats: { name: "settings.cleanup.chats.name", text: "settings.cleanup.chats.text" },
  connections: { name: "settings.cleanup.connections.name", text: "settings.cleanup.connections.text" },
  templates: { name: "settings.cleanup.templates.name", text: "settings.cleanup.templates.text" },
  backups: { name: "settings.cleanup.backups.name", text: "settings.cleanup.backups.text" },
  cache: { name: "settings.cleanup.cache.name", text: "settings.cleanup.cache.text" },
  leftovers: { name: "settings.cleanup.leftovers.name", text: "settings.cleanup.leftovers.text" },
};

const KB = 1024;
const MB = KB * KB;

/** One "Clear" button for every kind of data that only piles up. */
export function DataCleanup() {
  const { t, td, language } = useT();
  const { locked, confirm, notify, showError } = useApp();
  /** Null until the sizes have been counted. */
  const [items, setItems] = useState<CleanupInfo[] | null>(null);
  const [running, setRunning] = useState<CleanupKind | null>(null);

  const refresh = useCallback(() => {
    api.cleanupInfo().then(setItems).catch(showError);
  }, [showError]);

  // A run that starts or ends changes what may be cleared.
  useEffect(refresh, [refresh, locked]);

  const sizeText = (bytes: number): string => {
    const round = (value: number) => value.toLocaleString(language, { maximumFractionDigits: 1 });
    if (bytes < KB) {
      return t("settings.sizeBytes", { value: bytes });
    }
    return bytes < MB
      ? t("settings.sizeKb", { value: round(bytes / KB) })
      : t("settings.sizeMb", { value: round(bytes / MB) });
  };

  const clear = async (item: CleanupInfo) => {
    const name = t(KINDS[item.kind].name);
    const ok = await confirm({
      title: t("settings.cleanupConfirmTitle", { name }),
      text: t(KINDS[item.kind].text),
      okLabel: t("settings.cleanupClear"),
      danger: true,
    });
    if (!ok) {
      return;
    }
    setRunning(item.kind);
    try {
      setItems(await api.cleanupRun(item.kind));
      notify(t("settings.cleanupDone", { name }));
    } catch (error) {
      showError(error);
      refresh();
    } finally {
      setRunning(null);
    }
  };

  return (
    <div className="settings-cleanup">
      <div className="settings-cleanup-title">{t("settings.cleanupTitle")}</div>
      <div className="field-hint">{t("settings.cleanupHint")}</div>
      {items === null ? (
        <div className="settings-empty">{t("common.loading")}</div>
      ) : (
        <ul className="settings-list settings-cleanup-list">
          {items.map((item) => {
            const empty = item.files === 0 && item.bytes === 0;
            return (
              <li key={item.kind} className="settings-list-row settings-cleanup-row">
                <div className="settings-cleanup-about">
                  <div className="settings-cleanup-name">
                    {t(KINDS[item.kind].name)}
                    <span className="settings-cleanup-size">
                      {empty
                        ? t("settings.cleanupEmpty")
                        : t("settings.cleanupSize", { size: sizeText(item.bytes), files: item.files })}
                    </span>
                  </div>
                  <div className="field-hint">{t(KINDS[item.kind].text)}</div>
                  {item.blocked !== null && !empty ? (
                    <div className="field-error">{td(`errors.${item.blocked}`)}</div>
                  ) : null}
                </div>
                <Button
                  small
                  disabled={empty || item.blocked !== null || running !== null}
                  onClick={() => void clear(item)}
                >
                  {t("settings.cleanupClear")}
                </Button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
