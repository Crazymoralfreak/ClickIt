// Immutable edits of a graph and the questions the canvas asks about it.
// Blocks, links and notes are addressed by id; untouched parts keep their identity.

import {
  BLOCK_HEIGHT,
  BLOCK_WIDTH,
  DEFAULT_INTERCEPT_MS,
  MAX_STEPS,
  type Graph,
  type GraphEdge,
  type GraphNode,
  type GraphNote,
  type GraphPoint,
  type NodeType,
  type NoteKind,
  type Port,
  type Scenario,
  type ScenarioMode,
} from "../../types/scenario";
import { createStep, defaultCondition, DEFAULT_POLL_MS, DEFAULT_WAIT_TIMEOUT_MS, noPause } from "../editor/defaults";
import { countSteps } from "../editor/tree";

export const GRID = 20;
/** Distance between blocks placed by the automatic layout; the same as in the core. */
const COLUMN = 280;
const ROW = 140;
const FRAME_MARGIN = 24;
/** Room for the title of a frame above its blocks. */
const FRAME_TITLE = 36;
export const MAX_BLOCKS = MAX_STEPS;
export const NOTE_WIDTH = 200;
export const NOTE_HEIGHT = 100;
export const FRAME_MIN_WIDTH = 160;
export const FRAME_MIN_HEIGHT = 80;
/** Size of a collapsed frame: it looks like one block. */
export const COLLAPSED_HEIGHT = 56;

export type Role = "entry" | "action" | "check" | "control";

const ROLES: Record<NodeType, Role> = {
  start: "entry",
  intercept: "entry",
  click: "action",
  move: "action",
  drag: "action",
  scroll: "action",
  key: "action",
  text: "action",
  wait: "check",
  branch: "check",
  pause: "control",
  counter: "control",
  call: "control",
  stop: "control",
};

const SINGLE: ReadonlyArray<Port> = ["next"];

const PORTS: Record<NodeType, ReadonlyArray<Port>> = {
  start: SINGLE,
  intercept: SINGLE,
  click: SINGLE,
  move: SINGLE,
  drag: SINGLE,
  scroll: SINGLE,
  key: SINGLE,
  text: SINGLE,
  pause: SINGLE,
  call: SINGLE,
  stop: [],
  wait: ["next", "timeout"],
  branch: ["yes", "no"],
  counter: ["body", "done"],
};

/** Order of the list of blocks, grouped by role. */
export const PALETTE: ReadonlyArray<{ role: Role; types: ReadonlyArray<NodeType> }> = [
  { role: "action", types: ["click", "move", "drag", "scroll", "key", "text"] },
  { role: "check", types: ["wait", "branch"] },
  { role: "control", types: ["pause", "counter", "call", "stop"] },
  { role: "entry", types: ["intercept"] },
];

export function roleOf(type: NodeType): Role {
  return ROLES[type];
}

/** Outputs of a block: the first one leaves it downwards, the second one to the right. */
export function portsOf(type: NodeType): ReadonlyArray<Port> {
  return PORTS[type];
}

/** Start and intercept blocks begin a flow; nothing may lead into them. */
export function isEntry(type: NodeType): boolean {
  return type === "start" || type === "intercept";
}

export function edgeKey(edge: { from: string; port: Port }): string {
  return `${edge.from}:${edge.port}`;
}

/** Blocks that count towards the size limit and the list of scenarios. */
export function blockCount(graph: Graph): number {
  return graph.nodes.filter((node) => node.type !== "start").length;
}

/** What the list of scenarios shows: steps of a list or blocks of a graph. */
export function stepCountOf(scenario: Scenario): number {
  return scenario.graph ? blockCount(scenario.graph) : countSteps(scenario.steps);
}

let idSequence = 0;

/** Same shape as the ids made by the core: prefix, milliseconds, a unique tail. */
function newId(prefix: string): string {
  idSequence = (idSequence + 1) % 0x1000;
  const random = Math.floor(Math.random() * 0x1000);
  return `${prefix}${Date.now().toString(16)}${idSequence.toString(16).padStart(3, "0")}${random.toString(16).padStart(3, "0")}`;
}

export function snap(value: number): number {
  return Math.round(value / GRID) * GRID;
}

