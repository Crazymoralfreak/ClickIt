// Mirror of the Rust model in src-tauri/src/model. Keep both in sync by hand.

export const MAX_STEPS = 500;
export const MAX_DEPTH = 3;
export const MAX_STEP_REPEAT = 10_000;
export const MIN_INTERVAL_MINUTES = 1;
export const MAX_INTERVAL_MINUTES = 24 * 60;
export const MAX_COUNTDOWN_SEC = 10;

/** Whole milliseconds picked from `fromMs..=toMs`; equal bounds mean an exact pause. */
export interface Pause {
  fromMs: number;
  toMs: number;
}

export type ScenarioMode = "screen" | "window";
export type TitleMatch = "contains" | "exact";

/** Empty strings mean "not set"; when both are set a window has to match both. */
export interface WindowTarget {
  title: string;
  titleMatch: TitleMatch;
  process: string;
}

/** `absolute` belongs to the "screen" mode, `client` to the "window" mode. */
export type Position =
  | { kind: "cursor" }
  | { kind: "absolute"; x: number; y: number }
  | { kind: "client"; x: number; y: number };

export type MouseButton = "left" | "right" | "middle";
export type ClickAction = "click" | "double" | "down" | "up";
export type ScrollDirection = "up" | "down";
export type KeyAction = "press" | "down" | "up";

export interface Modifiers {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  win: boolean;
}

export type CoordSpace = "absolute" | "client";

export type Area =
  | { kind: "screen" }
  | { kind: "client" }
  | { kind: "rect"; space: CoordSpace; x: number; y: number; width: number; height: number };

export type Condition =
  /** `color` is `#RRGGBB`, `tolerance` is per channel, 0..255. `point` is never `cursor`. */
  | { kind: "pixel"; point: Position; color: string; tolerance: number }
  /** `template` is a PNG file name in the scenario folder, `threshold` is 0.5..1. */
  | { kind: "imageFound"; template: string; area: Area; threshold: number }
  | { kind: "imageGone"; template: string; area: Area; threshold: number }
  | { kind: "window"; title: string; titleMatch: TitleMatch }
  | { kind: "process"; name: string }
  /** Both items hold. The second is not looked at when the first does not. */
  | { kind: "all"; items: Condition[] }
  /** Either item holds. The second is not looked at when the first does. */
  | { kind: "any"; items: Condition[] };

/** A condition that is one check. `all` and `any` combine two of these. */
export type ConditionKind = Exclude<Condition["kind"], "all" | "any">;
export type OnTimeout = "stop" | "continue";

export type OnFound =
  | { kind: "none" }
  | { kind: "click"; button: MouseButton; offsetX: number; offsetY: number };

/** Repeat of a single input or pause step, 1..10000 times. */
export interface Repeat {
  times: number;
  pauseBetween: Pause;
}

export type LoopMode =
  | { kind: "count"; times: number }
  | { kind: "while"; condition: Condition }
  | { kind: "forever" };

/** Repeat of a whole scenario or queue. */
export type RepeatMode = { kind: "once" } | { kind: "count"; times: number } | { kind: "forever" };

interface StepBase {
  id: string;
  enabled: boolean;
  pauseAfter: Pause;
}

/** Only mouse, keyboard, text and pause steps may carry `repeat`. */
interface RepeatableStepBase extends StepBase {
  repeat?: Repeat;
}

export interface ClickStep extends RepeatableStepBase {
  type: "click";
  button: MouseButton;
  action: ClickAction;
  position: Position;
}

export interface MoveStep extends RepeatableStepBase {
  type: "move";
  position: Position;
}

export interface DragStep extends RepeatableStepBase {
  type: "drag";
  button: MouseButton;
  from: Position;
  to: Position;
}

export interface ScrollStep extends RepeatableStepBase {
  type: "scroll";
  direction: ScrollDirection;
  notches: number;
  position: Position;
}

/** `key` is a `KeyboardEvent.code` name: `KeyA`, `Enter`, `F5`. */
export interface KeyStep extends RepeatableStepBase {
  type: "key";
  key: string;
  action: KeyAction;
  modifiers: Modifiers;
}

export interface TextStep extends RepeatableStepBase {
  type: "text";
  text: string;
}

