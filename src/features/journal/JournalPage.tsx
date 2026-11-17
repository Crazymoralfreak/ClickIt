import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { useAppEvent } from "../../api/events";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { JournalEntry } from "../../types/runtime";
import { Button, EmptyState, PageFrame } from "../../ui";
import { journalText } from "./journalText";
import "./journal.css";

export function JournalPage() {
  const translator = useT();
  const { t } = translator;
  const { showError } = useApp();
  /** Oldest first, as the core returns them; null until the first load. */
  const [entries, setEntries] = useState<JournalEntry[] | null>(null);
  const requestRef = useRef(0);

  const reload = useCallback(() => {
    const request = ++requestRef.current;
    api
      .journalGet()
      .then((loaded) => {
        // Only the answer to the latest request counts.
        if (request === requestRef.current) {
          setEntries(loaded);
        }
      })
      .catch(showError);
  }, [showError]);

  useEffect(reload, [reload]);
  useAppEvent("journal:changed", reload);

  const refreshButton = <Button onClick={reload}>{t("journalView.refresh")}</Button>;

  return (
    <PageFrame title={t("nav.journal")} actions={refreshButton}>
      <div className="journal-note">{t("journalView.note")}</div>
      {entries !== null && entries.length === 0 ? (
        <EmptyState title={t("journalView.emptyTitle")} text={t("journalView.emptyText")} />
      ) : null}
      {entries !== null && entries.length > 0 ? (
        <div className="journal-list">
          {entries
            .map((entry, index) => (
              // Entries have no id; rows keep no state, so the position in the original order will do.
              <div key={index} className="journal-row">
                <span className="journal-time">{entry.time}</span>
                <span className="journal-text">{journalText(entry, translator)}</span>
              </div>
            ))
            .reverse()}
        </div>
      ) : null}
    </PageFrame>
  );
}
