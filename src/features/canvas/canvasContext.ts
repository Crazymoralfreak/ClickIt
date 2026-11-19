// What the blocks, links and notes of the canvas get from the editor around them.

import type { Edge, Node } from "@xyflow/react";
import { createContext, useContext } from "react";
import type { GraphNode, GraphNote, GraphPoint } from "../../types/scenario";
import type { PortTone } from "./blockText";
import type { TraceStore } from "./useRunTrace";

/** Everything a block shows that does not change while a run goes on. */
export type BlockData = {
  node: GraphNode;
  /** 1-based number, the same one the journal and the messages use. */
  number: number;
  title: string;
  summary: string;
  /** Outputs that have a link, joined with commas. */
  wired: string;
  issue: "error" | "warning" | null;
  issueText: string;
  breakpoint: boolean;
  /** Not on the path through the selected block. */
  dim: boolean;
  /** No pass can get here. */
  unreachable: boolean;
  /** Added or changed from outside a moment ago. */
  changed: boolean;
  locked: boolean;
};

export type NoteData = {
  note: GraphNote;
  /** Ids of the blocks inside a frame, joined with commas. */
  members: string;
  blocks: number;
  locked: boolean;
};

export type BlockFlowNode = Node<BlockData, "block">;
export type NoteFlowNode = Node<NoteData, "note">;
export type CanvasNode = BlockFlowNode | NoteFlowNode;

export type FlowEdgeData = {
  /** `<block id>:<output>`, the key of the link in the graph and in the trace. */
  key: string;
  tone: PortTone;
  /** Leads to a block already on the way: a loop. */
  back: boolean;
  points: GraphPoint[];
  dim: boolean;
  /** False for a link drawn to a collapsed frame: its bend points are hidden. */
  editable: boolean;
};

export type CanvasEdge = Edge<FlowEdgeData, "flow">;

export interface CanvasApi {
  store: TraceStore;
  scenarioId: string;
  toggleBreakpoint: (id: string) => void;
  toggleCollapsed: (id: string) => void;
  resizeNote: (id: string, rect: { x: number; y: number; width: number; height: number }) => void;
  /** Replaces the bend points of a link. */
  setBends: (key: string, points: GraphPoint[]) => void;
}

export const CanvasContext = createContext<CanvasApi | null>(null);

export function useCanvas(): CanvasApi {
  const value = useContext(CanvasContext);
  if (!value) {
    throw new Error("useCanvas must be used inside the canvas editor");
  }
  return value;
}

/** Colours a note or a frame may have; the name is what the graph stores. */
export const NOTE_COLORS: ReadonlyArray<{ name: string; css: string }> = [
  { name: "yellow", css: "var(--warning)" },
  { name: "green", css: "var(--ok)" },
  { name: "blue", css: "var(--accent)" },
  { name: "red", css: "var(--danger)" },
  { name: "violet", css: "#8b5cf6" },
];

export function noteColor(name: string | undefined): string {
  return NOTE_COLORS.find((color) => color.name === name)?.css ?? "var(--text-dim)";
}
