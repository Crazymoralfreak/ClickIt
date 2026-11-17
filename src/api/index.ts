// Typed wrappers over the Rust commands. Every call rejects with an `AppError`.

import { invoke } from "@tauri-apps/api/core";
import type {
  AppError,
  AppInfo,
  Issue,
  JournalEntry,
  RegionPurpose,
  RunStatus,
  WindowInfo,
} from "../types/runtime";
import type {
  Queue,
  QueueSummary,
  RunTarget,
  Scenario,
  ScenarioSummary,
  ScheduleRule,
  Settings,
  WindowTarget,
} from "../types/scenario";

export function isAppError(value: unknown): value is AppError {
  return typeof value === "object" && value !== null && typeof (value as AppError).code === "string";
}

/** Turns anything thrown by `invoke` into an `AppError`. */
export function toAppError(value: unknown): AppError {
  if (isAppError(value)) {
    return value;
  }
  return { code: "unknown", params: { message: String(value) } };
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  appQuit: () => invoke<void>("app_quit"),

  settingsGet: () => invoke<Settings>("settings_get"),
  /** Saves and applies settings (hotkeys, language of the tray); returns what was stored. */
  settingsSet: (settings: Settings) => invoke<Settings>("settings_set", { settings }),
  autostartGet: () => invoke<boolean>("autostart_get"),
  autostartSet: (enabled: boolean) => invoke<void>("autostart_set", { enabled }),

  scenarioList: () => invoke<ScenarioSummary[]>("scenario_list"),
  scenarioGet: (id: string) => invoke<Scenario>("scenario_get", { id }),
  scenarioCreate: (name: string) => invoke<Scenario>("scenario_create", { name }),
  /** Rejects with `busy` while this scenario is running or being recorded. */
  scenarioSave: (scenario: Scenario) => invoke<void>("scenario_save", { scenario }),
  scenarioDuplicate: (id: string, name: string) => invoke<Scenario>("scenario_duplicate", { id, name }),
  scenarioDelete: (id: string) => invoke<void>("scenario_delete", { id }),
  /** Checks an unsaved scenario; template files are looked up in its folder. */
  scenarioValidate: (scenario: Scenario) => invoke<Issue[]>("scenario_validate", { scenario }),
  /** Opens a save dialog; resolves to the written path or null when cancelled. */
  scenarioExport: (id: string) => invoke<string | null>("scenario_export", { id }),
  /** Opens a file dialog; resolves to the imported scenario or null when cancelled. */
  scenarioImport: () => invoke<Scenario | null>("scenario_import"),
  /** Data URL of a template image of the scenario. */
  templatePreview: (scenarioId: string, file: string) =>
    invoke<string>("template_preview", { scenarioId, file }),

  queueList: () => invoke<QueueSummary[]>("queue_list"),
  queueGet: (id: string) => invoke<Queue>("queue_get", { id }),
  queueCreate: (name: string) => invoke<Queue>("queue_create", { name }),
  queueSave: (queue: Queue) => invoke<void>("queue_save", { queue }),
  queueDuplicate: (id: string, name: string) => invoke<Queue>("queue_duplicate", { id, name }),
  queueDelete: (id: string) => invoke<void>("queue_delete", { id }),
  queueValidate: (queue: Queue) => invoke<Issue[]>("queue_validate", { queue }),

  scheduleGet: () => invoke<ScheduleRule[]>("schedule_get"),
  scheduleSet: (rules: ScheduleRule[]) => invoke<void>("schedule_set", { rules }),

  /** Manual start with the countdown from settings. Progress arrives as `run:status` events. */
  runStart: (target: RunTarget) => invoke<void>("run_start", { target }),
  /** Stops a run, a countdown or a recording. */
  runStop: () => invoke<void>("run_stop"),
  runStatus: () => invoke<RunStatus>("run_status"),

  /** Hides the main window, shows the badge and records into the scenario until stopped. */
  recordStart: (scenarioId: string) => invoke<void>("record_start", { scenarioId }),
  recordStop: () => invoke<void>("record_stop"),

  /** Arms the capture hotkey; the result arrives as one `capture:point` event.
   *  Pass the window target to also get client coordinates. */
  captureStart: (target: WindowTarget | null) => invoke<void>("capture_start", { target }),
  captureCancel: () => invoke<void>("capture_cancel"),

  /** Opens the region overlay; the result arrives as `region:done` or `region:cancelled`. */
  regionStart: (scenarioId: string, purpose: RegionPurpose, target: WindowTarget | null) =>
    invoke<void>("region_start", { scenarioId, purpose, target }),
  /** Overlay only: remembers the cursor position as the first corner. */
  regionMark: () => invoke<void>("region_mark"),
  /** Overlay only: takes the cursor position as the second corner and finishes. */
  regionFinish: () => invoke<void>("region_finish"),
  regionCancel: () => invoke<void>("region_cancel"),

  journalGet: () => invoke<JournalEntry[]>("journal_get"),
  /** Visible top-level windows, for picking a window target. */
  windowsList: () => invoke<WindowInfo[]>("windows_list"),
};
