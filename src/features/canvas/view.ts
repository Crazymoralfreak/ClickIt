// From a graph to what the canvas draws: nodes and edges of the flow, with the
// state of the editor around them (issues, breakpoints, the path through the selection).

import type { Translator } from "../../i18n";
import { BLOCK_WIDTH, type Graph, type Port } from "../../types/scenario";
import { NO_ISSUES, type IssueMap } from "../editor/editorContext";
import { blockSummary, blockTitle, portTone } from "./blockText";
import type { BlockData, CanvasEdge, CanvasNode, NoteData } from "./canvasContext";
import {
  arrange,
  collapsedOwners,
  edgeKey,
  frameMembers,
  pathThrough,
  portsOf,
  COLLAPSED_HEIGHT,
  type GraphShape,
} from "./graph";

const PORTS: ReadonlyArray<Port> = ["next", "timeout", "yes", "no", "body", "done"];

export function isPort(value: string | null | undefined): value is Port {
  return PORTS.some((port) => port === value);
}

function sameFields<T extends object>(a: T, b: T): boolean {
  return (Object.keys(a) as Array<keyof T>).every((key) => a[key] === b[key]);
}

export interface ViewInput {
  graph: Graph;
  shape: GraphShape;
  issues: IssueMap;
  breakpoints: ReadonlySet<string>;
  /** Blocks changed from outside a moment ago. */
  changedIds: ReadonlySet<string>;
  /** The only selected block: the path through it is shown, the rest is dimmed. */
  focusId: string | null;
  locked: boolean;
  scenarioNames: ReadonlyMap<string, string>;
  translator: Translator;
}

export interface View {
  nodes: CanvasNode[];
  edges: CanvasEdge[];
  /** For a block or a note hidden in a collapsed frame: the frame that is shown instead. */
  shownAs: ReadonlyMap<string, string>;
}

export function buildView({
  graph,
  shape,
  issues,
  breakpoints,
  changedIds,
  focusId,
  locked,
  scenarioNames,
  translator,
}: ViewInput): View {
  const owners = collapsedOwners(graph);
  const shownAs = new Map<string, string>();
  for (const id of owners.keys()) {
    let at = id;
    // A collapsed frame may lie inside another collapsed frame.
    for (let hops = 0; hops < graph.notes.length; hops += 1) {
      const owner = owners.get(at);
      if (owner === undefined) {
        break;
      }
      at = owner;
    }
    shownAs.set(id, at);
  }

  // A block saved without a place is shown where the automatic layout would put it.
  const unplaced = graph.nodes.some((node) => node.x === undefined || node.y === undefined);
  const arranged = unplaced ? new Map(arrange(graph).nodes.map((node) => [node.id, node])) : null;

  const wired = new Map<string, Port[]>();
  for (const key of shape.out.keys()) {
    const at = key.lastIndexOf(":");
    const port = key.slice(at + 1);
    if (isPort(port)) {
      const id = key.slice(0, at);
      wired.set(id, [...(wired.get(id) ?? []), port]);
    }
  }

  const path = focusId === null ? null : pathThrough(shape, focusId);
  const upstream = path && focusId !== null ? new Set([focusId, ...path.before]) : null;
  const downstream = path && focusId !== null ? new Set([focusId, ...path.after]) : null;
  const onPath = (id: string) => upstream === null || downstream === null || upstream.has(id) || downstream.has(id);

  const noteNodes: CanvasNode[] = graph.notes.map((note) => {
    const frame = note.kind === "frame";
    const members = frame ? frameMembers(graph, note).nodes : [];
    const open = frame && !note.collapsed;
    const data: NoteData = { note, members: members.join(","), blocks: members.length, locked };
    return {
      id: note.id,
      type: "note",
      position: { x: note.x, y: note.y },
      data,
      width: frame && note.collapsed ? BLOCK_WIDTH : note.width,
      height: frame && note.collapsed ? COLLAPSED_HEIGHT : note.height,
      // An open frame lies under the blocks and the links it holds.
      zIndex: open ? -1 : 0,
      hidden: shownAs.has(note.id),
      className: open ? "canvas-frame-node" : undefined,
      dragHandle: open ? ".frame-head" : undefined,
    };
  });

  const blockNodes: CanvasNode[] = [];
  for (const node of graph.nodes) {
    // Two blocks with one id are reported by the checks; only the first one can be drawn.
    if (shape.byId.get(node.id) !== node) {
      continue;
    }
    const list = issues.get(node.id) ?? NO_ISSUES;
    const placed = arranged?.get(node.id);
    const data: BlockData = {
      node,
      number: shape.numbers.get(node.id) ?? 0,
      title: blockTitle(node, translator),
      summary: blockSummary(node, translator, scenarioNames),
      wired: (wired.get(node.id) ?? []).join(","),
      issue: list.length === 0 ? null : list.some((issue) => issue.severity === "error") ? "error" : "warning",
      issueText: list.map((issue) => translator.td(`issues.${issue.code}`)).join("\n"),
      breakpoint: breakpoints.has(node.id),
      dim: !onPath(node.id),
      unreachable: !shape.reachable.has(node.id),
      changed: changedIds.has(node.id),
      locked,
    };
    blockNodes.push({
      id: node.id,
      type: "block",
      position: { x: node.x ?? placed?.x ?? 0, y: node.y ?? placed?.y ?? 0 },
      data,
      hidden: shownAs.has(node.id),
    });
  }

  const edges: CanvasEdge[] = [];
  const drawn = new Set<string>();
  for (const edge of graph.edges) {
    const from = shape.byId.get(edge.from);
    const key = edgeKey(edge);
    if (!from || !shape.byId.has(edge.to) || !portsOf(from.type).includes(edge.port) || drawn.has(key)) {
      continue;
    }
    drawn.add(key);
    const source = shownAs.get(edge.from) ?? edge.from;
    const target = shownAs.get(edge.to) ?? edge.to;
    // Both ends are inside one collapsed frame.
    if (source === target && shownAs.has(edge.from)) {
      continue;
    }
    const lit =
      upstream === null ||
      downstream === null ||
      (upstream.has(edge.from) && upstream.has(edge.to)) ||
      (downstream.has(edge.from) && downstream.has(edge.to));
    edges.push({
      id: key,
      type: "flow",
      source,
      target,
      sourceHandle: shownAs.has(edge.from) ? "out" : edge.port,
      targetHandle: "in",
      data: {
        key,
        tone: portTone(from.type, edge.port),
        back: shape.back.has(key),
        points: edge.points,
        dim: !lit,
        editable: !shownAs.has(edge.from) && !shownAs.has(edge.to),
      },
    });
  }

  return { nodes: [...noteNodes, ...blockNodes], edges, shownAs };
}

