// Edits of the graph of the open canvas, with undo and redo. A graph that changed
// without the canvas (the assistant, a harness, "Extract", a recording) goes into
// the same history, and what it added or changed is reported for a few seconds.

import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import type { Graph, Scenario } from "../../types/scenario";
import { changedNodes, sameGraph } from "./graph";

/** Edits of one field made within this time are one step of the history. */
const COALESCE_MS = 800;
const HISTORY_LIMIT = 100;
/** How long blocks changed from outside stay highlighted. */
const CHANGED_MS = 4000;
const NO_IDS: ReadonlySet<string> = new Set();

export interface GraphHistory {
  /** Applies an edit. Edits with the same `key` made in a row are undone together. */
  apply: (fn: (graph: Graph) => Graph, key?: string) => void;
  undo: () => void;
  redo: () => void;
  canUndo: boolean;
  canRedo: boolean;
  /** Blocks added or changed from outside a moment ago. */
  changedIds: ReadonlySet<string>;
}

export function useGraphHistory(
  graph: Graph,
  change: (fn: (scenario: Scenario) => Scenario) => void,
  /** True while a run or a recording is in progress: the scenario must not be edited. */
  locked: RefObject<boolean>,
): GraphHistory {
  /** The graph this canvas made or saw last; anything else came from outside. */
  const own = useRef(graph);
  const undoStack = useRef<Graph[]>([]);
  const redoStack = useRef<Graph[]>([]);
  const lastEdit = useRef<{ key: string | undefined; at: number }>({ key: undefined, at: 0 });
  const [, setTick] = useState(0);
  const [changedIds, setChangedIds] = useState<ReadonlySet<string>>(NO_IDS);

  const remember = useCallback((before: Graph, key?: string) => {
    const now = Date.now();
    const merged = key !== undefined && key === lastEdit.current.key && now - lastEdit.current.at < COALESCE_MS;
    lastEdit.current = { key, at: now };
    if (!merged) {
      undoStack.current.push(before);
      if (undoStack.current.length > HISTORY_LIMIT) {
        undoStack.current.shift();
      }
    }
    redoStack.current = [];
  }, []);

  const apply = useCallback(
    (fn: (graph: Graph) => Graph, key?: string) => {
      if (locked.current) {
        return;
      }
      change((scenario) => {
        const current = scenario.graph;
        if (!current) {
          return scenario;
        }
        const next = fn(current);
        if (next === current) {
          return scenario;
        }
        remember(current, key);
        own.current = next;
        return { ...scenario, graph: next };
      });
      setTick((tick) => tick + 1);
    },
    [change, remember, locked],
  );

  const travel = useCallback(
    (from: Graph[], to: Graph[]) => {
      if (locked.current || from.length === 0) {
        return;
      }
      change((scenario) => {
        const target = from.pop();
        if (!scenario.graph || !target) {
          return scenario;
        }
        to.push(scenario.graph);
        own.current = target;
        return { ...scenario, graph: target };
      });
      lastEdit.current = { key: undefined, at: 0 };
      setTick((tick) => tick + 1);
    },
    [change, locked],
  );

  const undo = useCallback(() => travel(undoStack.current, redoStack.current), [travel]);
  const redo = useCallback(() => travel(redoStack.current, undoStack.current), [travel]);

  useEffect(() => {
    if (graph === own.current) {
      return;
    }
    const before = own.current;
    own.current = graph;
    // The same graph read from disk again is not a change.
    if (sameGraph(before, graph)) {
      return;
    }
    remember(before);
    setTick((tick) => tick + 1);
    const ids = changedNodes(before, graph);
    if (ids.length > 0) {
      setChangedIds(new Set(ids));
    }
  }, [graph, remember]);

  useEffect(() => {
    if (changedIds.size === 0) {
      return;
    }
    const timer = window.setTimeout(() => setChangedIds(NO_IDS), CHANGED_MS);
    return () => window.clearTimeout(timer);
  }, [changedIds]);

  return {
    apply,
    undo,
    redo,
    canUndo: undoStack.current.length > 0,
    canRedo: redoStack.current.length > 0,
    changedIds,
  };
}
