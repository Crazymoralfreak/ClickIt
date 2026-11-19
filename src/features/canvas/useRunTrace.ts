// Live state of a run on the canvas: the working block, the links a pass took
// and the trace of the last run. Blocks and links subscribe to their own piece,
// so an event repaints only what it touched.

import { useEffect, useMemo, useSyncExternalStore } from "react";
import { api } from "../../api";
import { useAppEvent } from "../../api/events";
import { useApp } from "../../app/context";
import type { RunFinished, RunState, RunStatus, RunTrace, TraceMove, TracePhase } from "../../types/runtime";
import type { Port } from "../../types/scenario";

/** Time between two moves of one event, so that a pass is seen running along the links. */
const MOVE_GAP_MS = 90;
/** Moves that pile up beyond this are shown at once: the canvas does not lag behind the run. */
const MOVE_BACKLOG = 12;

export type LivePhase = TracePhase | "break";

export interface NodeLive {
  /** The run is at this block right now. */
  active: boolean;
  phase: LivePhase | null;
  /** How long the phase lasts when that is known: a timeout or a pause. */
  phaseMs: number | null;
  /** Changes with every entry into the block and every phase; restarts the animations. */
  stamp: number;
  /** `performance.now()` at the start of the phase. */
  phaseAt: number;
  checks: number;
  /** Times the block was entered in this run. */
  passes: number;
  /** The output the block was left through last. */
  outcome: Port | null;
  counterPass: number | null;
  counterTotal: number | null;
  repeat: number | null;
  repeatTotal: number | null;
  /** Number of the block a called scenario is at. */
  inner: string | null;
  /** The run ended at this block, and how. */
  ended: RunFinished["outcome"] | null;
}

export interface EdgeLive {
  passes: number;
  /** Grows every time a pass takes the link; 0 when it has not been taken on screen. */
  flash: number;
}

const IDLE_NODE: NodeLive = {
  active: false,
  phase: null,
  phaseMs: null,
  stamp: 0,
  phaseAt: 0,
  checks: 0,
  passes: 0,
  outcome: null,
  counterPass: null,
  counterTotal: null,
  repeat: null,
  repeatTotal: null,
  inner: null,
  ended: null,
};

const IDLE_EDGE: EdgeLive = { passes: 0, flash: 0 };

function sameFields<T extends object>(a: T, b: T): boolean {
  return (Object.keys(a) as Array<keyof T>).every((key) => a[key] === b[key]);
}

interface StatusPart {
  state: RunState;
  scenarioId: string | null;
  stepId: string | null;
  stepLabel: string | null;
  stepRepeat: number | null;
  stepRepeatTotal: number | null;
}

const IDLE_STATUS: StatusPart = {
  state: "idle",
  scenarioId: null,
  stepId: null,
  stepLabel: null,
  stepRepeat: null,
  stepRepeatTotal: null,
};

export class TraceStore {
  private readonly listeners = new Set<() => void>();
  private trace: RunTrace | null = null;
  private status: StatusPart = IDLE_STATUS;
  private version = 0;
  private stamp = 0;
  private phaseAt = 0;
  private phaseKey = "";
  private flashSeq = 0;
  private readonly flashes = new Map<string, number>();
  private queue: TraceMove[] = [];
  private timer: number | null = null;
  private readonly nodeCache = new Map<string, { version: number; value: NodeLive }>();
  private readonly edgeCache = new Map<string, { version: number; value: EdgeLive }>();

  constructor(private readonly scenarioId: string) {}

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  private changed(): void {
    this.version += 1;
    for (const listener of this.listeners) {
      listener();
    }
  }

  /** The run is going and the trace belongs to it. */
  private running(): boolean {
    return this.trace !== null && this.trace.stop === null && this.status.state !== "idle";
  }

  /** The block the run is at, null when nothing runs. */
  getActive = (): string | null => (this.running() ? (this.trace?.node ?? null) : null);

  /** No event and no stored trace has been taken yet. */
  isEmpty(): boolean {
    return this.trace === null;
  }

  /** There is something for "Clear trace" to clear. */
  getHasTrace = (): boolean => this.trace !== null && !this.running();

  node(id: string): NodeLive {
    const cached = this.nodeCache.get(id);
    if (cached && cached.version === this.version) {
      return cached.value;
    }
    const next = this.computeNode(id);
    const value = cached && sameFields(cached.value, next) ? cached.value : sameFields(IDLE_NODE, next) ? IDLE_NODE : next;
    this.nodeCache.set(id, { version: this.version, value });
    return value;
  }

  private computeNode(id: string): NodeLive {
    const trace = this.trace;
    if (!trace) {
      return IDLE_NODE;
    }
    const active = this.running() && trace.node === id;
    const counter = trace.counters[id];
    const status = this.status;
    // While a called scenario runs, the status speaks about its blocks.
    const inside = active && status.scenarioId !== null && status.scenarioId !== this.scenarioId;
    const own = active && !inside && status.stepId === id;
    return {
      active,
      phase: active ? (status.state === "pausedBreak" ? "break" : trace.phase) : null,
      phaseMs: active ? trace.phaseMs : null,
      stamp: active ? this.stamp : 0,
      phaseAt: active ? this.phaseAt : 0,
      checks: active ? trace.checks : 0,
      passes: trace.nodePasses[id] ?? 0,
      outcome: trace.outcomes[id] ?? null,
      counterPass: counter ? counter[0] : null,
      counterTotal: counter ? counter[1] : null,
      repeat: own ? status.stepRepeat : null,
      repeatTotal: own ? status.stepRepeatTotal : null,
      inner: inside ? status.stepLabel : null,
      ended: trace.stop !== null && trace.stop.node === id ? trace.stop.outcome : null,
    };
  }