export interface PauseStep extends RepeatableStepBase {
  type: "pause";
  duration: Pause;
}

/** `timeoutMs` 0 waits until the run is stopped by hand. */
export interface WaitStep extends StepBase {
  type: "wait";
  condition: Condition;
  pollMs: number;
  timeoutMs: number;
  onTimeout: OnTimeout;
  onFound: OnFound;
}

/** `timeoutMs` 0 checks the condition once, right away. */
export interface IfStep extends StepBase {
  type: "if";
  condition: Condition;
  pollMs: number;
  timeoutMs: number;
  thenSteps: Step[];
  elseSteps: Step[];
}

export interface LoopStep extends StepBase {
  type: "loop";
  mode: LoopMode;
  pauseBetween: Pause;
  steps: Step[];
}

export interface BreakLoopStep extends StepBase {
  type: "breakLoop";
}

export interface StopStep extends StepBase {
  type: "stop";
}

export type Step =
  | ClickStep
  | MoveStep
  | DragStep
  | ScrollStep
  | KeyStep
  | TextStep
  | PauseStep
  | WaitStep
  | IfStep
  | LoopStep
  | BreakLoopStep
  | StopStep;

export type StepType = Step["type"];
export type RepeatableStep = ClickStep | MoveStep | DragStep | ScrollStep | KeyStep | TextStep | PauseStep;

// The canvas form of a scenario. Mirror of src-tauri/src/model/graph.rs.

export const MAX_INTERCEPTS = 8;
export const DEFAULT_INTERCEPT_MS = 2000;
export const MIN_INTERCEPT_MS = 500;
/** Size of a block on the canvas. */
export const BLOCK_WIDTH = 220;
export const BLOCK_HEIGHT = 84;

/** An output of a block. One link may leave it. */
export type Port = "next" | "timeout" | "yes" | "no" | "body" | "done";

/** What a block has on top of the fields of a step. */
interface NodeExtra {
  /** The user's own name of the block, shown instead of its type. */
  title?: string;
  /** A block saved without a position is placed by the automatic layout. */
  x?: number;
  y?: number;
}

interface NodeBase extends StepBase, NodeExtra {}

/** Action blocks carry the same fields as the steps of a list. */
export type ActionNode = (RepeatableStep | StopStep) & NodeExtra;

/** Where a pass begins. Exactly one per graph. */
export interface StartNode extends NodeBase {
  type: "start";
}

/** Outputs `next` and `timeout`. An unwired `timeout` fails the scenario. */
export interface WaitNode extends NodeBase {
  type: "wait";
  condition: Condition;
  pollMs: number;
  timeoutMs: number;
  onFound: OnFound;
}

/** Outputs `yes` and `no`. `timeoutMs` 0 checks the condition once, right away. */
export interface BranchNode extends NodeBase {
  type: "branch";
  condition: Condition;
  pollMs: number;
  timeoutMs: number;
}

/** Lets the pass into `body` the given number of times, then into `done`. */
export interface CounterNode extends NodeBase {
  type: "counter";
  times: number;
  pauseBetween: Pause;
}

/** Checked once on entry. `until` sends the pass into `body` while the condition does not hold. */
export interface WhileNode extends NodeBase {
  type: "while";
  condition: Condition;
  until: boolean;
  pauseBetween: Pause;
}

/** Brings a window to the front. The run fails when it is not there. */
export interface FocusNode extends NodeBase {
  type: "focus";
  target: WindowTarget;
}

/** Runs one pass of another scenario. */
export interface CallNode extends NodeBase {
  type: "call";
  scenarioId: string;
  repeat?: Repeat;
}

/** Watches for a condition on top of the main flow. Has no input. */
export interface InterceptNode extends NodeBase {
  type: "intercept";
  condition: Condition;
  minIntervalMs: number;
}

export type GraphNode =
  | ActionNode
  | StartNode
  | WaitNode
  | BranchNode
  | CounterNode
  | WhileNode
  | FocusNode
  | CallNode
  | InterceptNode;
export type NodeType = GraphNode["type"];

export interface GraphPoint {
  x: number;
  y: number;
}

