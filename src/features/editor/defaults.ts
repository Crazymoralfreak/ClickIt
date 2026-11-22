// Defaults for new steps and conditions.

import {
  DEFAULT_COMMAND_TIMEOUT_MS,
  type CommandRun,
  type CommandSpec,
  type Condition,
  type ConditionKind,
  type Pause,
  type Position,
  type RepeatableStep,
  type ScenarioMode,
  type Step,
  type StepType,
} from "../../types/scenario";
import { newStepId } from "./tree";

/** Order of the "Add step" menu. */
export const STEP_TYPES: ReadonlyArray<StepType> = [
  "click",
  "move",
  "drag",
  "scroll",
  "key",
  "text",
  "command",
  "pause",
  "wait",
  "if",
  "loop",
  "breakLoop",
  "stop",
];

export const CONDITION_KINDS: ReadonlyArray<ConditionKind> = ["pixel", "imageFound", "imageGone", "window", "process"];

export const DEFAULT_POLL_MS = 200;
export const DEFAULT_WAIT_TIMEOUT_MS = 10_000;
export const DEFAULT_THRESHOLD = 0.9;
export const DEFAULT_TOLERANCE = 10;

export function noPause(): Pause {
  return { fromMs: 0, toMs: 0 };
}

export function isStepType(value: string): value is StepType {
  return STEP_TYPES.some((type) => type === value);
}

export function isBlockType(type: StepType): boolean {
  return type === "if" || type === "loop";
}

export function isRepeatable(step: Step): step is RepeatableStep {
  return (
    step.type === "click" ||
    step.type === "move" ||
    step.type === "drag" ||
    step.type === "scroll" ||
    step.type === "key" ||
    step.type === "text" ||
    step.type === "pause"
  );
}

/** A concrete point in the coordinate space of the mode. */
export function pointFor(mode: ScenarioMode, x = 0, y = 0): Position {
  return mode === "window" ? { kind: "client", x, y } : { kind: "absolute", x, y };
}

export function defaultCondition(kind: ConditionKind, mode: ScenarioMode): Condition {
  switch (kind) {
    case "pixel":
      return { kind: "pixel", point: pointFor(mode), color: "#000000", tolerance: DEFAULT_TOLERANCE };
    case "imageFound":
      return { kind: "imageFound", template: "", area: { kind: "screen" }, threshold: DEFAULT_THRESHOLD };
    case "imageGone":
      return { kind: "imageGone", template: "", area: { kind: "screen" }, threshold: DEFAULT_THRESHOLD };
    case "window":
      return { kind: "window", title: "", titleMatch: "contains" };
    case "process":
      return { kind: "process", name: "" };
    default: {
      const unreachable: never = kind;
      throw new Error(`Unknown condition kind: ${String(unreachable)}`);
    }
  }
}

/** Condition of another kind; the two image kinds keep their template, area and threshold. */
export function switchCondition(current: Condition, kind: ConditionKind, mode: ScenarioMode): Condition {
  if (current.kind === kind) {
    return current;
  }
  const isImage = current.kind === "imageFound" || current.kind === "imageGone";
  if (isImage && (kind === "imageFound" || kind === "imageGone")) {
    return { kind, template: current.template, area: current.area, threshold: current.threshold };
  }
  return defaultCondition(kind, mode);
}

export function emptyCommandRun(kind: CommandRun["kind"]): CommandRun {
  switch (kind) {
    case "program":
      return { kind, path: "", args: "" };
    case "script":
      return { kind, shell: "cmd", text: "" };
    default: {
      const unreachable: never = kind;
      throw new Error(`Unknown command kind: ${String(unreachable)}`);
    }
  }
}

/** A new command runs without a console window and is waited for, so the window of the scenario stays in front. */
export function defaultCommand(): CommandSpec {
  return {
    run: emptyCommandRun("program"),
    workDir: "",
    wait: true,
    timeoutMs: DEFAULT_COMMAND_TIMEOUT_MS,
    hidden: true,
  };
}

export function createStep(type: StepType, mode: ScenarioMode): Step {
  const base = { id: newStepId(), enabled: true, pauseAfter: noPause() };
  switch (type) {
    case "click":
      return { ...base, type, button: "left", action: "click", position: { kind: "cursor" } };
    case "move":
      return { ...base, type, position: pointFor(mode) };
    case "drag":
      return { ...base, type, button: "left", from: pointFor(mode), to: pointFor(mode) };
    case "scroll":
      return { ...base, type, direction: "down", notches: 3, position: { kind: "cursor" } };
    case "key":
      return {
        ...base,
        type,
        key: "",
        action: "press",
        modifiers: { ctrl: false, alt: false, shift: false, win: false },
      };
    case "text":
      return { ...base, type, text: "" };
    case "pause":
      return { ...base, type, duration: { fromMs: 500, toMs: 500 } };
    case "wait":
      return {
        ...base,
        type,
        condition: defaultCondition("imageFound", mode),
        pollMs: DEFAULT_POLL_MS,
        timeoutMs: DEFAULT_WAIT_TIMEOUT_MS,
        onTimeout: "stop",
        onFound: { kind: "none" },
      };
    case "if":
      return {
        ...base,
        type,
        condition: defaultCondition("imageFound", mode),
        pollMs: DEFAULT_POLL_MS,
        timeoutMs: 0,
        thenSteps: [],
        elseSteps: [],
      };
    case "loop":
      return { ...base, type, mode: { kind: "count", times: 2 }, pauseBetween: noPause(), steps: [] };
    case "breakLoop":
      return { ...base, type };
    case "stop":
      return { ...base, type };
    case "command":
      return { ...base, type, command: defaultCommand(), onFailure: "stop" };
    default: {
      const unreachable: never = type;
      throw new Error(`Unknown step type: ${String(unreachable)}`);
    }
  }
}
