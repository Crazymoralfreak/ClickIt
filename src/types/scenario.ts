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
  | { kind: "process"; name: string };

export type ConditionKind = Condition["kind"];
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
}

export interface ScenarioSummary {
  id: string;
  name: string;
  mode: ScenarioMode;
  stepCount: number;
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