export interface GraphEdge {
  from: string;
  port: Port;
  to: string;
  /** Bend points the user put on the link. */
  points: GraphPoint[];
}

export type NoteKind = "note" | "frame";

/** A note or a frame. Neither the engine nor the checks read them. */
export interface GraphNote {
  id: string;
  kind: NoteKind;
  x: number;
  y: number;
  width: number;
  height: number;
  text: string;
  collapsed: boolean;
  /** One of the names in the palette of the canvas; absent for the default look. */
  color?: string;
}

export interface Graph {
  nodes: GraphNode[];
  edges: GraphEdge[];
  notes: GraphNote[];
}

export type ScenarioFormat = "list" | "canvas";

export interface Scenario {
  version: number;
  id: string;
  /** Grows by one on every successful save. A stale copy is rejected. */
  revision: number;
  name: string;
  mode: ScenarioMode;
  target: WindowTarget;
  repeat: RepeatMode;
  pauseBetweenRounds: Pause;
  steps: Step[];
  /** Set for a scenario drawn on the canvas; `steps` is empty then. */
  graph?: Graph;
}

export interface ScenarioSummary {
  id: string;
  name: string;
  mode: ScenarioMode;
  /** Steps of a list or blocks of a graph. */
  stepCount: number;
  format: ScenarioFormat;
}

/** A piece of a graph the user saved to put into other scenarios. */
export interface Snippet {
  id: string;
  name: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface QueueItem {
  id: string;
  scenarioId: string;
  /** Rounds of the scenario inside the queue; the scenario's own repeat is ignored there. */
  rounds: number;
}

export type OnItemFailure = "stop" | "next";

export interface Queue {
  version: number;
  id: string;
  name: string;
  items: QueueItem[];
  pauseBetweenItems: Pause;
  repeat: RepeatMode;
  onItemFailure: OnItemFailure;
}

export interface QueueSummary {
  id: string;
  name: string;
  itemCount: number;
  /** True when an item points at a deleted scenario. */
  broken: boolean;
}

export type TargetKind = "scenario" | "queue";

export interface RunTarget {
  kind: TargetKind;
  id: string;
}

/** Local `HH:MM`; `from` later than `to` wraps over midnight. */
export interface DayWindow {
  from: string;
  to: string;
}

/** Local time. Date-times are `YYYY-MM-DDTHH:MM`, times are `HH:MM`. */
export type RuleWhen =
  | { kind: "once"; at: string }
  | { kind: "daily"; time: string }
  /** `days`: 1 = Monday .. 7 = Sunday. */
  | { kind: "weekly"; days: number[]; time: string }
  | { kind: "interval"; everyMinutes: number; startAt: string; window?: DayWindow };

export interface ScheduleRule {
  id: string;
  enabled: boolean;
  target: RunTarget;
  when: RuleWhen;
}

export type Language = "ru" | "en";

export interface Settings {
  language: Language | null;
  /** `Ctrl+Shift+F6`: modifiers (Ctrl, Alt, Shift, Win) plus a key code name. */
  hotkeyStartStop: string;
  hotkeyCapture: string;
  /** Held only during a run with breakpoints: one block further, and on to the next breakpoint. */
  hotkeyStep: string;
  hotkeyContinue: string;
  /** Held only while a recording is going. Empty means not assigned. */
  hotkeyRecordPause: string;
  hotkeyRecordWaitImage: string;
  hotkeyRecordClickImage: string;
  /** 0..10 seconds, 0 turns the countdown off. */
  countdownSec: number;
  cornerStop: boolean;
  blockedProcesses: string[];
  closeExits: boolean;
  catchUp: boolean;
  /** Ask GitHub for a newer version at startup. The assistant is a separate, explicit request. */
  checkUpdates: boolean;
  /** Accept a local connection from clickit-mcp. Off by default. */
  harness: boolean;
  /** A harness or the assistant may start a scenario without the confirm dialog. */
  harnessAutoRun: boolean;
  /** OpenAI-compatible base address. Empty means the assistant does not use the network. */
  agentEndpoint: string;
  /** Model name sent with every assistant request. */
  agentModel: string;
  /** What Start, the hotkey and the tray "Start" item run. */
  current: RunTarget | null;
}
