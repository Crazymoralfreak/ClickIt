// Immutable helpers for the step tree. Steps are addressed by id; untouched
// branches keep their identity so memoized rows do not re-render.

import type { IfStep, LoopStep, Step } from "../../types/scenario";

export type ListSlot = "root" | "then" | "else" | "body";

/** One list of steps: the top level (`parentId` null) or a list of a block. */
export interface ListRef {
  parentId: string | null;
  slot: ListSlot;
}

export const ROOT_LIST: ListRef = { parentId: null, slot: "root" };

export interface StepLocation {
  step: Step;
  list: ListRef;
  index: number;
  /** Number of blocks around the step; 0 at the top level. */
  depth: number;
}

let idSequence = 0;

/** Same shape as the ids made by the core: prefix, milliseconds, a unique tail. */
export function newStepId(): string {
  idSequence = (idSequence + 1) % 0x1000;
  const random = Math.floor(Math.random() * 0x1000);
  return `s${Date.now().toString(16)}${idSequence.toString(16).padStart(3, "0")}${random.toString(16).padStart(3, "0")}`;
}

export function isBlock(step: Step): step is IfStep | LoopStep {
  return step.type === "if" || step.type === "loop";
}

/** Nested lists of a block in display order; empty for plain steps. */
export function childLists(step: Step): Array<{ list: ListRef; steps: Step[] }> {
  if (step.type === "if") {
    return [
      { list: { parentId: step.id, slot: "then" }, steps: step.thenSteps },
      { list: { parentId: step.id, slot: "else" }, steps: step.elseSteps },
    ];
  }
  if (step.type === "loop") {
    return [{ list: { parentId: step.id, slot: "body" }, steps: step.steps }];
  }
  return [];
}

/** Passes every nested list of a block through `fn`; returns the same step when nothing changed. */
function mapLists(step: Step, fn: (steps: Step[]) => Step[]): Step {
  if (step.type === "if") {
    const thenSteps = fn(step.thenSteps);
    const elseSteps = fn(step.elseSteps);
    return thenSteps === step.thenSteps && elseSteps === step.elseSteps ? step : { ...step, thenSteps, elseSteps };
  }
  if (step.type === "loop") {
    const steps = fn(step.steps);
    return steps === step.steps ? step : { ...step, steps };
  }
  return step;
}

/** Replaces the first step that `fn` changes; ids are unique, so one hit is enough. */
function mapFirstChange(steps: Step[], fn: (step: Step) => Step): Step[] {
  for (let index = 0; index < steps.length; index += 1) {
    const step = steps[index];
    const next = fn(step);
    if (next !== step) {
      const copy = steps.slice();
      copy[index] = next;
      return copy;
    }
  }
  return steps;
}

export function locateStep(
  steps: Step[],
  id: string,
  list: ListRef = ROOT_LIST,
  depth = 0,
): StepLocation | null {
  for (let index = 0; index < steps.length; index += 1) {
    const step = steps[index];
    if (step.id === id) {
      return { step, list, index, depth };
    }
    for (const child of childLists(step)) {
      const found = locateStep(child.steps, id, child.list, depth + 1);
      if (found) {
        return found;
      }
    }
  }
  return null;
}

export function findStep(steps: Step[], id: string): Step | null {
  return locateStep(steps, id)?.step ?? null;
}

/** Number of blocks around the step, null when the id is unknown. */
export function depthOf(steps: Step[], id: string): number | null {
  return locateStep(steps, id)?.depth ?? null;
}

/** Number of blocks around the steps of `list`, null when its block is unknown. */
export function depthOfList(steps: Step[], list: ListRef): number | null {
  if (list.parentId === null) {
    return 0;
  }
  const parent = depthOf(steps, list.parentId);
  return parent === null ? null : parent + 1;
}

