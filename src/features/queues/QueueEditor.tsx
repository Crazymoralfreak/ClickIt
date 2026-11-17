import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { Issue } from "../../types/runtime";
import type { OnItemFailure, Queue, ScenarioSummary } from "../../types/scenario";
import { Button, Field, PauseInput, RepeatModeInput, Section, Select, TextInput } from "../../ui";
import { QueueItems } from "./QueueItems";

const SAVE_DELAY_MS = 400;

/** Queue-level issue codes that have their own place in the form. */
const NAME_CODES = ["nameEmpty"];
const ITEMS_CODES = ["queueEmpty"];
const PAUSE_CODES = ["pauseNegative", "pauseOrder"];
const REPEAT_CODES = ["roundsRange"];
const PLACED_CODES = [...NAME_CODES, ...ITEMS_CODES, ...PAUSE_CODES, ...REPEAT_CODES];

/** What the queue list shows about a queue; a change of it means the list has to be reloaded. */
function listKey(queue: Queue, scenarioIds: ReadonlySet<string> | null): string {
  const broken = scenarioIds !== null && queue.items.some((item) => !scenarioIds.has(item.scenarioId));
  return `${queue.name}\n${queue.items.length}\n${broken ? 1 : 0}`;
}

interface QueueEditorProps {
  queueId: string;
  /** Null until the scenario list is loaded. */
  scenarios: ScenarioSummary[] | null;
}

