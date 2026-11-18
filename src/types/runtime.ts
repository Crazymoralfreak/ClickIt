// Runtime payloads exchanged with the Rust core (commands and events).

import type { Language, TargetKind } from "./scenario";

/** Error returned by any command: `errors.<code>` in the dictionaries, `params` fill placeholders. */
export type AgentRole = "user" | "assistant" | "tool";

/** One line of the assistant conversation. `imagePng` is base64 and is not stored on disk. */
export interface AgentEntry {
  id: number;
  role: AgentRole;
  text: string;
  imagePng?: string;
}

/** Who spoke a line in an external harness dialog. */
export type DialogRole = "call" | "result";

/** One tool call or its result from an external harness. */
export interface DialogEntry {
  id: number;
  role: DialogRole;
  text: string;
  imagePng?: string;
}

/** One external connection. The chat text stays in that program. */
export interface HarnessDialog {
  id: number;
  client: string;
  open: boolean;
  entries: DialogEntry[];
}

/** Ready text for a harness MCP config. `servers` replaces an empty `"mcpServers": {}`. */
export interface McpSnippet {
  servers: string;
  entry: string;
  found: boolean;
}

/** A new line, or a connection opening, for a harness dialog already on screen. */
export interface HarnessDialogEvent {
  id: number;
  client: string;
  open: boolean;
  entry?: DialogEntry;
}

export interface AppError {
  code: string;
  params?: Record<string, string>;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  /** Language picked from Windows: "ru" for a Russian UI, "en" otherwise. */
  systemLanguage: Language;
}

/** `issues.<code>` in the dictionaries. `stepId` is null for the scenario or queue as a whole;
 *  for queues it holds the item id. */
export interface Issue {
  stepId: string | null;
  code: string;
  severity: "error" | "warning";
}

export type RunState =
  | "idle"
  | "countdown"
  | "running"
  | "waiting"
  | "pausedWindow"
  | "pausedProcess"
  | "recording";

export interface RunStatus {
  state: RunState;
  /** What was started; null when idle or recording. */
  target: { kind: TargetKind; id: string; name: string } | null;
  /** Seconds left while `state` is "countdown". */
  countdownLeft: number | null;
  /** Scenario executing right now (the current item when a queue runs). */
  scenarioId: string | null;
  scenarioName: string | null;
  /** Round of the scenario, from 1. `roundsTotal` is null for "until stopped". */
  round: number;
  roundsTotal: number | null;
  stepId: string | null;
  /** 1-based position of the step, nested ones as "4.2.1". */
  stepLabel: string | null;
  /** Pass of the innermost "Loop" block around the step. `loopTotal` is null unless it is a count loop. */
  loopPass: number | null;
  loopTotal: number | null;
  /** Repeat of the step itself, set when the step has "repeat". */
  stepRepeat: number | null;
  stepRepeatTotal: number | null;
  queue: { itemIndex: number; itemCount: number; round: number; roundsTotal: number | null } | null;
  /** Executable that caused the "pausedProcess" state. */
  blockedProcess: string | null;
  recording: { scenarioId: string; steps: number; paused: boolean } | null;
}

export interface RunFinished {
  target: { kind: TargetKind; id: string; name: string };
  outcome: "completed" | "stopped" | "failed";
  /** Why it stopped or failed; null for a normal completion. */
  reason: AppError | null;
}

export interface RecordFinished {
  scenarioId: string;
  /** Steps appended to the scenario. */
  added: number;
  reason: "user" | "timeLimit" | "stepLimit" | "exit";
}

export interface CaptureResult {
  /** Virtual-screen pixels. */
  x: number;
  y: number;
  /** Set when capture was started with a window target. */
  clientX: number | null;
  clientY: number | null;
  /** `#RRGGBB` of the pixel under the cursor. */
  color: string;
  error: AppError | null;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type RegionPurpose = "template" | "area";

export interface RegionResult {
  purpose: RegionPurpose;
  /** Virtual-screen pixels. */
  rect: Rect;
  /** Same rectangle relative to the target client area, when a target was given. */
  clientRect: Rect | null;
  /** Saved template, when `purpose` is "template". */
  template: { file: string; width: number; height: number } | null;
  error: AppError | null;
}

/** `journal.codes.<code>` in the dictionaries. */
export interface JournalEntry {
  /** Local `YYYY-MM-DD HH:MM:SS`. */
  time: string;
  code: string;
  params: Record<string, string>;
}

export interface WindowInfo {
  title: string;
  /** Executable name such as `notepad.exe`. */
  process: string;
}

export type DataKind = "scenarios" | "queues" | "schedule" | "settings";

export type UpdateState =
  /** Nothing was checked in this session yet. */
  | "idle"
  | "checking"
  | "upToDate"
  /** A newer version exists, but this copy cannot install it by itself. */
  | "available"
  | "downloading"
  /** Downloaded; the installation waits for a run or a recording to end. */
  | "ready"
  | "installing"
  | "failed";

export interface UpdateStatus {
  state: UpdateState;
  /** The newer version, once one is found. */
  version: string | null;
  /** True for a copy that was not put in place by the installer. */
  portable: boolean;
}
