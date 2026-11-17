import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, toAppError } from "./api";
import { useAppEvent } from "./api/events";
import {
  AppContext,
  type AppContextValue,
  type AskNameOptions,
  type ConfirmOptions,
  type Page,
} from "./app/context";
import { AboutPage } from "./features/help/AboutPage";
import { HelpPage } from "./features/help/HelpPage";
import { JournalPage } from "./features/journal/JournalPage";
import { QueuesPage } from "./features/queues/QueuesPage";
import { RunBar } from "./features/run/RunBar";
import { ScenariosPage } from "./features/scenarios/ScenariosPage";
import { SchedulePage } from "./features/schedule/SchedulePage";
import { SettingsPage } from "./features/settings/SettingsPage";
import { I18nProvider, translate, useT, type TKey } from "./i18n";
import type { AppError, AppInfo, DataKind, RunStatus } from "./types/runtime";
import type { Language, RunTarget, Settings } from "./types/scenario";
import { Button, Modal, TextInput, cx } from "./ui";

interface Boot {
  info: AppInfo;
  settings: Settings;
  status: RunStatus;
}

export function App() {
  const [boot, setBoot] = useState<Boot | null>(null);
  const [failure, setFailure] = useState<AppError | null>(null);

  useEffect(() => {
    Promise.all([api.appInfo(), api.settingsGet(), api.runStatus()])
      .then(([info, settings, status]) => setBoot({ info, settings, status }))
      .catch((error: unknown) => setFailure(toAppError(error)));
  }, []);

  if (failure) {
    const params = failure.params ?? {};
    return <div className="boot boot-error">{translate("en", `errors.${failure.code}`, params, failure.code)}</div>;
  }
  if (!boot) {
    return <div className="boot" />;
  }
  return <MainWindow boot={boot} />;
}

interface Toast {
  id: number;
  text: string;
  kind: "info" | "error";
}

type Dialog =
  | { kind: "confirm"; options: ConfirmOptions; resolve: (ok: boolean) => void }
  | { kind: "name"; options: AskNameOptions; resolve: (name: string | null) => void };

const TOAST_MS = 6000;

const NAV: ReadonlyArray<{ page: Page; label: TKey }> = [
  { page: "scenarios", label: "nav.scenarios" },
  { page: "queues", label: "nav.queues" },
  { page: "schedule", label: "nav.schedule" },
  { page: "journal", label: "nav.journal" },
  { page: "settings", label: "nav.settings" },
  { page: "help", label: "nav.help" },
  { page: "about", label: "nav.about" },
];

