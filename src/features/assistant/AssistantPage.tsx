import { useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { useAppEvent } from "../../api/events";
import { useApp } from "../../app/context";
import { useT, type TKey } from "../../i18n";
import type { AgentEntry, AgentRole, DialogEntry, DialogRole, HarnessDialog, McpSnippet } from "../../types/runtime";
import { Button, PageFrame, Section, Segmented, TextInput } from "../../ui";
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

/** Conversation with the model in the settings, and tool calls from external MCP clients. */
export function AssistantPage() {
  const { t } = useT();
  const { settings, showError, navigate } = useApp();
  const [view, setView] = useState<AssistantView>("local");
  const [entries, setEntries] = useState<AgentEntry[]>([]);
  const [dialogs, setDialogs] = useState<HarnessDialog[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [text, setText] = useState("");
  const [snippet, setSnippet] = useState<McpSnippet | null>(null);
  const [copied, setCopied] = useState<"servers" | "entry" | null>(null);
  const [busy, setBusy] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  const configured = settings.agentEndpoint.trim() !== "" && settings.agentModel.trim() !== "";
  const selected = dialogs.find((dialog) => dialog.id === selectedId) ?? dialogs[dialogs.length - 1] ?? null;

  useEffect(() => {
    api
      .agentHistory()
      .then(setEntries)
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

  useAppEvent("agent:entry", (entry) => {
    setEntries((current) => [...current, entry]);
    setBusy(true);
  });
  useAppEvent("agent:idle", () => setBusy(false));
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
  }, [view, shownCount, selected?.id]);

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
    if (!message || busy || !configured || view !== "local") {
      return;
    }
    setText("");
    setBusy(true);
    void api.agentSend(message).catch((error: unknown) => {
      setBusy(false);
      showError(error);
    });
  };

  return (
    <PageFrame title={t("nav.assistant")}>
      <Section title={t("agent.connectTitle")}>
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
        {view === "external" && dialogs.length > 1 ? (
          <Segmented
            value={String(selected?.id ?? "")}
            onChange={(value) => setSelectedId(Number(value))}
            options={dialogs.map((dialog) => ({
              value: String(dialog.id),
              label: sessionLabel(dialog, dialogs),
            }))}
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
            disabled={!configured || busy}
          />
          <Button type="submit" variant="primary" disabled={!configured || busy || text.trim() === ""}>
            {busy ? t("agent.running") : t("agent.send")}
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
