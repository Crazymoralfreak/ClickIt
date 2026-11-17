import { useEffect, useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { Queue, QueueSummary, ScenarioSummary } from "../../types/scenario";
import { Button, EmptyState, PageFrame, cx } from "../../ui";
import { QueueEditor } from "./QueueEditor";
import "./queues.css";

/** Row for the list until the next reload brings the real summary. */
function toSummary(queue: Queue, broken: boolean): QueueSummary {
  return { id: queue.id, name: queue.name, itemCount: queue.items.length, broken };
}

export function QueuesPage() {
  const { t } = useT();
  const { dataVersion, selectedQueueId, selectQueue, touch, showError, confirm, askName, flushAll, locked } =
    useApp();
  const [queues, setQueues] = useState<QueueSummary[] | null>(null);
  const [scenarios, setScenarios] = useState<ScenarioSummary[] | null>(null);
  // Grows when the selected queue was rewritten on disk past the editor, which then reloads.
  const [editorEpoch, setEditorEpoch] = useState(0);

  useEffect(() => {
    let cancelled = false;
    api
      .queueList()
      .then((list) => {
        if (!cancelled) {
          setQueues(list);
        }
      })
      .catch(showError);
    return () => {
      cancelled = true;
    };
  }, [dataVersion.queues, dataVersion.scenarios, showError]);

  useEffect(() => {
    let cancelled = false;
    api
      .scenarioList()
      .then((list) => {
        if (!cancelled) {
          setScenarios(list);
        }
      })
      .catch(showError);
    return () => {
      cancelled = true;
    };
  }, [dataVersion.scenarios, showError]);

  useEffect(() => {
    if (queues === null || queues.some((queue) => queue.id === selectedQueueId)) {
      return;
    }
    const fallback = queues.length > 0 ? queues[0].id : null;
    if (fallback !== selectedQueueId) {
      selectQueue(fallback);
    }
  }, [queues, selectedQueueId, selectQueue]);

  const selected = queues?.find((queue) => queue.id === selectedQueueId) ?? null;

  const create = async () => {
    const name = await askName({
      title: t("queues.createTitle"),
      initial: t("queues.defaultName"),
      okLabel: t("common.create"),
    });
    if (name === null) {
      return;
    }
    try {
      const created = await api.queueCreate(name);
      setQueues((list) => (list ? [...list, toSummary(created, false)] : list));
      selectQueue(created.id);
      touch("queues");
    } catch (error) {
      showError(error);
    }
  };

  const rename = async (queue: QueueSummary) => {
    const name = await askName({
      title: t("queues.renameTitle"),
      initial: queue.name,
      okLabel: t("common.rename"),
    });
    if (name === null || name === queue.name) {
      return;
    }
    try {
      // The editor may hold edits that are not on disk yet; they must not be lost or written later.
      await flushAll();
      const stored = await api.queueGet(queue.id);
      await api.queueSave({ ...stored, name });
      touch("queues");
      setEditorEpoch((epoch) => epoch + 1);
    } catch (error) {
      showError(error);
    }
  };

  const duplicate = async (queue: QueueSummary) => {
    try {
      await flushAll();
      const copy = await api.queueDuplicate(queue.id, t("common.copySuffix", { name: queue.name }));
      setQueues((list) => (list ? [...list, toSummary(copy, queue.broken)] : list));
      selectQueue(copy.id);
      touch("queues");
    } catch (error) {
      showError(error);
    }
  };

  const remove = async (queue: QueueSummary) => {
    const ok = await confirm({
      title: t("queues.deleteTitle", { name: queue.name }),
      text: t("queues.deleteText"),
      okLabel: t("common.delete"),
      danger: true,
    });
    if (!ok) {
      return;
    }
    try {
      // Otherwise the editor would write its pending edits after the delete.
      await flushAll();
      await api.queueDelete(queue.id);
      setQueues((list) => (list ? list.filter((item) => item.id !== queue.id) : list));
      touch("queues");
    } catch (error) {
      showError(error);
    }
  };

  const createButton = (
    <Button variant="primary" disabled={locked} onClick={() => void create()}>
      {t("queues.create")}
    </Button>
  );

  if (queues === null) {
    return <PageFrame title={t("nav.queues")}>{null}</PageFrame>;
  }

  if (queues.length === 0) {
    return (
      <PageFrame title={t("nav.queues")}>
        <EmptyState title={t("queues.emptyTitle")} text={t("queues.emptyText")} action={createButton} />
      </PageFrame>
    );
  }

  return (
    <PageFrame
      title={t("nav.queues")}
      actions={
        <>
          <Button disabled={locked || !selected} onClick={() => selected && void rename(selected)}>
            {t("common.rename")}
          </Button>
          <Button disabled={locked || !selected} onClick={() => selected && void duplicate(selected)}>
            {t("common.duplicate")}
          </Button>
          <Button disabled={locked || !selected} onClick={() => selected && void remove(selected)}>
            {t("common.delete")}
          </Button>
          {createButton}
        </>
      }
    >
      <div className="queues-layout">
        <div className="queues-list">
          {queues.map((queue) => (
            <button
              key={queue.id}
              type="button"
              className={cx("queues-list-item", queue.id === selectedQueueId && "queues-list-item-active")}
              aria-current={queue.id === selectedQueueId}
              onClick={() => selectQueue(queue.id)}
            >
              <span className="queues-list-name">{queue.name}</span>
              <span className="queues-list-meta">
                {queue.broken ? (
                  <span
                    className="queues-list-broken"
                    role="img"
                    title={t("queues.brokenMarker")}
                    aria-label={t("queues.brokenMarker")}
                  >
                    !
                  </span>
                ) : null}
                {t("queues.itemCount", { count: queue.itemCount })}
              </span>
            </button>
          ))}
        </div>
        <div className="queues-editor">
          {selected ? (
            <QueueEditor key={`${selected.id}:${editorEpoch}`} queueId={selected.id} scenarios={scenarios} />
          ) : null}
        </div>
      </div>
    </PageFrame>
  );
}