/** Steps of `list`, null when its block is unknown or has no such list. */
export function stepsOfList(steps: Step[], list: ListRef): Step[] | null {
  if (list.parentId === null) {
    return steps;
  }
  const parent = findStep(steps, list.parentId);
  if (!parent) {
    return null;
  }
  return childLists(parent).find((child) => child.list.slot === list.slot)?.steps ?? null;
}

export function containsStep(step: Step, id: string): boolean {
  return step.id === id || childLists(step).some((child) => child.steps.some((inner) => containsStep(inner, id)));
}

/** Counts the steps of the list and of every nested list. */
export function countSteps(steps: Step[]): number {
  let total = 0;
  for (const step of steps) {
    total += 1;
    for (const child of childLists(step)) {
      total += countSteps(child.steps);
    }
  }
  return total;
}

/** Levels of blocks the step brings along: 0 for a plain step, 1 for a block without nested blocks. */
export function blockLevels(step: Step): number {
  if (!isBlock(step)) {
    return 0;
  }
  let deepest = 0;
  for (const child of childLists(step)) {
    for (const inner of child.steps) {
      deepest = Math.max(deepest, blockLevels(inner));
    }
  }
  return 1 + deepest;
}

export function updateStep(steps: Step[], id: string, fn: (step: Step) => Step): Step[] {
  const index = steps.findIndex((step) => step.id === id);
  if (index >= 0) {
    const next = fn(steps[index]);
    if (next === steps[index]) {
      return steps;
    }
    const copy = steps.slice();
    copy[index] = next;
    return copy;
  }
  return mapFirstChange(steps, (step) => mapLists(step, (list) => updateStep(list, id, fn)));
}

/** Rewrites the list that directly holds the step `id`. */
function editListOf(steps: Step[], id: string, fn: (list: Step[], index: number) => Step[]): Step[] {
  const index = steps.findIndex((step) => step.id === id);
  if (index >= 0) {
    return fn(steps, index);
  }
  return mapFirstChange(steps, (step) => mapLists(step, (list) => editListOf(list, id, fn)));
}

function insertAt(list: Step[], index: number, step: Step): Step[] {
  const at = Math.max(0, Math.min(index, list.length));
  return [...list.slice(0, at), step, ...list.slice(at)];
}

export function removeStep(steps: Step[], id: string): Step[] {
  return editListOf(steps, id, (list, index) => list.filter((_, at) => at !== index));
}

/** Swaps the step with its neighbour inside its own list; `delta` is -1 (up) or 1 (down). */
export function moveStep(steps: Step[], id: string, delta: -1 | 1): Step[] {
  return editListOf(steps, id, (list, index) => {
    const other = index + delta;
    if (other < 0 || other >= list.length) {
      return list;
    }
    const copy = list.slice();
    copy[index] = list[other];
    copy[other] = list[index];
    return copy;
  });
}

/** Puts `step` into `list` at `index` (clamped, so a large index appends). */
export function insertStep(steps: Step[], list: ListRef, index: number, step: Step): Step[] {
  const parentId = list.parentId;
  if (parentId === null) {
    return insertAt(steps, index, step);
  }
  return updateStep(steps, parentId, (parent) => {
    if (parent.type === "if" && list.slot === "then") {
      return { ...parent, thenSteps: insertAt(parent.thenSteps, index, step) };
    }
    if (parent.type === "if" && list.slot === "else") {
      return { ...parent, elseSteps: insertAt(parent.elseSteps, index, step) };
    }
    if (parent.type === "loop" && list.slot === "body") {
      return { ...parent, steps: insertAt(parent.steps, index, step) };
    }
    return parent;
  });
}

/** Puts `step` right after the step `id`, in the same list. */
export function insertAfter(steps: Step[], id: string, step: Step): Step[] {
  return editListOf(steps, id, (list, index) => insertAt(list, index + 1, step));
}

/** Deep copy with fresh ids for the step and everything nested in it. */
export function cloneStep(step: Step): Step {
  return mapLists({ ...step, id: newStepId() }, (list) => list.map(cloneStep));
}
