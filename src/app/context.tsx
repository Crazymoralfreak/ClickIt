// App-wide state shared by every screen of the main window.

import { createContext, useContext } from "react";
import type { AppInfo, DataKind, RunStatus, UpdateStatus } from "../types/runtime";
import type { RunTarget, Settings } from "../types/scenario";

export type Page = "scenarios" | "queues" | "schedule" | "settings" | "journal" | "help" | "about";

export interface ConfirmOptions {
  title: string;
  text?: string;
  okLabel?: string;
  danger?: boolean;
}

export interface AskNameOptions {
  title: string;
  initial: string;
  okLabel?: string;
}

export interface AppContextValue {
  info: AppInfo;
  settings: Settings;
  /** Saves and applies settings. Shows the error and resolves to false when the core rejects them. */
  saveSettings: (next: Settings) => Promise<boolean>;

  /** Live state of the run or recording. */
  status: RunStatus;
  /** True while something runs or records: scenarios and queues must not be edited. */
  locked: boolean;
  /** Saves pending edits, optionally makes `target` the current one, then starts it. */
  start: (target?: RunTarget) => Promise<void>;
  stop: () => Promise<void>;

  /** What is known about a newer version in this session. */
  update: UpdateStatus;

  page: Page;
  navigate: (page: Page) => void;
  selectedScenarioId: string | null;
  selectScenario: (id: string | null) => void;
  selectedQueueId: string | null;
  selectQueue: (id: string | null) => void;

  /** Counters that grow whenever data of that kind changes on disk; use them as effect dependencies. */
  dataVersion: Record<DataKind, number>;
  /** Tells the other screens that this screen changed data of that kind. */
  touch: (kind: DataKind) => void;

  notify: (text: string, kind?: "info" | "error") => void;
  /** Shows anything thrown by an `api` call as a localized toast. */
  showError: (error: unknown) => void;
  confirm: (options: ConfirmOptions) => Promise<boolean>;
  /** Asks for a non-empty name; resolves to null when cancelled. */
  askName: (options: AskNameOptions) => Promise<string | null>;

  /** Screens with debounced saves register a flush that runs before a start or a recording. */
  registerFlush: (flush: () => Promise<void>) => () => void;
  flushAll: () => Promise<void>;
}

export const AppContext = createContext<AppContextValue | null>(null);

export function useApp(): AppContextValue {
  const value = useContext(AppContext);
  if (!value) {
    throw new Error("useApp must be used inside the main window");
  }
  return value;
}

export const IDLE_STATUS: RunStatus = {
  state: "idle",
  target: null,
  countdownLeft: null,
  scenarioId: null,
  scenarioName: null,
  round: 0,
  roundsTotal: null,
  stepId: null,
  stepLabel: null,
  loopPass: null,
  loopTotal: null,
  stepRepeat: null,
  stepRepeatTotal: null,
  queue: null,
  blockedProcess: null,
  recording: null,
};