export function createNode(type: NodeType, mode: ScenarioMode, at: GraphPoint): GraphNode {
  const base = { id: newId("n"), enabled: true, pauseAfter: noPause(), x: at.x, y: at.y };
  switch (type) {
    case "start":
      return { ...base, type };
    case "click":
    case "move":
    case "drag":
    case "scroll":
    case "key":
    case "text":
    case "pause":
    case "stop": {
      const step = createStep(type, mode);
      if (step.type === "wait" || step.type === "if" || step.type === "loop" || step.type === "breakLoop") {
        throw new Error(`Not an action: ${step.type}`);
      }
      return { ...step, id: base.id, x: at.x, y: at.y };
    }
    case "wait":
      return {
        ...base,
        type,
        condition: defaultCondition("imageFound", mode),
        pollMs: DEFAULT_POLL_MS,
        timeoutMs: DEFAULT_WAIT_TIMEOUT_MS,
        onFound: { kind: "none" },
      };
    case "branch":
      return { ...base, type, condition: defaultCondition("imageFound", mode), pollMs: DEFAULT_POLL_MS, timeoutMs: 0 };
    case "counter":
      return { ...base, type, times: 3, pauseBetween: noPause() };
    case "call":
      return { ...base, type, scenarioId: "" };
    case "intercept":
      return { ...base, type, condition: defaultCondition("imageFound", mode), minIntervalMs: DEFAULT_INTERCEPT_MS };
    default: {
      const unreachable: never = type;
      throw new Error(`Unknown block type: ${String(unreachable)}`);
    }
  }
}

export function createNote(kind: NoteKind, text: string, rect: { x: number; y: number; width: number; height: number }): GraphNote {
  return { id: newId("t"), kind, text, collapsed: false, ...rect };
}

// What the canvas knows about the shape of a graph

export interface GraphShape {
  byId: ReadonlyMap<string, GraphNode>;
  /** 1-based number of a block, the same one the journal and the messages use. */
  numbers: ReadonlyMap<string, number>;
  /** `<block id>:<output>` to the id of the block the link leads to; the first link of an output wins. */
  out: ReadonlyMap<string, string>;
  incoming: ReadonlyMap<string, ReadonlyArray<string>>;
  /** Blocks a pass can get to from the start block or from an intercept. */
  reachable: ReadonlySet<string>;
  /** Keys of the links that lead to a block already on the way: loops. */
  back: ReadonlySet<string>;
}

function targetsOf(node: GraphNode, out: ReadonlyMap<string, string>): Array<{ key: string; to: string }> {
  const list: Array<{ key: string; to: string }> = [];
  for (const port of portsOf(node.type)) {
    const key = `${node.id}:${port}`;
    const to = out.get(key);
    if (to !== undefined) {
      list.push({ key, to });
    }
  }
  return list;
}

export function analyse(graph: Graph): GraphShape {
  const byId = new Map<string, GraphNode>();
  const numbers = new Map<string, number>();
  graph.nodes.forEach((node, index) => {
    if (!byId.has(node.id)) {
      byId.set(node.id, node);
      numbers.set(node.id, index + 1);
    }
  });

  const out = new Map<string, string>();
  const incoming = new Map<string, string[]>();
  for (const edge of graph.edges) {
    const from = byId.get(edge.from);
    const key = edgeKey(edge);
    if (!from || !byId.has(edge.to) || !portsOf(from.type).includes(edge.port) || out.has(key)) {
      continue;
    }
    out.set(key, edge.to);
    incoming.set(edge.to, [...(incoming.get(edge.to) ?? []), edge.from]);
  }

  // Depth first from every beginning of a flow: a link to a block that is still open is a loop.
  const state = new Map<string, "open" | "done">();
  const back = new Set<string>();
  const walk = (root: string) => {
    if (state.has(root)) {
      return;
    }
    const stack: Array<{ id: string; next: Array<{ key: string; to: string }> }> = [];
    const enter = (id: string) => {
      const node = byId.get(id);
      state.set(id, "open");
      stack.push({ id, next: node ? targetsOf(node, out).reverse() : [] });
    };
    enter(root);
    while (stack.length > 0) {
      const top = stack[stack.length - 1];
      const link = top.next.pop();
      if (!link) {
        state.set(top.id, "done");
        stack.pop();
        continue;
      }
      const seen = state.get(link.to);
      if (seen === "open") {
        back.add(link.key);
      } else if (seen === undefined) {
        enter(link.to);
      }
    }
  };

  const entries: GraphNode[] = [
    ...graph.nodes.filter((node) => node.type === "start"),
    ...graph.nodes.filter((node) => node.type === "intercept"),
  ];
  for (const node of entries) {
    walk(node.id);
  }
  const reachable = new Set(state.keys());
  for (const node of graph.nodes) {
    walk(node.id);
  }
  return { byId, numbers, out, incoming, reachable, back };
}