function MainWindow({ boot }: { boot: Boot }) {
  const [settings, setSettings] = useState(boot.settings);
  const [status, setStatus] = useState(boot.status);
  const [page, setPage] = useState<Page>("scenarios");
  const [selectedScenarioId, setSelectedScenarioId] = useState<string | null>(
    boot.settings.current?.kind === "scenario" ? boot.settings.current.id : null,
  );
  const [selectedQueueId, setSelectedQueueId] = useState<string | null>(
    boot.settings.current?.kind === "queue" ? boot.settings.current.id : null,
  );
  const [dataVersion, setDataVersion] = useState<Record<DataKind, number>>({
    scenarios: 0,
    queues: 0,
    schedule: 0,
    settings: 0,
  });
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [dialog, setDialog] = useState<Dialog | null>(null);

  const language: Language = settings.language ?? boot.info.systemLanguage;
  const languageRef = useRef(language);
  languageRef.current = language;
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const flushes = useRef(new Set<() => Promise<void>>());
  const toastId = useRef(0);

  const notify = useCallback((text: string, kind: "info" | "error" = "info") => {
    const id = ++toastId.current;
    setToasts((list) => [...list.slice(-3), { id, text, kind }]);
    window.setTimeout(() => setToasts((list) => list.filter((toast) => toast.id !== id)), TOAST_MS);
  }, []);

  const errorText = useCallback((error: AppError): string => {
    const lang = languageRef.current;
    const fallback = translate(lang, "errors.unknown", { message: error.code });
    return translate(lang, `errors.${error.code}`, error.params, fallback);
  }, []);

  const showError = useCallback(
    (error: unknown) => notify(errorText(toAppError(error)), "error"),
    [notify, errorText],
  );

  const touch = useCallback((kind: DataKind) => {
    setDataVersion((versions) => ({ ...versions, [kind]: versions[kind] + 1 }));
  }, []);

  const saveSettings = useCallback(
    async (next: Settings): Promise<boolean> => {
      try {
        const stored = await api.settingsSet(next);
        settingsRef.current = stored;
        setSettings(stored);
        return true;
      } catch (error) {
        showError(error);
        return false;
      }
    },
    [showError],
  );

  const registerFlush = useCallback((flush: () => Promise<void>) => {
    flushes.current.add(flush);
    return () => {
      flushes.current.delete(flush);
    };
  }, []);

  const flushAll = useCallback(async () => {
    await Promise.all([...flushes.current].map((flush) => flush()));
  }, []);

  const start = useCallback(
    async (target?: RunTarget) => {
      try {
        await flushAll();
        const current = settingsRef.current.current;
        const chosen = target ?? current;
        if (!chosen) {
          notify(translate(languageRef.current, "errors.nothingToRun"), "error");
          return;
        }
        if (!current || current.kind !== chosen.kind || current.id !== chosen.id) {
          const saved = await saveSettings({ ...settingsRef.current, current: chosen });
          if (!saved) {
            return;
          }
        }
        await api.runStart(chosen);
      } catch (error) {
        showError(error);
      }
    },
    [flushAll, notify, saveSettings, showError],
  );

  const stop = useCallback(async () => {
    try {
      await api.runStop();
    } catch (error) {
      showError(error);
    }
  }, [showError]);

  const confirm = useCallback(
    (options: ConfirmOptions) =>
      new Promise<boolean>((resolve) => setDialog({ kind: "confirm", options, resolve })),
    [],
  );

  const askName = useCallback(
    (options: AskNameOptions) =>
      new Promise<string | null>((resolve) => setDialog({ kind: "name", options, resolve })),
    [],
  );

  useAppEvent("run:status", setStatus);

  useAppEvent("run:finished", (finished) => {
    const lang = languageRef.current;
    const name = finished.target.name;
    switch (finished.outcome) {
      case "completed":
        notify(translate(lang, "run.finishedCompleted", { name }));
        break;
      case "stopped":
        notify(
          translate(lang, "run.finishedStopped", {
            name,
            reason: finished.reason ? errorText(finished.reason) : "",
          }),
        );
        break;
      case "failed":
        notify(
          translate(lang, "run.finishedFailed", {
            name,
            reason: finished.reason ? errorText(finished.reason) : "",
          }),
          "error",
        );
        break;
      default: {
        const unreachable: never = finished.outcome;
        throw new Error(`Unknown outcome: ${String(unreachable)}`);
      }
    }
  });

  useAppEvent("record:finished", (finished) => {
    const lang = languageRef.current;
    touch("scenarios");
    if (finished.added === 0) {
      notify(translate(lang, "run.recordNothing"));
      return;
    }
    const added = { added: finished.added };
    switch (finished.reason) {
      case "user":
      case "exit":
        notify(translate(lang, "run.recordFinished", added));
        break;
      case "timeLimit":
        notify(translate(lang, "run.recordTimeLimit", added), "error");
        break;
      case "stepLimit":
        notify(translate(lang, "run.recordStepLimit", added), "error");
        break;
      default: {
        const unreachable: never = finished.reason;
        throw new Error(`Unknown record reason: ${String(unreachable)}`);
      }
    }
  });

  useAppEvent("data:changed", ({ kind }) => {
    touch(kind);
    if (kind === "settings") {
      api
        .settingsGet()
        .then((stored) => {
          settingsRef.current = stored;
          setSettings(stored);
        })
        .catch(showError);
    }
  });

  const value = useMemo<AppContextValue>(
    () => ({
      info: boot.info,
      settings,
      saveSettings,
      status,
      locked: status.state !== "idle",
      start,
      stop,
      page,
      navigate: setPage,
      selectedScenarioId,
      selectScenario: setSelectedScenarioId,
      selectedQueueId,
      selectQueue: setSelectedQueueId,
      dataVersion,
      touch,
      notify,
      showError,
      confirm,
      askName,
      registerFlush,
      flushAll,
    }),
    [
      boot.info,
      settings,
      saveSettings,
      status,
      start,
      stop,
      page,
      selectedScenarioId,
      selectedQueueId,
      dataVersion,
      touch,
      notify,
      showError,
      confirm,
      askName,
      registerFlush,
      flushAll,
    ],
  );

  return (
    <I18nProvider language={language}>
      <AppContext.Provider value={value}>
        <div className="app">
          <Sidebar page={page} onNavigate={setPage} />
          <main className="app-main">
            <PageView page={page} />
            <RunBar />
          </main>
          <div className="toasts">
            {toasts.map((toast) => (
              <div
                key={toast.id}
                className={cx("toast", toast.kind === "error" && "toast-error")}
                onClick={() => setToasts((list) => list.filter((item) => item.id !== toast.id))}
              >
                {toast.text}
              </div>
            ))}
          </div>
          {dialog ? <DialogHost dialog={dialog} onDone={() => setDialog(null)} /> : null}
        </div>
      </AppContext.Provider>
    </I18nProvider>
  );
}

