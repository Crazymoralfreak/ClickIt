// What the step forms need to know about the scenario they belong to, and
// the operations of the step list. Rows get the operations as one stable object.

import { createContext, useContext } from "react";
import type { Issue } from "../../types/runtime";
import type { ScenarioMode, Step, StepType, WindowTarget } from "../../types/scenario";
import type { ListRef } from "./tree";

export interface EditorEnv {
  scenarioId: string;
  mode: ScenarioMode;
  target: WindowTarget;
}

export const EditorEnvContext = createContext<EditorEnv | null>(null);

export function useEditorEnv(): EditorEnv {
  const value = useContext(EditorEnvContext);
  if (!value) {
    throw new Error("useEditorEnv must be used inside the scenario editor");
  }
  return value;
}

export interface StepActions {
  /** Opens the form of the step, or closes it when it is already open. */
  toggle: (id: string) => void;
  update: (id: string, fn: (step: Step) => Step) => void;
  move: (id: string, delta: -1 | 1) => void;
  duplicate: (id: string) => void;
  remove: (id: string) => void;
  add: (list: ListRef, type: StepType) => void;
}

/** Issues grouped by step id. */
export type IssueMap = ReadonlyMap<string, ReadonlyArray<Issue>>;

export const NO_ISSUES: ReadonlyArray<Issue> = [];