/** Everything a pass may go through before it gets to the block, and everything after it. */
export function pathThrough(shape: GraphShape, id: string): { before: Set<string>; after: Set<string> } {
  const before = new Set<string>();
  let queue = [...(shape.incoming.get(id) ?? [])];
  while (queue.length > 0) {
    const next: string[] = [];
    for (const at of queue) {
      if (!before.has(at)) {
        before.add(at);
        next.push(...(shape.incoming.get(at) ?? []));
      }
    }
    queue = next;
  }

  const after = new Set<string>();
  const follow = (from: string): string[] => {
    const node = shape.byId.get(from);
    return node ? targetsOf(node, shape.out).map((link) => link.to) : [];
  };
  queue = follow(id);
  while (queue.length > 0) {
    const next: string[] = [];
    for (const at of queue) {
      if (!after.has(at)) {
        after.add(at);
        next.push(...follow(at));
      }
    }
    queue = next;
  }
  return { before, after };
}

// Frames

function centreOf(node: GraphNode): GraphPoint {
  return { x: (node.x ?? 0) + BLOCK_WIDTH / 2, y: (node.y ?? 0) + BLOCK_HEIGHT / 2 };
}

function holds(frame: GraphNote, point: GraphPoint): boolean {
  return point.x >= frame.x && point.x <= frame.x + frame.width && point.y >= frame.y && point.y <= frame.y + frame.height;
}

/** Blocks and smaller notes that lie inside the rectangle of a frame. */
export function frameMembers(graph: Graph, frame: GraphNote): { nodes: string[]; notes: string[] } {
  const nodes = graph.nodes.filter((node) => holds(frame, centreOf(node))).map((node) => node.id);
  const area = frame.width * frame.height;
  const notes = graph.notes
    .filter((note) => note.id !== frame.id && note.width * note.height < area)
    .filter((note) => holds(frame, { x: note.x + note.width / 2, y: note.y + note.height / 2 }))
    .map((note) => note.id);
  return { nodes, notes };
}

/** For every block and note hidden inside a collapsed frame: the id of that frame. */
export function collapsedOwners(graph: Graph): Map<string, string> {
  const owners = new Map<string, string>();
  for (const frame of graph.notes) {
    if (frame.kind !== "frame" || !frame.collapsed || owners.has(frame.id)) {
      continue;
    }
    const members = frameMembers(graph, frame);
    for (const id of [...members.nodes, ...members.notes]) {
      if (!owners.has(id)) {
        owners.set(id, frame.id);
      }
    }
  }
  return owners;
}

/** Rectangle of a frame that holds the given blocks; null when there are none. */
export function frameAround(graph: Graph, ids: ReadonlySet<string>): { x: number; y: number; width: number; height: number } | null {
  const chosen = graph.nodes.filter((node) => ids.has(node.id));
  if (chosen.length === 0) {
    return null;
  }
  const xs = chosen.map((node) => node.x ?? 0);
  const ys = chosen.map((node) => node.y ?? 0);
  const left = Math.min(...xs);
  const top = Math.min(...ys);
  return {
    x: left - FRAME_MARGIN,
    y: top - FRAME_MARGIN - FRAME_TITLE,
    width: Math.max(...xs) - left + BLOCK_WIDTH + FRAME_MARGIN * 2,
    height: Math.max(...ys) - top + BLOCK_HEIGHT + FRAME_MARGIN * 2 + FRAME_TITLE,
  };
}

// Edits

export function updateNode(graph: Graph, id: string, fn: (node: GraphNode) => GraphNode): Graph {
  let changed = false;
  const nodes = graph.nodes.map((node) => {
    if (node.id !== id) {
      return node;
    }
    const next = fn(node);
    changed ||= next !== node;
    return next;
  });
  return changed ? { ...graph, nodes } : graph;
}

export function updateNote(graph: Graph, id: string, fn: (note: GraphNote) => GraphNote): Graph {
  let changed = false;
  const notes = graph.notes.map((note) => {
    if (note.id !== id) {
      return note;
    }
    const next = fn(note);
    changed ||= next !== note;
    return next;
  });
  return changed ? { ...graph, notes } : graph;
}

/** Puts blocks and notes at new places; ids that are not in the map stay where they are. */
export function moveItems(graph: Graph, places: ReadonlyMap<string, GraphPoint>): Graph {
  let changed = false;
  const nodes = graph.nodes.map((node) => {
    const place = places.get(node.id);
    if (!place || (node.x === place.x && node.y === place.y)) {
      return node;
    }
    changed = true;
    return { ...node, x: place.x, y: place.y };
  });
  const notes = graph.notes.map((note) => {
    const place = places.get(note.id);
    if (!place || (note.x === place.x && note.y === place.y)) {
      return note;
    }
    changed = true;
    return { ...note, x: place.x, y: place.y };
  });
  return changed ? { ...graph, nodes, notes } : graph;
}

