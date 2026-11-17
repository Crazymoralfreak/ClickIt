import type { Translator } from "../../i18n";
import type { JournalEntry } from "../../types/runtime";

/** Localized line of a journal entry. Codes stored in the params are translated first. */
export function journalText(entry: JournalEntry, { td }: Translator): string {
  const params: Record<string, string> = { ...entry.params };
  // Any of them may be absent: which params an entry carries depends on its code.
  const source: string | undefined = entry.params.source;
  const outcome: string | undefined = entry.params.outcome;
  const reason: string | undefined = entry.params.reason;

  if (source !== undefined) {
    params.source = td(`journal.sources.${source}`, undefined, source);
  }
  if (outcome !== undefined) {
    params.outcome = td(`journal.outcomes.${outcome}`, undefined, outcome);
  }
  if (reason !== undefined) {
    // Everywhere except "recordEnd" the reason is an error code, and the entry's
    // other params fill the placeholders of that error text.
    params.reason =
      entry.code === "recordEnd"
        ? td(`journal.recordReasons.${reason}`, undefined, reason)
        : td(`errors.${reason}`, entry.params, reason);
  }

  const fallback = `${entry.code} ${JSON.stringify(entry.params)}`;
  return td(`journal.codes.${entry.code}`, params, fallback);
}