  edge(key: string): EdgeLive {
    const cached = this.edgeCache.get(key);
    if (cached && cached.version === this.version) {
      return cached.value;
    }
    const passes = this.trace?.edgePasses[key] ?? 0;
    const flash = this.flashes.get(key) ?? 0;
    const value =
      cached && cached.value.passes === passes && cached.value.flash === flash
        ? cached.value
        : passes === 0 && flash === 0
          ? IDLE_EDGE
          : { passes, flash };
    this.edgeCache.set(key, { version: this.version, value });
    return value;
  }

  /** The reason the last run stopped at a block, for the panel of that block. */
  stopOf(id: string): RunTrace["stop"] {
    const stop = this.trace?.stop ?? null;
    return stop !== null && stop.node === id ? stop : null;
  }

  /** Takes an event of a run, or the trace kept by the core when `animate` is false. */
  apply(event: RunTrace, animate: boolean): void {
    if (event.scenarioId !== this.scenarioId) {
      return;
    }
    const previous = this.trace;
    if (!previous || previous.run !== event.run) {
      this.flashes.clear();
      this.queue = [];
      this.nodeCache.clear();
      this.edgeCache.clear();
    }
    this.trace = event;
    const key = `${event.run}:${event.node ?? ""}:${event.phase ?? ""}:${event.phaseMs ?? ""}:${
      event.node === null ? 0 : (event.nodePasses[event.node] ?? 0)
    }`;
    if (key !== this.phaseKey) {
      this.phaseKey = key;
      this.stamp += 1;
      this.phaseAt = performance.now();
    }
    if (animate && event.moves.length > 0) {
      this.queue.push(...event.moves);
      this.play();
    }
    this.changed();
  }

  /** Shows the queued moves one after another. */
  private play(): void {
    while (this.queue.length > MOVE_BACKLOG) {
      this.flash(this.queue.shift());
    }
    if (this.timer !== null) {
      return;
    }
    this.flash(this.queue.shift());
    const tick = () => {
      this.timer = null;
      if (this.queue.length === 0) {
        return;
      }
      this.flash(this.queue.shift());
      this.changed();
      this.timer = window.setTimeout(tick, MOVE_GAP_MS);
    };
    this.timer = window.setTimeout(tick, MOVE_GAP_MS);
  }

  private flash(move: TraceMove | undefined): void {
    if (move && move.to !== null) {
      this.flashSeq += 1;
      this.flashes.set(`${move.from}:${move.port}`, this.flashSeq);
    }
  }

  setStatus(status: RunStatus): void {
    const next: StatusPart = {
      state: status.state,
      scenarioId: status.scenarioId,
      stepId: status.stepId,
      stepLabel: status.stepLabel,
      stepRepeat: status.stepRepeat,
      stepRepeatTotal: status.stepRepeatTotal,
    };
    if (sameFields(this.status, next)) {
      return;
    }
    this.status = next;
    this.changed();
  }

  /** Forgets the trace of the last run. */
  clear(): void {
    if (this.trace === null) {
      return;
    }
    this.trace = null;
    this.flashes.clear();
    this.queue = [];
    this.nodeCache.clear();
    this.edgeCache.clear();
    this.changed();
  }

  /** Stops the timer of the queued moves; the next event starts it again. */
  dispose(): void {
    if (this.timer !== null) {
      window.clearTimeout(this.timer);
      this.timer = null;
    }
    this.queue = [];
  }
}

/** The store of one open canvas: fed by `run:trace`, by the status and by the trace the core kept. */
export function useRunTrace(scenarioId: string): TraceStore {
  const { status, showError } = useApp();
  const store = useMemo(() => new TraceStore(scenarioId), [scenarioId]);

  useEffect(() => {
    let cancelled = false;
    api
      .runTrace(scenarioId)
      .then((trace) => {
        // An event that came meanwhile is newer than the stored copy.
        if (!cancelled && trace && store.isEmpty()) {
          store.apply(trace, false);
        }
      })
      .catch(showError);
    return () => {
      cancelled = true;
      store.dispose();
    };
  }, [store, scenarioId, showError]);

  useEffect(() => {
    store.setStatus(status);
  }, [store, status]);

  useAppEvent("run:trace", (event) => store.apply(event, true));

  return store;
}

export function useNodeLive(store: TraceStore, id: string): NodeLive {
  return useSyncExternalStore(store.subscribe, () => store.node(id));
}

export function useEdgeLive(store: TraceStore, key: string): EdgeLive {
  return useSyncExternalStore(store.subscribe, () => store.edge(key));
}

export function useActiveNode(store: TraceStore): string | null {
  return useSyncExternalStore(store.subscribe, store.getActive);
}

export function useHasTrace(store: TraceStore): boolean {
  return useSyncExternalStore(store.subscribe, store.getHasTrace);
}