/** The start block stays while it is the only one: a graph without it does not run. */
export function removable(graph: Graph, id: string): boolean {
  const node = graph.nodes.find((item) => item.id === id);
  if (!node) {
    return graph.notes.some((note) => note.id === id);
  }
  return node.type !== "start" || graph.nodes.filter((item) => item.type === "start").length > 1;
}

/** Removes blocks with their links, notes, and links given by their keys. */
export function removeItems(graph: Graph, ids: ReadonlySet<string>, edges: ReadonlySet<string>): Graph {
  const gone = new Set([...ids].filter((id) => removable(graph, id)));
  if (gone.size === 0 && edges.size === 0) {
    return graph;
  }
  return {
    nodes: graph.nodes.filter((node) => !gone.has(node.id)),
    edges: graph.edges.filter((edge) => !gone.has(edge.from) && !gone.has(edge.to) && !edges.has(edgeKey(edge))),
    notes: graph.notes.filter((note) => !gone.has(note.id)),
  };
}

/** Whether a link from that output may lead into that block. */
export function canConnect(graph: Graph, from: string, port: Port, to: string): boolean {
  const source = graph.nodes.find((node) => node.id === from);
  const target = graph.nodes.find((node) => node.id === to);
  return Boolean(source && target && portsOf(source.type).includes(port) && !isEntry(target.type));
}

/** Links an output to a block. One link may leave an output, so the old one goes away. */
export function connect(graph: Graph, from: string, port: Port, to: string): Graph {
  if (!canConnect(graph, from, port, to)) {
    return graph;
  }
  const existing = graph.edges.find((edge) => edge.from === from && edge.port === port);
  if (existing && existing.to === to) {
    return graph;
  }
  const edges = graph.edges.filter((edge) => !(edge.from === from && edge.port === port));
  edges.push({ from, port, to, points: [] });
  return { ...graph, edges };
}

export function updateEdge(graph: Graph, key: string, fn: (edge: GraphEdge) => GraphEdge): Graph {
  let changed = false;
  const edges = graph.edges.map((edge) => {
    if (edgeKey(edge) !== key) {
      return edge;
    }
    const next = fn(edge);
    changed ||= next !== edge;
    return next;
  });
  return changed ? { ...graph, edges } : graph;
}

