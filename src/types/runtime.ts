// Runtime payloads exchanged with the Rust core (commands and events).

import type { Language, Port, Scenario, ScenarioSummary, TargetKind } from "./scenario";

/** Error returned by any command: `errors.<code>` in the dictionaries, `params` fill placeholders. */
export type AgentRole = "user" | "assistant" | "tool";

/** One line of the assistant conversation. `imagePng` is base64 and is not stored on disk. */
export interface AgentEntry {
  id: number;
  role: AgentRole;
  text: string;
  imagePng?: string;
}

/** One chat with the model. An empty `title` is shown as "New chat". */
export interface ChatInfo {
  id: string;
  title: string;
  /** Local `YYYY-MM-DDTHH:MM:SS`. */
  created: string;
  entryCount: number;
  /** The model is answering in this chat right now. */
  busy: boolean;
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

/** A command the way the confirm dialog shows it. */
export interface ShownCommand {
  /** The program with its arguments, or the script under the name of its interpreter. */
  text: string;
  /** Empty when the step does not set the folder. */
  workDir: string;
}

/**
 * A question a harness or the assistant waits on: start the scenario, or
 * save new commands into it. `commands` is empty for a scenario without them.
 */
export interface HarnessAsk {
  requestId: number;
  kind: "run" | "save";
  name: string;
  commands: ShownCommand[];
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
  /** Stopped on a breakpoint or between two blocks of a step-by-step run. */
  | "pausedBreak"
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
  recording: { scenarioId: string; steps: number; paused: boolean; held: boolean } | null;
}

export interface RunFinished {
  target: { kind: TargetKind; id: string; name: string };
  outcome: "completed" | "stopped" | "failed";
  /** Why it stopped or failed; null for a normal completion. */
  reason: AppError | null;
}

/** Breakpoints of the open editor and the block a trial run starts from. */
export interface RunOptions {
  /** One pass of a graph from this block, without the repeats of the scenario. */
  startNode?: string;
  debug?: {
    breakpoints: string[];
    /** Pause before every block, starting with the first one. */
    step: boolean;
  };
}

export type TracePhase = "action" | "check" | "pause";

/** A pass went from one block to the next one. `to` is null at an output without a link. */
export interface TraceMove {
  from: string;
  port: Port;
  to: string | null;
}

export interface TraceStop {
  node: string | null;
  outcome: RunFinished["outcome"];
  reason: AppError | null;
}

/** Where a graph scenario is and where it has been. Sent at most ten times a second. */
export interface RunTrace {
  scenarioId: string;
  /** Grows with every run. */
  run: number;
  /** The block being executed. */
  node: string | null;
  phase: TracePhase | null;
  /** How long the phase lasts when that is known: a timeout or a pause. */
  phaseMs: number | null;
  /** How many times the condition of the current block was looked at. */
  checks: number;
  /** Transitions since the previous event; in a stored trace, the last ones of the run. */
  moves: TraceMove[];
  nodePasses: Record<string, number>;
  /** The key is `<block id>:<output>`. */
  edgePasses: Record<string, number>;
  /** The output every block was left through last. */
  outcomes: Record<string, Port>;
  /** Passes done and total of the counters that are in their body now. */
  counters: Record<string, [number, number]>;
  /** Set by the last event of a run. */
  stop: TraceStop | null;
}

/** One look at a condition: "Check now". */
export interface CheckResult {
  holds: boolean;
  /** For an image: whether it is on the screen, whatever the condition asks for. */
  found: boolean | null;
  /** Centre of the found image, in virtual-screen pixels. */
  x: number | null;
  y: number | null;
  clientX: number | null;
  clientY: number | null;
  /** Similarity of the found image, 0..1. */
  score: number | null;
  /** Set when the image is not found at its threshold but would be at this similarity. */
  wouldMatchAt: number | null;
  /** For a pixel: the colour that is there now. */
  color: string | null;
  ms: number;
}

export type CleanupKind = "journal" | "chats" | "connections" | "templates" | "backups" | "cache" | "leftovers";

/** What one "Clear" button of the settings would remove right now. */
export interface CleanupInfo {
  kind: CleanupKind;
  bytes: number;
  files: number;
  /** Error code of the reason the button cannot be used right now. */
  blocked: string | null;
}

export interface ScenarioMeta {
  /** The list this graph was made from is still there, so it can be returned. */
  hasBackup: boolean;
  /** Scenarios whose "Run scenario" blocks run this one. */
  callers: ScenarioSummary[];
}

export interface Extracted {
  /** The scenario the blocks were taken from, with a "Run scenario" block in their place. */
  scenario: Scenario;
  created: Scenario;
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