function Sidebar({ page, onNavigate }: { page: Page; onNavigate: (page: Page) => void }) {
  const { t } = useT();
  return (
    <nav className="sidebar">
      <div className="sidebar-brand">ClickIt</div>
      {NAV.map((item) => (
        <button
          key={item.page}
          type="button"
          className={cx("sidebar-item", item.page === page && "sidebar-item-active")}
          onClick={() => onNavigate(item.page)}
        >
          {t(item.label)}
        </button>
      ))}
    </nav>
  );
}

function PageView({ page }: { page: Page }) {
  switch (page) {
    case "scenarios":
      return <ScenariosPage />;
    case "queues":
      return <QueuesPage />;
    case "schedule":
      return <SchedulePage />;
    case "settings":
      return <SettingsPage />;
    case "journal":
      return <JournalPage />;
    case "help":
      return <HelpPage />;
    case "about":
      return <AboutPage />;
    default: {
      const unreachable: never = page;
      throw new Error(`Unknown page: ${String(unreachable)}`);
    }
  }
}

function DialogHost({ dialog, onDone }: { dialog: Dialog; onDone: () => void }) {
  const { t } = useT();
  const [name, setName] = useState(dialog.kind === "name" ? dialog.options.initial : "");

  switch (dialog.kind) {
    case "confirm": {
      const finish = (ok: boolean) => {
        dialog.resolve(ok);
        onDone();
      };
      return (
        <Modal
          title={dialog.options.title}
          onClose={() => finish(false)}
          footer={
            <>
              <Button onClick={() => finish(false)}>{t("common.cancel")}</Button>
              <Button variant={dialog.options.danger ? "danger" : "primary"} autoFocus onClick={() => finish(true)}>
                {dialog.options.okLabel ?? t("common.ok")}
              </Button>
            </>
          }
        >
          {dialog.options.text ? <p className="modal-text">{dialog.options.text}</p> : null}
        </Modal>
      );
    }
    case "name": {
      const finish = (result: string | null) => {
        dialog.resolve(result);
        onDone();
      };
      const trimmed = name.trim();
      return (
        <Modal
          title={dialog.options.title}
          onClose={() => finish(null)}
          footer={
            <>
              <Button onClick={() => finish(null)}>{t("common.cancel")}</Button>
              <Button variant="primary" disabled={trimmed === ""} onClick={() => finish(trimmed)}>
                {dialog.options.okLabel ?? t("common.ok")}
              </Button>
            </>
          }
        >
          <TextInput
            value={name}
            onChange={setName}
            autoFocus
            maxLength={80}
            placeholder={t("common.name")}
            onKeyDown={(event) => {
              if (event.key === "Enter" && trimmed !== "") {
                finish(trimmed);
              }
            }}
          />
        </Modal>
      );
    }
    default: {
      const unreachable: never = dialog;
      throw new Error(`Unknown dialog: ${String(unreachable)}`);
    }
  }
}