/** Editor of one queue. Mount it with a `key` so that another queue gets a fresh editor. */
export function QueueEditor({ queueId, scenarios }: QueueEditorProps) {
  const { t, td } = useT();
  const { locked, start, touch, showError, registerFlush, dataVersion } = useApp();
  const [draft, setDraft] = useState<Queue | null>(null);
  const [issues, setIssues] = useState<Issue[]>([]);

  const draftRef = useRef<Queue | null>(null);
  const dirtyRef = useRef(false);
  const timerRef = useRef<number | null>(null);
  const savesRef = useRef<Promise<void>>(Promise.resolve());
  const storedRef = useRef({ name: "", listKey: "" });
  const validatedRef = useRef(false);

  const scenarioIds = useMemo(
    () => (scenarios ? new Set(scenarios.map((scenario) => scenario.id)) : null),
    [scenarios],
  );
  const scenarioIdsRef = useRef(scenarioIds);
  scenarioIdsRef.current = scenarioIds;

  /** Writes pending edits and resolves when every save in flight has finished. */
  const flush = useCallback(async (): Promise<void> => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    const queue = draftRef.current;
    if (dirtyRef.current && queue) {
      dirtyRef.current = false;
      savesRef.current = savesRef.current.then(async () => {
        // An empty name is never written: the stored one stays on disk until a new one is
        // typed, so the other edits are not lost while the name field is blank.
        const name = queue.name.trim() === "" ? storedRef.current.name : queue.name;
        const stored: Queue = { ...queue, name };
        try {
          await api.queueSave(stored);
          const key = listKey(stored, scenarioIdsRef.current);
          const listChanged = key !== storedRef.current.listKey;
          storedRef.current = { name, listKey: key };
          if (listChanged) {
            touch("queues");
          }
        } catch (error) {
          // Keep the edit pending so that the next flush tries again.
          dirtyRef.current = true;
          showError(error);
        }
      });
    }
    await savesRef.current;
  }, [touch, showError]);

  useEffect(() => {
    let cancelled = false;
    api
      .queueGet(queueId)
      .then((queue) => {
        if (cancelled) {
          return;
        }
        draftRef.current = queue;
        storedRef.current = { name: queue.name, listKey: listKey(queue, scenarioIdsRef.current) };
        setDraft(queue);
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          showError(error);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [queueId, showError]);

  useEffect(() => {
    const unregister = registerFlush(flush);
    return () => {
      unregister();
      void flush();
    };
  }, [registerFlush, flush]);

  useEffect(() => {
    if (!draft) {
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(
      () => {
        validatedRef.current = true;
        api
          .queueValidate(draft)
          .then((found) => {
            if (!cancelled) {
              setIssues(found);
            }
          })
          .catch((error: unknown) => {
            if (!cancelled) {
              showError(error);
            }
          });
      },
      validatedRef.current ? SAVE_DELAY_MS : 0,
    );
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
    // A deleted or restored scenario changes the result for the same draft.
  }, [draft, dataVersion.scenarios, showError]);

  const edit = useCallback(
    (change: (queue: Queue) => Queue) => {
      const current = draftRef.current;
      if (!current) {
        return;
      }
      const next = change(current);
      draftRef.current = next;
      dirtyRef.current = true;
      setDraft(next);
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
      }
      timerRef.current = window.setTimeout(() => void flush(), SAVE_DELAY_MS);
    },
    [flush],
  );

  if (!draft) {
    return null;
  }

  const queueCodes = Array.from(
    new Set(issues.filter((issue) => issue.stepId === null).map((issue) => issue.code)),
  );
  const errorOf = (codes: string[]): string | undefined => {
    const found = queueCodes.filter((code) => codes.includes(code));
    return found.length > 0 ? found.map((code) => td(`issues.${code}`, undefined, code)).join(". ") : undefined;
  };
  const nameError = errorOf(NAME_CODES);
  const itemsError = errorOf(ITEMS_CODES);
  const pauseError = errorOf(PAUSE_CODES);
  const repeatError = errorOf(REPEAT_CODES);
  const otherError = errorOf(queueCodes.filter((code) => !PLACED_CODES.includes(code)));

  const hasMissing = scenarioIds !== null && draft.items.some((item) => !scenarioIds.has(item.scenarioId));
  const hasErrors = hasMissing || issues.some((issue) => issue.severity === "error");

  return (
    <>
      {locked ? <div className="queues-banner">{t("queues.lockedBanner")}</div> : null}
      <Section>
        <div className="queues-start">
          <Button
            variant="primary"
            disabled={locked || hasErrors}
            onClick={() => void start({ kind: "queue", id: draft.id })}
          >
            {t("queues.start")}
          </Button>
          {hasErrors ? <span className="queues-start-note">{t("queues.startBlocked")}</span> : null}
        </div>
        {otherError ? <div className="field-error">{otherError}</div> : null}
        <Field label={t("common.name")} error={nameError}>
          <TextInput
            className="queues-name-input"
            value={draft.name}
            maxLength={80}
            disabled={locked}
            invalid={nameError !== undefined}
            onChange={(name) => edit((queue) => ({ ...queue, name }))}
          />
        </Field>
      </Section>
      <Section title={t("queues.itemsTitle")}>
        <QueueItems
          items={draft.items}
          scenarios={scenarios}
          issues={issues}
          disabled={locked}
          onChange={(items) => edit((queue) => ({ ...queue, items }))}
        />
        {itemsError ? <div className="field-error">{itemsError}</div> : null}
        <div className="field-hint">{t("queues.roundsHint")}</div>
      </Section>
      <Section title={t("queues.optionsTitle")}>
        <Field label={t("queues.pauseBetween")} error={pauseError}>
          <PauseInput
            value={draft.pauseBetweenItems}
            disabled={locked}
            invalid={pauseError !== undefined}
            onChange={(pauseBetweenItems) => edit((queue) => ({ ...queue, pauseBetweenItems }))}
          />
        </Field>
        <Field label={t("queues.repeat")} error={repeatError}>
          <RepeatModeInput
            value={draft.repeat}
            disabled={locked}
            invalid={repeatError !== undefined}
            onChange={(repeat) => edit((queue) => ({ ...queue, repeat }))}
          />
        </Field>
        <Field label={t("queues.onFailure")} hint={t("queues.failureHint")}>
          <Select<OnItemFailure>
            value={draft.onItemFailure}
            disabled={locked}
            options={[
              { value: "stop", label: t("queues.onFailureStop") },
              { value: "next", label: t("queues.onFailureNext") },
            ]}
            onChange={(onItemFailure) => edit((queue) => ({ ...queue, onItemFailure }))}
          />
        </Field>
        <ul className="queues-notes">
          <li>{t("queues.stopHint")}</li>
          <li>{t("queues.countdownHint")}</li>
        </ul>
      </Section>
    </>
  );
}
