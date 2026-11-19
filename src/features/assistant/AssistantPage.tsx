import { useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { useAppEvent } from "../../api/events";
import { useApp } from "../../app/context";
import { useT, type TKey } from "../../i18n";
import type {
  AgentEntry,
  AgentRole,
  ChatInfo,
  DialogEntry,
  DialogRole,
  HarnessDialog,
  McpSnippet,
} from "../../types/runtime";
import { Button, PageFrame, Section, Segmented, TextInput } from "../../ui";
import { TabStrip } from "./TabStrip";
import "./assistant.css";

type AssistantView = "local" | "external";

function roleLabel(role: AgentRole): TKey {
  switch (role) {
    case "user":
      return "agent.role.user";
    case "assistant":
      return "agent.role.assistant";
    case "tool":
      return "agent.role.tool";
    default: {
      const unreachable: never = role;
      throw new Error(`Unknown role: ${String(unreachable)}`);
    }
  }
}

function speaker(role: DialogRole, client: string, resultLabel: string): string {
  switch (role) {
    case "call":
      return client;
    case "result":
      return resultLabel;
    default: {
      const unreachable: never = role;
      throw new Error(`Unknown role: ${String(unreachable)}`);
    }
  }
}

function sessionLabel(dialog: HarnessDialog, dialogs: HarnessDialog[]): string {
  const same = dialogs.filter((item) => item.client === dialog.client);
  if (same.length < 2) {
    return dialog.client;
  }
  const index = same.findIndex((item) => item.id === dialog.id) + 1;
  return `${dialog.client} · ${index}`;
}

/** Adds lines that are not in the list yet: an event and the history request may bring the same line. */
function mergeEntries(known: AgentEntry[], added: AgentEntry[]): AgentEntry[] {
  const ids = new Set(known.map((entry) => entry.id));
  const fresh = added.filter((entry) => !ids.has(entry.id));
  return fresh.length === 0 ? known : [...known, ...fresh].sort((a, b) => a.id - b.id);
}

/** Chats with the model in the settings, and tool calls from external MCP clients. */
export function AssistantPage() {
  const { t } = useT();
  const { settings, showError, navigate, confirm } = useApp();
  const [view, setView] = useState<AssistantView>("local");
  const [chats, setChats] = useState<ChatInfo[]>([]);
  const [chatId, setChatId] = useState<string | null>(null);
  /** Lines of the chats opened in this session, by chat id. */
  const [lines, setLines] = useState<Record<string, AgentEntry[]>>({});
  /** The chat the model is answering in. One answer at a time. */
  const [busyChat, setBusyChat] = useState<string | null>(null);
  const [dialogs, setDialogs] = useState<HarnessDialog[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [text, setText] = useState("");
  const [snippet, setSnippet] = useState<McpSnippet | null>(null);
  const [copied, setCopied] = useState<"servers" | "entry" | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const configured = settings.agentEndpoint.trim() !== "" && settings.agentModel.trim() !== "";
  const selected = dialogs.find((dialog) => dialog.id === selectedId) ?? dialogs[dialogs.length - 1] ?? null;
  const chat = chats.find((item) => item.id === chatId) ?? chats[chats.length - 1] ?? null;
  const openChatId = chat?.id ?? null;
  const entries = openChatId === null ? [] : (lines[openChatId] ?? []);
  const busyHere = busyChat !== null && busyChat === openChatId;

  useEffect(() => {
    api
      .agentChats()
      .then((found) => {
        setChats(found);
        setBusyChat(found.find((item) => item.busy)?.id ?? null);
      })
      .catch(showError);
    api
      .harnessDialogs()
      .then(setDialogs)
      .catch(showError);
    api
      .harnessMcpSnippet()
      .then(setSnippet)
      .catch(showError);
  }, [showError]);

  // The lines of a chat are read when it is first opened.
  useEffect(() => {
    if (openChatId === null) {
      return;
    }
    let cancelled = false;
    api
      .agentHistory(openChatId)
      .then((history) => {
        if (!cancelled) {
          setLines((current) => ({ ...current, [openChatId]: mergeEntries(history, current[openChatId] ?? []) }));
        }
      })
      .catch(showError);
    return () => {
      cancelled = true;
    };
  }, [openChatId, showError]);

  useAppEvent("agent:entry", ({ chatId: id, entry }) => {
    setLines((current) => ({ ...current, [id]: mergeEntries(current[id] ?? [], [entry]) }));
    setChats((current) => current.map((item) => (item.id === id ? { ...item, entryCount: item.entryCount + 1 } : item)));
    setBusyChat(id);
  });
  useAppEvent("agent:idle", () => setBusyChat(null));
  useAppEvent("agent:chat", (info) => {
    setChats((current) => current.map((item) => (item.id === info.id ? { ...item, title: info.title } : item)));
  });
  useAppEvent("harness:dialog", (event) => {
    setDialogs((current) => {
      const index = current.findIndex((dialog) => dialog.id === event.id);
      if (index < 0) {
        return [
          ...current,
          { id: event.id, client: event.client, open: event.open, entries: event.entry ? [event.entry] : [] },
        ];
      }
      const next = current.slice();
      const dialog = next[index];
      if (!dialog) {
        return current;
      }
      const entries = event.entry ? [...dialog.entries, event.entry] : dialog.entries;
      next[index] = { ...dialog, client: event.client, open: event.open, entries };
      return next;
    });
  });

  const shownCount = view === "external" ? (selected?.entries.length ?? 0) : entries.length;
  useEffect(() => {
    list.current?.scrollTo({ top: list.current.scrollHeight });
  }, [view, shownCount, selected?.id, openChatId]);

  const copySnippet = (value: string, kind: "servers" | "entry") => {
    const mark = () => {
      setCopied(kind);
      window.setTimeout(() => setCopied((current) => (current === kind ? null : current)), 2000);
    };
    const fallback = () => {
      const area = document.createElement("textarea");
      area.value = value;
      area.setAttribute("readonly", "");
      document.body.appendChild(area);
      area.select();
      const ok = document.execCommand("copy");
      area.remove();
      if (ok) {
        mark();
      }
    };
    if (navigator.clipboard?.writeText) {
      void navigator.clipboard.writeText(value).then(mark).catch(fallback);
      return;
    }
    fallback();
  };

  const send = () => {
    const message = text.trim();
    if (!message || busyChat !== null || !configured || view !== "local" || openChatId === null) {
      return;
    }
    setText("");
    setBusyChat(openChatId);
    void api.agentSend(openChatId, message).catch((error: unknown) => {
      setBusyChat(null);
      showError(error);
    });
  };

  const chatTitle = (item: ChatInfo) => (item.title === "" ? t("agent.chatNew") : item.title);

  const addChat = () => {
    api
      .agentChatCreate()
      .then((created) => {
        setChats((current) => (current.some((item) => item.id === created.id) ? current : [...current, created]));
        setChatId(created.id);
      })
      .catch(showError);
  };

  const closeChat = async (id: string) => {
    const item = chats.find((other) => other.id === id);
    if (!item) {
      return;
    }
    if (item.entryCount > 0) {
      const ok = await confirm({
        title: t("agent.chatDeleteTitle", { name: chatTitle(item) }),
        text: t("agent.chatDeleteText"),
        okLabel: t("common.delete"),
        danger: true,
      });
      if (!ok) {
        return;
      }
    }
    try {
      const left = await api.agentChatDelete(id);
      setChats(left);
      setLines((current) => {
        const next = { ...current };
        delete next[id];
        return next;
      });
      setChatId((current) => (current === id ? null : current));
    } catch (error) {
      showError(error);
    }
  };

  const renameChat = (id: string, title: string) => {
    api
      .agentChatRename(id, title)
      .then((info) => setChats((current) => current.map((item) => (item.id === info.id ? info : item))))
      .catch(showError);
  };

  const closeDialog = (id: number) => {
    void api.harnessDialogForget(id).then(
      () => {
        setDialogs((current) => current.filter((item) => item.id !== id));
        setSelectedId((current) => (current === id ? null : current));
      },
      (error: unknown) => showError(error),
    );
  };

  return (
    <PageFrame title={t("nav.assistant")}>
      <Section title={t("agent.connectTitle")} collapsible defaultOpen={false}>
        <ol className="assistant-steps">
          <li>
            {t("agent.connectRun")}{" "}
            <button type="button" className="link-button" onClick={() => navigate("settings")}>
              {t("nav.settings")}
            </button>
          </li>
          <li>{t("agent.connectClient")}</li>
          <li>{t("agent.connectPath")}</li>
          <li>{t("agent.connectSee")}</li>
        </ol>
        {snippet ? (
          <>
            <textarea className="assistant-snippet" readOnly value={snippet.servers} rows={6} spellCheck={false} />
            <div className="assistant-snippet-actions">
              <Button small onClick={() => copySnippet(snippet.servers, "servers")}>
                {copied === "servers" ? t("agent.connectCopied") : t("agent.connectCopy")}
              </Button>
              <Button small onClick={() => copySnippet(snippet.entry, "entry")}>
                {copied === "entry" ? t("agent.connectCopied") : t("agent.connectCopyEntry")}
              </Button>
            </div>
            {snippet.found ? null : <p className="assistant-hint">{t("agent.connectMissing")}</p>}
          </>
        ) : null}
      </Section>
      <div className="assistant-switch">
        <Segmented
          value={view}
          onChange={setView}
          options={[
            { value: "local", label: t("agent.local") },
            { value: "external", label: t("agent.connections") },
          ]}
        />
        {view === "local" && chats.length > 0 ? (
          <TabStrip
            label={t("agent.chatsLabel")}
            items={chats.map((item) => ({
              id: item.id,
              label: chatTitle(item),
              title: t("agent.chatRename"),
              closeTitle: item.id === busyChat ? t("agent.chatAnswering") : t("agent.chatClose"),
              closeDisabled: item.id === busyChat,
              busy: item.id === busyChat,
            }))}
            activeId={openChatId}
            onSelect={setChatId}
            onClose={(id) => void closeChat(id)}
            onAdd={addChat}
            addTitle={t("agent.chatNew")}
            onRename={renameChat}
          />
        ) : null}
        {view === "external" && dialogs.length > 0 ? (
          <TabStrip
            label={t("agent.connectionsLabel")}
            items={dialogs.map((dialog) => ({
              id: String(dialog.id),
              label: sessionLabel(dialog, dialogs),
              closeTitle: dialog.open ? t("agent.connectionOpen") : t("agent.connectionForget"),
              closeDisabled: dialog.open,
            }))}
            activeId={selected ? String(selected.id) : null}
            onSelect={(id) => setSelectedId(Number(id))}
            onClose={(id) => closeDialog(Number(id))}
          />
        ) : null}
      </div>
      {view === "local" && !configured ? (
        <p className="assistant-hint">
          {t("agent.needSettings")}{" "}
          <button type="button" className="link-button" onClick={() => navigate("settings")}>
            {t("nav.settings")}
          </button>
        </p>
      ) : null}
      {view === "local" && configured ? <p className="assistant-hint">{t("agent.notice")}</p> : null}
      {view === "external" && selected ? (
        <p className="assistant-hint">{t("agent.connectionsNotice", { client: selected.client })}</p>
      ) : null}
      <div className="assistant-log" ref={list}>
        {view === "local" && entries.length === 0 ? <p className="assistant-empty">{t("agent.empty")}</p> : null}
        {view === "external" && dialogs.length === 0 ? (
          <p className="assistant-empty">{t("agent.connectionsEmpty")}</p>
        ) : null}
        {view === "local"
          ? entries.map((entry) => <LocalLine key={entry.id} entry={entry} role={t(roleLabel(entry.role))} />)
          : null}
        {view === "external" && selected
          ? selected.entries.map((entry) => (
              <ExternalLine
                key={entry.id}
                entry={entry}
                speaker={speaker(entry.role, selected.client, t("agent.role.result"))}
              />
            ))
          : null}
      </div>
      {view === "local" ? (
        <form
          className="assistant-form"
          onSubmit={(event) => {
            event.preventDefault();
            send();
          }}
        >
          <TextInput
            value={text}
            onChange={setText}
            placeholder={t("agent.placeholder")}
            disabled={!configured || busyChat !== null}
            title={busyChat !== null && !busyHere ? t("agent.busyElsewhere") : undefined}
          />
          <Button
            type="submit"
            variant="primary"
            disabled={!configured || busyChat !== null || text.trim() === ""}
            title={busyChat !== null && !busyHere ? t("agent.busyElsewhere") : undefined}
          >
            {busyHere ? t("agent.running") : t("agent.send")}
          </Button>
        </form>
      ) : null}
    </PageFrame>
  );
}

function LocalLine({ entry, role }: { entry: AgentEntry; role: string }) {
  return (
    <article className={`assistant-entry assistant-${entry.role}`}>
      <div className="assistant-role">{role}</div>
      <p className="assistant-text">{entry.text}</p>
      {entry.imagePng ? <img className="assistant-shot" alt="" src={`data:image/png;base64,${entry.imagePng}`} /> : null}
    </article>
  );
}

function ExternalLine({ entry, speaker }: { entry: DialogEntry; speaker: string }) {
  return (
    <article className={`assistant-entry assistant-${entry.role}`}>
      <div className="assistant-role">{speaker}</div>
      <p className="assistant-text">{entry.text}</p>
      {entry.imagePng ? <img className="assistant-shot" alt="" src={`data:image/png;base64,${entry.imagePng}`} /> : null}
    </article>
  );
}
