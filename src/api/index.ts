// Typed wrappers over the Rust commands. Every call rejects with an `AppError`.

import { invoke } from "@tauri-apps/api/core";
import type {
  AgentEntry,
  AppError,
  ChatInfo,
  HarnessDialog,
  McpSnippet,
  AppInfo,
  CheckResult,
  CleanupInfo,
  CleanupKind,
  Extracted,
  Issue,
  JournalEntry,
  RegionPurpose,
  RunOptions,
  RunStatus,
  RunTrace,
  ScenarioMeta,
  UpdateStatus,
  WindowInfo,
} from "../types/runtime";
import type {
  Condition,
  Folder,
  GraphEdge,
  GraphNode,
  Queue,
  QueueSummary,
  RunTarget,
  Scenario,
  ScenarioFormat,
  ScenarioMode,
  ScenarioSummary,
  ScheduleRule,
  Settings,
  Snippet,
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

  updateStatus: () => invoke<UpdateStatus>("update_status"),
  /** Starts a check for updates; the progress arrives as `update:status` events. */
  updateCheck: () => invoke<void>("update_check"),

  scenarioList: () => invoke<ScenarioSummary[]>("scenario_list"),
  scenarioGet: (id: string) => invoke<Scenario>("scenario_get", { id }),
  /** A canvas scenario starts with its start block. `folderId` puts the new scenario into a folder. */
  scenarioCreate: (name: string, format: ScenarioFormat = "list", folderId: string | null = null) =>
    invoke<Scenario>("scenario_create", { name, format, folderId }),
  /** Puts a scenario into a folder; `null` is the top of the list. */
  scenarioMove: (id: string, folderId: string | null) => invoke<void>("scenario_move", { id, folderId }),

  /** Folders of the list of scenarios; the ones with the same parent come in the order they are shown. */
  folderList: () => invoke<Folder[]>("folder_list"),
  /** Rejects with `folderTooDeep` when the parent is already at the deepest level. */
  folderCreate: (name: string, parentId: string | null = null) =>
    invoke<Folder>("folder_create", { name, parentId }),
  folderRename: (id: string, name: string) => invoke<void>("folder_rename", { id, name }),
  /** Removes the folder only: its folders and scenarios go one level up. */
  folderDelete: (id: string) => invoke<void>("folder_delete", { id }),
  folderSetCollapsed: (id: string, collapsed: boolean) =>
    invoke<void>("folder_set_collapsed", { id, collapsed }),
  /** Puts a folder into `parentId` in front of `beforeId` (after the last one when `null`).
   *  Resolves to the folders in their new order. */
  folderMove: (id: string, parentId: string | null, beforeId: string | null) =>
    invoke<Folder[]>("folder_move", { id, parentId, beforeId }),
  scenarioMeta: (id: string) => invoke<ScenarioMeta>("scenario_meta", { id }),
  /** Turns a list into a graph. With `copyName` the list stays and a new scenario gets the graph. */
  scenarioToCanvas: (id: string, copyName: string | null = null) =>
    invoke<Scenario>("scenario_to_canvas", { id, copyName }),
  /** Brings back the list a graph was made from; what was changed on the canvas is lost. */
  scenarioToList: (id: string) => invoke<Scenario>("scenario_to_list", { id }),
  /** Moves blocks of a saved graph into a new scenario and leaves a "Run scenario" block. */
  scenarioExtract: (id: string, nodeIds: string[], name: string) =>
    invoke<Extracted>("scenario_extract", { id, nodeIds, name }),
  /** "Check now": one look at a condition, the way a run does it. Nothing is clicked. */
  conditionCheck: (scenarioId: string, mode: ScenarioMode, target: WindowTarget, condition: Condition) =>
    invoke<CheckResult>("condition_check", { scenarioId, mode, target, condition }),

  /** Opens a file dialog for the program of a "Command" step; resolves to null when cancelled. */
  commandPickFile: () => invoke<string | null>("command_pick_file"),
  /** Opens a folder dialog for the working folder of a "Command" step; resolves to null when cancelled. */
  commandPickFolder: () => invoke<string | null>("command_pick_folder"),

  snippetList: () => invoke<Snippet[]>("snippet_list"),
  /** Saves blocks with their links and the template images of the scenario they refer to. */
  snippetSave: (scenarioId: string, name: string, nodes: GraphNode[], edges: GraphEdge[]) =>
    invoke<Snippet>("snippet_save", { scenarioId, name, nodes, edges }),
  /** Copies the template images of a snippet into the scenario and returns its blocks. */
  snippetUse: (id: string, scenarioId: string) => invoke<Snippet>("snippet_use", { id, scenarioId }),
  snippetDelete: (id: string) => invoke<void>("snippet_delete", { id }),

  /** Rejects with `busy` while this scenario is running or being recorded.
   *  Returns the scenario as stored, including the new revision. */
  scenarioSave: (scenario: Scenario) => invoke<Scenario>("scenario_save", { scenario }),
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
  runStart: (target: RunTarget, options: RunOptions = {}) =>
    invoke<void>("run_start", { target, startNode: options.startNode ?? null, debug: options.debug ?? null }),
  /** Runs the block a run is paused on and pauses on the next one. */
  runStep: () => invoke<void>("run_step"),
  /** Lets a paused run go on until the next breakpoint. */
  runContinue: () => invoke<void>("run_continue"),
  /** The trace of the last run of a canvas scenario, finished or still going. */
  runTrace: (id: string) => invoke<RunTrace | null>("run_trace", { id }),
  runTraceClear: (id: string) => invoke<void>("run_trace_clear", { id }),
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

  harnessReply: (requestId: number, accepted: boolean) =>
    invoke<void>("harness_reply", { requestId, accepted }),
  windowShow: () => invoke<void>("window_show"),

  agentInfo: () => invoke<{ keySet: boolean }>("agent_info"),
  agentKeySet: (key: string) => invoke<void>("agent_key_set", { key }),
  /** Chats with the model, oldest first. There is always at least one. */
  agentChats: () => invoke<ChatInfo[]>("agent_chats"),
  /** A chat that is still empty is handed out again instead of a second one. */
  agentChatCreate: () => invoke<ChatInfo>("agent_chat_create"),
  /** Resolves to the chats that are left. Rejects with `chatBusy` while the model answers in it. */
  agentChatDelete: (id: string) => invoke<ChatInfo[]>("agent_chat_delete", { id }),
  agentChatRename: (id: string, title: string) => invoke<ChatInfo>("agent_chat_rename", { id, title }),
  agentHistory: (chatId: string) => invoke<AgentEntry[]>("agent_history", { chatId }),
  /** The answer arrives as `agent:entry` events and ends with `agent:idle`. */
  agentSend: (chatId: string, text: string) => invoke<void>("agent_send", { chatId, text }),
  /** Tool-call dialogs from MCP clients. The words of those chats are not included. */
  harnessDialogs: () => invoke<HarnessDialog[]>("harness_dialogs"),
  /** Drops a closed connection tab. An open connection is refused. */
  harnessDialogForget: (id: number) => invoke<void>("harness_dialog_forget", { id }),
  /** MCP text with the path of clickit-mcp.exe next to this copy of ClickIt. */
  harnessMcpSnippet: () => invoke<McpSnippet>("harness_mcp_snippet"),

  /** What every "Clear" button of the settings would remove right now. */
  cleanupInfo: () => invoke<CleanupInfo[]>("cleanup_info"),
  /** Removes one kind of data and resolves to the sizes that are left. */
  cleanupRun: (kind: CleanupKind) => invoke<CleanupInfo[]>("cleanup_run", { kind }),

  journalGet: () => invoke<JournalEntry[]>("journal_get"),
  /** Visible top-level windows, for picking a window target. */
  windowsList: () => invoke<WindowInfo[]>("windows_list"),
};