/** A piece of a graph: blocks and the links between them. */
export interface Fragment {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

/** The chosen blocks (never the start block) with the links between them. */
export function copyFragment(graph: Graph, ids: ReadonlySet<string>): Fragment {
  const nodes = graph.nodes.filter((node) => ids.has(node.id) && node.type !== "start");
  const kept = new Set(nodes.map((node) => node.id));
  return { nodes, edges: graph.edges.filter((edge) => kept.has(edge.from) && kept.has(edge.to)) };
}

/** Adds a fragment under fresh ids, its top left corner at `at`. Returns the new ids. */
export function insertFragment(graph: Graph, fragment: Fragment, at: GraphPoint): { graph: Graph; ids: string[] } {
  const source = fragment.nodes.filter((node) => node.type !== "start");
  if (source.length === 0) {
    return { graph, ids: [] };
  }
  const placed = source.map((node, index) => ({ node, x: node.x ?? 0, y: node.y ?? index * ROW }));
  const left = Math.min(...placed.map((item) => item.x));
  const top = Math.min(...placed.map((item) => item.y));
  const renamed = new Map<string, string>();
  const nodes = placed.map(({ node, x, y }) => {
    const id = newId("n");
    renamed.set(node.id, id);
    return { ...node, id, x: snap(at.x + x - left), y: snap(at.y + y - top) };
  });
  const edges: GraphEdge[] = [];
  for (const edge of fragment.edges) {
    const from = renamed.get(edge.from);
    const to = renamed.get(edge.to);
    if (from !== undefined && to !== undefined) {
      edges.push({ from, port: edge.port, to, points: [] });
    }
  }
  return {
    graph: { ...graph, nodes: [...graph.nodes, ...nodes], edges: [...graph.edges, ...edges] },
    ids: nodes.map((node) => node.id),
  };
}

/** Places every block again: the first output goes straight down, the others open a column to the right. */
export function arrange(graph: Graph): Graph {
  const shape = analyse(graph);
  const spots = new Map<string, { column: number; row: number }>();
  let nextColumn = 0;
  const roots = [
    ...graph.nodes.filter((node) => node.type === "start"),
    ...graph.nodes.filter((node) => node.type === "intercept"),
    ...graph.nodes,
  ];
  for (const root of roots) {
    if (spots.has(root.id)) {
      continue;
    }
    const stack = [{ id: root.id, column: nextColumn, row: 0 }];
    nextColumn += 1;
    while (stack.length > 0) {
      const at = stack.pop();
      if (!at || spots.has(at.id)) {
        continue;
      }
      spots.set(at.id, { column: at.column, row: at.row });
      const node = shape.byId.get(at.id);
      const pending: Array<{ id: string; column: number; row: number }> = [];
      (node ? targetsOf(node, shape.out) : []).forEach((link, order) => {
        if (spots.has(link.to)) {
          return;
        }
        if (order === 0) {
          pending.push({ id: link.to, column: at.column, row: at.row + 1 });
        } else {
          pending.push({ id: link.to, column: nextColumn, row: at.row + 1 });
          nextColumn += 1;
        }
      });
      // The straight-down output is taken first.
      stack.push(...pending.reverse());
    }
  }
  const places = new Map<string, GraphPoint>();
  for (const [id, spot] of spots) {
    places.set(id, { x: spot.column * COLUMN, y: spot.row * ROW });
  }
  const moved = moveItems({ ...graph, notes: [] }, places);
  // Bend points were put for the old places.
  const edges = graph.edges.some((edge) => edge.points.length > 0)
    ? graph.edges.map((edge) => (edge.points.length > 0 ? { ...edge, points: [] } : edge))
    : graph.edges;
  return moved.nodes === graph.nodes && edges === graph.edges ? graph : { ...graph, nodes: moved.nodes, edges };
}

export type ExtractProblem = "extractEmpty" | "extractEntry" | "extractManyEntries" | "extractManyExits";

/** Why the chosen blocks cannot become a scenario of their own; the same rules as in the core. */
export function extractProblem(graph: Graph, ids: ReadonlySet<string>): ExtractProblem | null {
  const chosen = graph.nodes.filter((node) => ids.has(node.id));
  if (chosen.length === 0) {
    return "extractEmpty";
  }
  if (chosen.some((node) => isEntry(node.type))) {
    return "extractEntry";
  }
  const inside = (id: string) => ids.has(id) && chosen.some((node) => node.id === id);

  const entries = new Set(graph.edges.filter((edge) => !inside(edge.from) && inside(edge.to)).map((edge) => edge.to));
  if (entries.size === 0) {
    for (const node of chosen) {
      if (!graph.edges.some((edge) => edge.to === node.id && edge.from !== node.id)) {
        entries.add(node.id);
      }
    }
  }
  if (entries.size !== 1) {
    return "extractManyEntries";
  }

  const exits = new Set(graph.edges.filter((edge) => inside(edge.from) && !inside(edge.to)).map((edge) => edge.to));
  const endsInside = chosen.some((node) =>
    portsOf(node.type)
      .filter((port) => port !== "timeout")
      .some((port) => !graph.edges.some((edge) => edge.from === node.id && edge.port === port)),
  );
  return exits.size + (endsInside ? 1 : 0) > 1 ? "extractManyExits" : null;
}

/** JSON with sorted keys: the core and the editor write the same fields in a different order. */
function stable(value: unknown): string {
  return JSON.stringify(value, (_key, item: unknown) => {
    if (typeof item !== "object" || item === null || Array.isArray(item)) {
      return item;
    }
    const source = item as Record<string, unknown>;
    const sorted: Record<string, unknown> = {};
    for (const key of Object.keys(source).sort()) {
      sorted[key] = source[key];
    }
    return sorted;
  });
}

function withoutPlace(node: GraphNode): string {
  const { x: _x, y: _y, ...rest } = node;
  return stable(rest);
}

/** Blocks of `after` that are new or differ from `before`, their place aside. */
export function changedNodes(before: Graph, after: Graph): string[] {
  const old = new Map(before.nodes.map((node) => [node.id, node]));
  const links = (graph: Graph, id: string) =>
    graph.edges
      .filter((edge) => edge.from === id)
      .map((edge) => `${edge.port}>${edge.to}`)
      .sort()
      .join("|");
  return after.nodes
    .filter((node) => {
      const was = old.get(node.id);
      if (!was) {
        return true;
      }
      return (was !== node && withoutPlace(was) !== withoutPlace(node)) || links(before, node.id) !== links(after, node.id);
    })
    .map((node) => node.id);
}

export function sameGraph(a: Graph, b: Graph): boolean {
  return a === b || stable(a) === stable(b);
}