/**
 * The nodes of a new view, with what only the canvas knows kept from the old ones:
 * the selection, the measured size, the place of a block being dragged. A node that
 * did not change keeps its identity, so it is not drawn again. `select` replaces the selection.
 */
export function mergeNodes(
  current: ReadonlyArray<CanvasNode>,
  next: ReadonlyArray<CanvasNode>,
  select: ReadonlySet<string> | null,
): CanvasNode[] {
  const old = new Map(current.map((node) => [node.id, node]));
  return next.map((node) => {
    const was = old.get(node.id);
    const selected = node.hidden ? false : select ? select.has(node.id) : (was?.selected ?? false);
    if (!was || was.type !== node.type) {
      return { ...node, selected } as CanvasNode;
    }
    const data = sameFields<object>(was.data, node.data) ? was.data : node.data;
    const position =
      was.dragging || (was.position.x === node.position.x && was.position.y === node.position.y)
        ? was.position
        : node.position;
    if (
      data === was.data &&
      position === was.position &&
      (was.selected ?? false) === selected &&
      (was.hidden ?? false) === (node.hidden ?? false) &&
      was.width === node.width &&
      was.height === node.height &&
      was.zIndex === node.zIndex &&
      was.className === node.className &&
      was.dragHandle === node.dragHandle
    ) {
      return was;
    }
    return {
      ...node,
      data,
      position,
      selected,
      dragging: was.dragging,
      // A node whose size is the same does not have to be measured again.
      measured: was.width === node.width && was.height === node.height ? was.measured : undefined,
    } as CanvasNode;
  });
}

/** The edges of a new view; unchanged ones keep their identity and their selection. */
export function mergeEdges(
  current: ReadonlyArray<CanvasEdge>,
  next: ReadonlyArray<CanvasEdge>,
  deselect: boolean,
): CanvasEdge[] {
  const old = new Map(current.map((edge) => [edge.id, edge]));
  return next.map((edge) => {
    const was = old.get(edge.id);
    if (!was) {
      return edge;
    }
    const same =
      was.source === edge.source &&
      was.target === edge.target &&
      was.sourceHandle === edge.sourceHandle &&
      was.data !== undefined &&
      edge.data !== undefined &&
      sameFields(was.data, edge.data) &&
      !(deselect && was.selected);
    return same ? was : { ...edge, selected: deselect ? false : was.selected };
  });
}
