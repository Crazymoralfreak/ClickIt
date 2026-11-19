import {
  Background,
  Controls,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  useNodesInitialized,
  useReactFlow,
  type Connection,
  type EdgeTypes,
  type FinalConnectionState,
  type NodeTypes,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import {
  BLOCK_HEIGHT,
  BLOCK_WIDTH,
  type Graph,
  type GraphNode,
  type GraphNote,
  type GraphPoint,
  type Port,
  type Scenario,
  type ScenarioSummary,
  type Snippet,
} from "../../types/scenario";
import { Button, Checkbox, cx } from "../../ui";
import { NO_ISSUES, type IssueMap } from "../editor/editorContext";
import { BlockNode } from "./BlockNode";
import { CanvasContext, type CanvasApi, type CanvasEdge, type CanvasNode } from "./canvasContext";
import { FlowEdge } from "./FlowEdge";
import {
  analyse,
  arrange,
  blockCount,
  canConnect,
  connect,
  copyFragment,
  createNode,
  createNote,
  edgeKey,
  extractProblem,
  frameAround,
  frameMembers,
  insertFragment,
  isEntry,
  moveItems,
  portsOf,
  removable,
  removeItems,
  roleOf,
  snap,
  updateEdge,
  updateNode,
  updateNote,
  GRID,
  MAX_BLOCKS,
  NOTE_HEIGHT,
  NOTE_WIDTH,
  type Fragment,
  type Role,
} from "./graph";
import { Inspector, type InspectorTarget } from "./Inspector";
import { NoteNode } from "./NoteNode";
import { CanvasMenu, Palette, PalettePopup, type MenuItem } from "./Palette";
import { builtinFragment, type PaletteItem } from "./snippets";
import { useGraphHistory } from "./useGraphHistory";
import { useActiveNode, useHasTrace, useRunTrace } from "./useRunTrace";
import { buildView, isPort, mergeEdges, mergeNodes } from "./view";
import "./canvas.css";

const NODE_TYPES: NodeTypes = { block: BlockNode, note: NoteNode };
const EDGE_TYPES: EdgeTypes = { flow: FlowEdge };

const ROLE_COLORS: Record<Role, string> = {
  entry: "var(--ok)",
  action: "var(--accent)",
  check: "var(--warning)",
  control: "var(--text-dim)",
};

const FRAME_WIDTH = 320;
const FRAME_HEIGHT = 200;
/** A copy is put this far from the blocks it was made from. */
const COPY_SHIFT = 40;
/** The working block is brought back into view when it gets closer to the edge than this. */
const FOLLOW_MARGIN = 40;
const POPUP_WIDTH = 230;
const POPUP_HEIGHT = 340;

function samePoints(a: ReadonlyArray<GraphPoint>, b: ReadonlyArray<GraphPoint>): boolean {
  return a.length === b.length && a.every((point, index) => point.x === b[index].x && point.y === b[index].y);
}

/** Distance from a point to a piece of a link. */
function distanceToSegment(point: GraphPoint, from: GraphPoint, to: GraphPoint): number {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const length = dx * dx + dy * dy;
  const along = length === 0 ? 0 : Math.max(0, Math.min(1, ((point.x - from.x) * dx + (point.y - from.y) * dy) / length));
  return Math.hypot(point.x - (from.x + along * dx), point.y - (from.y + along * dy));
}

type Popup =
  | { kind: "palette"; x: number; y: number; at: GraphPoint; from: { node: string; port: Port } | null }
  | { kind: "menu"; x: number; y: number; nodeId: string };

export interface CanvasEditorProps {
  draft: Scenario;
  graph: Graph;
  change: (fn: (scenario: Scenario) => Scenario) => void;
  /** Reads the scenario from disk again, after the core changed it. */
  reload: () => void;
  issues: IssueMap;
  locked: boolean;
  breakpoints: ReadonlySet<string>;
  onToggleBreakpoint: (id: string) => void;
  onRunFrom: (id: string) => void;
  /** The list of scenarios beside the editor is hidden to give the canvas more room. */
  listHidden?: boolean;
  onToggleList?: () => void;
}

/** The canvas of a scenario: blocks, the links between their outputs, notes and frames. */
export function CanvasEditor(props: CanvasEditorProps) {
  return (
    <ReactFlowProvider>
      <CanvasBody {...props} />
    </ReactFlowProvider>
  );
}

function CanvasBody({
  draft,
  graph,
  change,
  reload,
  issues,
  locked,
  breakpoints,
  onToggleBreakpoint,
  onRunFrom,
  listHidden,
  onToggleList,
}: CanvasEditorProps) {
  const translator = useT();
  const { t, td } = translator;
  const { notify, showError, askName, confirm, flushAll, touch, dataVersion } = useApp();
  const flow = useReactFlow<CanvasNode, CanvasEdge>();
  const store = useRunTrace(draft.id);
  const active = useActiveNode(store);
  const hasTrace = useHasTrace(store);
  const root = useRef<HTMLDivElement>(null);
  const flowBox = useRef<HTMLDivElement>(null);

  const [nodes, setNodes] = useState<CanvasNode[]>([]);
  const [edges, setEdges] = useState<CanvasEdge[]>([]);
  const [paletteOpen, setPaletteOpen] = useState(true);
  const [snapOn, setSnapOn] = useState(true);
  const [follow, setFollow] = useState(true);
  /** The block or note whose panel the user closed; it opens again with another selection. */
  const [closedFor, setClosedFor] = useState<string | null>(null);
  const [popup, setPopup] = useState<Popup | null>(null);
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [scenarios, setScenarios] = useState<ScenarioSummary[]>([]);

  const graphRef = useRef(graph);
  graphRef.current = graph;
  const nodesRef = useRef(nodes);
  nodesRef.current = nodes;
  const lockedRef = useRef(locked);
  lockedRef.current = locked;
  const toggleBreakpointRef = useRef(onToggleBreakpoint);
  toggleBreakpointRef.current = onToggleBreakpoint;

  const { apply, undo, redo, canUndo, canRedo, changedIds } = useGraphHistory(graph, change, lockedRef);

  // Lists the panels need

  useEffect(() => {
    let cancelled = false;
    api
      .scenarioList()
      .then((list) => {
        if (!cancelled) {
          setScenarios(list);
        }
      })
      .catch(showError);
    return () => {
      cancelled = true;
    };
  }, [dataVersion.scenarios, showError]);

  const loadSnippets = useCallback(() => {
    api.snippetList().then(setSnippets).catch(showError);
  }, [showError]);
  useEffect(loadSnippets, [loadSnippets]);

  // From the graph to what the canvas draws

  const shape = useMemo(() => analyse(graph), [graph]);
  const scenarioNames = useMemo(() => new Map(scenarios.map((item) => [item.id, item.name])), [scenarios]);

  const selectedKey = nodes
    .filter((node) => node.selected && !node.hidden)
    .map((node) => node.id)
    .join(",");
  const selected = useMemo(() => (selectedKey === "" ? [] : selectedKey.split(",")), [selectedKey]);
  const selectedBlocks = useMemo(() => selected.filter((id) => shape.byId.has(id)), [selected, shape]);
  const focusId = selected.length === 1 && shape.byId.has(selected[0]) ? selected[0] : null;

  const built = useMemo(
    () => buildView({ graph, shape, issues, breakpoints, changedIds, focusId, locked, scenarioNames, translator }),
    [graph, shape, issues, breakpoints, changedIds, focusId, locked, scenarioNames, translator],
  );

  /** Ids to select once the blocks that were just added are on the canvas. */
  const pendingSelect = useRef<ReadonlySet<string> | null>(null);

  useEffect(() => {
    const pending = pendingSelect.current;
    pendingSelect.current = null;
    setNodes((current) => mergeNodes(current, built.nodes, pending));
    setEdges((current) => mergeEdges(current, built.edges, pending !== null));
  }, [built]);

  const selectOnly = useCallback((ids: ReadonlyArray<string>) => {
    const wanted = new Set(ids);
    // Blocks that are not on the canvas yet are selected when they get there.
    pendingSelect.current = ids.every((id) => nodesRef.current.some((node) => node.id === id)) ? null : wanted;
    setNodes((current) =>
      current.map((node) => ((node.selected ?? false) === wanted.has(node.id) ? node : { ...node, selected: wanted.has(node.id) })),
    );
    setEdges((current) => current.map((edge) => (edge.selected ? { ...edge, selected: false } : edge)));
  }, []);

  // The first look at a scenario shows all of it.
  const initialized = useNodesInitialized();
  const fitted = useRef(false);
  useEffect(() => {
    if (initialized && !fitted.current) {
      fitted.current = true;
      void flow.fitView({ padding: 0.15, maxZoom: 1 });
    }
  }, [initialized, flow]);

  // The run

  useEffect(() => {
    if (!follow || active === null) {
      return;
    }
    const shownId = built.shownAs.get(active) ?? active;
    const node = nodesRef.current.find((item) => item.id === shownId);
    const box = flowBox.current?.getBoundingClientRect();
    if (!node || !box) {
      return;
    }
    const view = flow.getViewport();
    const width = node.measured?.width ?? BLOCK_WIDTH;
    const height = node.measured?.height ?? BLOCK_HEIGHT;
    const left = node.position.x * view.zoom + view.x;
    const top = node.position.y * view.zoom + view.y;
    const outside =
      left < FOLLOW_MARGIN ||
      top < FOLLOW_MARGIN ||
      left + width * view.zoom > box.width - FOLLOW_MARGIN ||
      top + height * view.zoom > box.height - FOLLOW_MARGIN;
    if (outside) {
      const calm = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      void flow.setCenter(node.position.x + width / 2, node.position.y + height / 2, {
        zoom: view.zoom,
        duration: calm ? 0 : 300,
      });
    }
  }, [active, follow, flow, built.shownAs]);

  const clearTrace = () => {
    store.clear();
    api.runTraceClear(draft.id).catch(showError);
  };

  // What blocks, links and notes may ask for

  const canvasApi = useMemo<CanvasApi>(
    () => ({
      store,
      scenarioId: draft.id,
      toggleBreakpoint: (id) => toggleBreakpointRef.current(id),
      toggleCollapsed: (id) => apply((current) => updateNote(current, id, (note) => ({ ...note, collapsed: !note.collapsed }))),
      resizeNote: (id, rect) =>
        apply((current) =>
          updateNote(current, id, (note) => ({
            ...note,
            x: Math.round(rect.x),
            y: Math.round(rect.y),
            width: Math.round(rect.width),
            height: Math.round(rect.height),
          })),
        ),
      setBends: (key, points) =>
        apply((current) => updateEdge(current, key, (edge) => (samePoints(edge.points, points) ? edge : { ...edge, points }))),
    }),
    [store, draft.id, apply],
  );

  // Adding

  const viewCentre = (): GraphPoint => {
    const box = flowBox.current?.getBoundingClientRect();
    return box
      ? flow.screenToFlowPosition({ x: box.left + box.width / 2, y: box.top + box.height / 2 })
      : { x: 0, y: 0 };
  };

  /** A free place for a new block near `corner`: a block is never put right on top of another one. */
  const freeCorner = (corner: GraphPoint): GraphPoint => {
    let at = { x: snap(corner.x), y: snap(corner.y) };
    const taken = (point: GraphPoint) => graphRef.current.nodes.some((node) => node.x === point.x && node.y === point.y);
    for (let tries = 0; tries < 50 && taken(at); tries += 1) {
      at = { x: at.x + GRID * 2, y: at.y + GRID * 2 };
    }
    return at;
  };

  /** Puts blocks on the canvas with their top left corner at `corner` and selects them. */
  const place = (fragment: Fragment, corner: GraphPoint, from: { node: string; port: Port } | null) => {
    if (locked) {
      return;
    }
    const current = graphRef.current;
    const only = fragment.nodes.length === 1 ? fragment.nodes[0] : null;
    if (only && only.type === "start") {
      if (!current.nodes.some((node) => node.type === "start")) {
        const start = { ...only, ...freeCorner(corner) };
        apply((before) => ({ ...before, nodes: [start, ...before.nodes] }));
        selectOnly([start.id]);
      }
      return;
    }
    if (blockCount(current) + fragment.nodes.length > MAX_BLOCKS) {
      notify(t("canvas.limit", { max: MAX_BLOCKS }), "error");
      return;
    }
    let added: string[] = [];
    apply((before) => {
      const result = insertFragment(before, fragment, freeCorner(corner));
      added = result.ids;
      const first = result.graph.nodes.find((node) => added.includes(node.id) && !isEntry(node.type));
      return from && first ? connect(result.graph, from.node, from.port, first.id) : result.graph;
    });
    if (added.length > 0) {
      selectOnly(added);
    }
  };

  /** `at` is where the pointer was; null puts the blocks in the middle of what is on screen. */
  const addItem = async (item: PaletteItem, at: GraphPoint | null, from: { node: string; port: Port } | null) => {
    if (locked) {
      return;
    }
    let fragment: Fragment;
    switch (item.kind) {
      case "block":
        fragment = { nodes: [createNode(item.type, draft.mode, { x: 0, y: 0 })], edges: [] };
        break;
      case "builtin":
        fragment = builtinFragment(item.id, draft.mode);
        break;
      case "snippet":
        try {
          // The template images of the snippet are copied into this scenario first.
          const used = await api.snippetUse(item.id, draft.id);
          fragment = { nodes: used.nodes, edges: used.edges };
        } catch (error) {
          showError(error);
          return;
        }
        break;
      default: {
        const unreachable: never = item;
        throw new Error(`Unknown item: ${JSON.stringify(unreachable)}`);
      }
    }
    const point = at ?? viewCentre();
    // A block made by a dropped link hangs under the pointer; otherwise the pointer is its middle.
    const corner = from
      ? { x: point.x - BLOCK_WIDTH / 2, y: point.y }
      : { x: point.x - BLOCK_WIDTH / 2, y: point.y - BLOCK_HEIGHT / 2 };
    place(fragment, corner, from);
  };

  const dropItem = (item: PaletteItem, clientX: number, clientY: number) => {
    const box = flowBox.current?.getBoundingClientRect();
    if (!box || clientX < box.left || clientX > box.right || clientY < box.top || clientY > box.bottom) {
      return;
    }
    void addItem(item, flow.screenToFlowPosition({ x: clientX, y: clientY }), null);
  };

  const addNote = () => {
    const centre = viewCentre();
    const note = createNote("note", "", {
      x: snap(centre.x - NOTE_WIDTH / 2),
      y: snap(centre.y - NOTE_HEIGHT / 2),
      width: NOTE_WIDTH,
      height: NOTE_HEIGHT,
    });
    apply((current) => ({ ...current, notes: [...current.notes, note] }));
    selectOnly([note.id]);
  };

  const addFrame = () => {
    const centre = viewCentre();
    const rect = frameAround(graph, new Set(selectedBlocks)) ?? {
      x: snap(centre.x - FRAME_WIDTH / 2),
      y: snap(centre.y - FRAME_HEIGHT / 2),
      width: FRAME_WIDTH,
      height: FRAME_HEIGHT,
    };
    const frame = createNote("frame", "", rect);
    apply((current) => ({ ...current, notes: [...current.notes, frame] }));
    selectOnly([frame.id]);
  };

  // Selection

  const removeIds = (ids: ReadonlyArray<string>, links: ReadonlyArray<string>) => {
    apply((current) => removeItems(current, new Set(ids), new Set(links)));
  };

  const removeSelected = () => {
    removeIds(
      selected,
      edges.filter((edge) => edge.selected).map((edge) => edge.data?.key ?? edge.id),
    );
  };

  const clipboard = useRef<Fragment | null>(null);

  const copySelected = () => {
    const fragment = copyFragment(graph, new Set(selectedBlocks));
    if (fragment.nodes.length > 0) {
      clipboard.current = fragment;
    }
  };

  const cornerOf = (fragment: Fragment): GraphPoint => ({
    x: Math.min(...fragment.nodes.map((node) => node.x ?? 0)),
    y: Math.min(...fragment.nodes.map((node) => node.y ?? 0)),
  });

  const duplicate = (ids: ReadonlyArray<string>) => {
    const fragment = copyFragment(graph, new Set(ids));
    if (fragment.nodes.length > 0) {
      const corner = cornerOf(fragment);
      place(fragment, { x: corner.x + COPY_SHIFT, y: corner.y + COPY_SHIFT }, null);
    }
  };

  const paste = () => {
    const fragment = clipboard.current;
    if (fragment) {
      const centre = viewCentre();
      place(fragment, { x: centre.x - BLOCK_WIDTH / 2, y: centre.y - BLOCK_HEIGHT / 2 }, null);
    }
  };

  const selectAll = () => {
    selectOnly(nodes.filter((node) => !node.hidden).map((node) => node.id));
  };

  const keys = useRef<(event: KeyboardEvent) => void>(() => undefined);
  keys.current = (event) => {
    const focused = document.activeElement;
    const nowhere = !(focused instanceof HTMLElement) || focused === document.body;
    // Keys belong to the canvas while the focus is on it or nowhere in particular;
    // a field keeps its own keys.
    if (
      document.querySelector(".modal-backdrop") ||
      (!nowhere &&
        (!root.current?.contains(focused) ||
          focused instanceof HTMLInputElement ||
          focused instanceof HTMLTextAreaElement ||
          focused instanceof HTMLSelectElement))
    ) {
      return;
    }
    const control = event.ctrlKey || event.metaKey;
    if (!control) {
      // The panel of a block is not the place to delete the block with a key.
      const inPanel = !nowhere && focused.closest(".canvas-inspector") !== null;
      if ((event.key === "Delete" || event.key === "Backspace") && !locked && !inPanel) {
        removeSelected();
        event.preventDefault();
      }
      return;
    }
    switch (event.code) {
      case "KeyZ":
        if (event.shiftKey) {
          redo();
        } else {
          undo();
        }
        break;
      case "KeyY":
        redo();
        break;
      case "KeyC":
        // Text selected on the page is copied as text.
        if ((window.getSelection()?.toString() ?? "") !== "") {
          return;
        }
        copySelected();
        break;
      case "KeyV":
        paste();
        break;
      case "KeyD":
        duplicate(selectedBlocks);
        break;
      case "KeyA":
        selectAll();
        break;
      default:
        return;
    }
    event.preventDefault();
  };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => keys.current(event);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Dragging: a frame takes the blocks and notes inside it along

  const dragging = useRef<{ origin: Map<string, GraphPoint>; followers: Map<string, string> } | null>(null);

  const dragStart = (moved: ReadonlyArray<CanvasNode>) => {
    const current = graphRef.current;
    const movedIds = new Set(moved.map((node) => node.id));
    const followers = new Map<string, string>();
    for (const item of moved) {
      const note = current.notes.find((candidate) => candidate.id === item.id);
      if (note?.kind !== "frame") {
        continue;
      }
      const members = frameMembers(current, note);
      for (const id of [...members.nodes, ...members.notes]) {
        if (!movedIds.has(id) && !followers.has(id)) {
          followers.set(id, note.id);
        }
      }
    }
    dragging.current = {
      origin: new Map(nodesRef.current.map((node) => [node.id, { x: node.position.x, y: node.position.y }])),
      followers,
    };
  };

  /** Where the followers of the moved frames are now. */
  const followerPlaces = (moved: ReadonlyArray<CanvasNode>): Map<string, GraphPoint> => {
    const places = new Map<string, GraphPoint>();
    const state = dragging.current;
    if (!state) {
      return places;
    }
    const now = new Map(moved.map((node) => [node.id, node.position]));
    for (const [id, frameId] of state.followers) {
      const start = state.origin.get(id);
      const frameStart = state.origin.get(frameId);
      const frameNow = now.get(frameId);
      if (start && frameStart && frameNow) {
        places.set(id, { x: start.x + frameNow.x - frameStart.x, y: start.y + frameNow.y - frameStart.y });
      }
    }
    return places;
  };

  const dragMove = (moved: ReadonlyArray<CanvasNode>) => {
    const places = followerPlaces(moved);
    if (places.size > 0) {
      setNodes((current) =>
        current.map((node) => {
          const place = places.get(node.id);
          return place ? { ...node, position: place } : node;
        }),
      );
    }
  };

  const dragStop = (moved: ReadonlyArray<CanvasNode>) => {
    const places = followerPlaces(moved);
    for (const node of moved) {
      places.set(node.id, node.position);
    }
    dragging.current = null;
    const rounded = new Map<string, GraphPoint>();
    for (const [id, place] of places) {
      rounded.set(id, { x: Math.round(place.x), y: Math.round(place.y) });
    }
    apply((current) => moveItems(current, rounded));
  };

  // Links

  const onConnect = (connection: Connection) => {
    const port = connection.sourceHandle;
    if (isPort(port)) {
      apply((current) => connect(current, connection.source, port, connection.target));
    }
  };

  const isValidConnection = (connection: Connection | CanvasEdge) => {
    const port = connection.sourceHandle;
    return isPort(port) && canConnect(graphRef.current, connection.source, port, connection.target);
  };

  const openPalette = (clientX: number, clientY: number, from: { node: string; port: Port } | null) => {
    const box = flowBox.current?.getBoundingClientRect();
    if (!box || locked) {
      return;
    }
    setPopup({
      kind: "palette",
      x: Math.max(0, Math.min(clientX - box.left, box.width - POPUP_WIDTH)),
      y: Math.max(0, Math.min(clientY - box.top, box.height - POPUP_HEIGHT)),
      at: flow.screenToFlowPosition({ x: clientX, y: clientY }),
      from,
    });
  };

  /** A link dropped on empty space asks which block to put there. */
  const onConnectEnd = (event: MouseEvent | TouchEvent, state: FinalConnectionState) => {
    if (state.isValid || !state.fromNode || !state.fromHandle || state.fromHandle.type !== "source") {
      return;
    }
    const port = state.fromHandle.id;
    const target = event.target;
    if (!isPort(port) || !(target instanceof Element) || !target.classList.contains("react-flow__pane")) {
      return;
    }
    const point = "changedTouches" in event ? event.changedTouches[0] : event;
    openPalette(point.clientX, point.clientY, { node: state.fromNode.id, port });
  };

  /** A double click on a link puts a bend point there. */
  const addBend = (event: ReactMouseEvent, edge: CanvasEdge) => {
    const key = edge.data?.key;
    const stored = graph.edges.find((candidate) => edgeKey(candidate) === key);
    const from = stored ? shape.byId.get(stored.from) : undefined;
    const to = stored ? shape.byId.get(stored.to) : undefined;
    if (locked || !edge.data?.editable || key === undefined || !stored || !from || !to) {
      return;
    }
    const at = flow.screenToFlowPosition({ x: event.clientX, y: event.clientY });
    const down = portsOf(from.type).indexOf(stored.port) === 0;
    const start = down
      ? { x: (from.x ?? 0) + BLOCK_WIDTH / 2, y: (from.y ?? 0) + BLOCK_HEIGHT }
      : { x: (from.x ?? 0) + BLOCK_WIDTH, y: (from.y ?? 0) + BLOCK_HEIGHT / 2 };
    const end = { x: (to.x ?? 0) + BLOCK_WIDTH / 2, y: to.y ?? 0 };
    const chain = [start, ...stored.points, end];
    let nearest = 0;
    let best = Number.POSITIVE_INFINITY;
    for (let index = 0; index + 1 < chain.length; index += 1) {
      const distance = distanceToSegment(at, chain[index], chain[index + 1]);
      if (distance < best) {
        best = distance;
        nearest = index;
      }
    }
    const points = [
      ...stored.points.slice(0, nearest),
      { x: Math.round(at.x), y: Math.round(at.y) },
      ...stored.points.slice(nearest),
    ];
    apply((current) => updateEdge(current, key, (before) => ({ ...before, points })));
  };

  // Whole-graph actions

  const arrangeAll = () => {
    apply(arrange);
    window.setTimeout(() => void flow.fitView({ padding: 0.15, maxZoom: 1, duration: 200 }), 50);
  };

  const extract = async () => {
    const problem = extractProblem(graph, new Set(selectedBlocks));
    if (problem !== null) {
      notify(td(`errors.${problem}`), "error");
      return;
    }
    const name = await askName({
      title: t("canvas.extractTitle"),
      initial: t("canvas.extractName"),
      okLabel: t("common.create"),
    });
    if (name === null) {
      return;
    }
    try {
      // The core takes the blocks out of the saved scenario.
      await flushAll();
      const result = await api.scenarioExtract(draft.id, selectedBlocks, name);
      touch("scenarios");
      reload();
      notify(t("canvas.extracted", { name: result.created.name }));
    } catch (error) {
      showError(error);
    }
  };

  const saveSnippet = async () => {
    const fragment = copyFragment(graph, new Set(selectedBlocks));
    if (fragment.nodes.length === 0) {
      return;
    }
    const name = await askName({
      title: t("canvas.snippetTitle"),
      initial: t("canvas.snippetName"),
      okLabel: t("common.save"),
    });
    if (name === null) {
      return;
    }
    try {
      const saved = await api.snippetSave(draft.id, name, fragment.nodes, fragment.edges);
      notify(t("canvas.snippetSaved", { name: saved.name }));
      loadSnippets();
    } catch (error) {
      showError(error);
    }
  };

  const deleteSnippet = async (snippet: Snippet) => {
    const ok = await confirm({
      title: t("canvas.palette.snippetDeleteTitle", { name: snippet.name }),
      okLabel: t("common.delete"),
      danger: true,
    });
    if (!ok) {
      return;
    }
    try {
      await api.snippetDelete(snippet.id);
      loadSnippets();
    } catch (error) {
      showError(error);
    }
  };

  // The panel of the selected block or note

  let target: InspectorTarget | null = null;
  if (selected.length === 1 && selected[0] !== closedFor) {
    const node = shape.byId.get(selected[0]);
    const note = graph.notes.find((candidate) => candidate.id === selected[0]);
    if (node) {
      target = { kind: "node", node, number: shape.numbers.get(node.id) ?? 0, removable: removable(graph, node.id) };
    } else if (note) {
      target = { kind: "note", note };
    }
  }
  useEffect(() => {
    // Another selection opens the panel again.
    setClosedFor((current) => (current !== null && selectedKey !== current ? null : current));
  }, [selectedKey]);

  const menuItems = (id: string): MenuItem[] => {
    const node = shape.byId.get(id);
    if (!node) {
      return [];
    }
    const items: MenuItem[] = [];
    if (node.type !== "intercept") {
      items.push({ label: t("canvas.inspector.runFrom"), disabled: locked, run: () => onRunFrom(id) });
    }
    items.push({
      label: breakpoints.has(id) ? t("canvas.block.breakpointRemove") : t("canvas.block.breakpointSet"),
      run: () => onToggleBreakpoint(id),
    });
    if (node.type !== "start") {
      items.push({ label: t("common.duplicate"), disabled: locked, run: () => duplicate([id]) });
    }
    items.push({
      label: t("common.delete"),
      danger: true,
      disabled: locked || !removable(graph, id),
      run: () => removeIds([id], []),
    });
    return items;
  };

  const hasStart = graph.nodes.some((node) => node.type === "start");
  const empty = graph.nodes.length <= 1 && graph.edges.length === 0 && graph.notes.length === 0;

  return (
    <CanvasContext.Provider value={canvasApi}>
      <div ref={root} className="canvas">
        <div className="canvas-toolbar" role="toolbar">
          <Button
            small
            className={cx(paletteOpen && "canvas-tool-on")}
            title={t("canvas.toolbar.blocksTitle")}
            aria-pressed={paletteOpen}
            onClick={() => setPaletteOpen((open) => !open)}
          >
            {t("canvas.toolbar.blocks")}
          </Button>
          <Button small disabled={locked} onClick={addNote}>
            {t("canvas.toolbar.note")}
          </Button>
          <Button small disabled={locked} title={t("canvas.toolbar.frameTitle")} onClick={addFrame}>
            {t("canvas.toolbar.frame")}
          </Button>
          <span className="canvas-toolbar-gap" />
          <Button small disabled={locked || !canUndo} title="Ctrl+Z" onClick={undo}>
            {t("canvas.toolbar.undo")}
          </Button>
          <Button small disabled={locked || !canRedo} title="Ctrl+Y" onClick={redo}>
            {t("canvas.toolbar.redo")}
          </Button>
          <span className="canvas-toolbar-gap" />
          <Button small disabled={locked} title={t("canvas.toolbar.arrangeTitle")} onClick={arrangeAll}>
            {t("canvas.toolbar.arrange")}
          </Button>
          <Button small onClick={() => void flow.fitView({ padding: 0.15, maxZoom: 1, duration: 200 })}>
            {t("canvas.toolbar.fit")}
          </Button>
          <Checkbox
            checked={snapOn}
            label={t("canvas.toolbar.grid")}
            title={t("canvas.toolbar.gridTitle")}
            onChange={setSnapOn}
          />
          <Checkbox
            checked={follow}
            label={t("canvas.toolbar.follow")}
            title={t("canvas.toolbar.followTitle")}
            onChange={setFollow}
          />
          {hasTrace ? (
            <Button small onClick={clearTrace}>
              {t("canvas.toolbar.clearTrace")}
            </Button>
          ) : null}
          {selectedBlocks.length > 0 ? (
            <>
              <span className="canvas-toolbar-gap" />
              <span className="canvas-toolbar-note">{t("canvas.toolbar.selected", { count: selectedBlocks.length })}</span>
              <Button small disabled={locked} onClick={() => void extract()}>
                {t("canvas.toolbar.extract")}
              </Button>
              <Button small onClick={() => void saveSnippet()}>
                {t("canvas.toolbar.saveSnippet")}
              </Button>
            </>
          ) : null}
          {onToggleList ? (
            <Button small className="canvas-toolbar-end" onClick={onToggleList}>
              {listHidden ? t("canvas.toolbar.showList") : t("canvas.toolbar.hideList")}
            </Button>
          ) : null}
        </div>
        <div className="canvas-main">
          {paletteOpen ? (
            <Palette
              hasStart={hasStart}
              snippets={snippets}
              disabled={locked}
              onPick={(item) => void addItem(item, null, null)}
              onDrop={dropItem}
              onDeleteSnippet={(snippet) => void deleteSnippet(snippet)}
            />
          ) : null}
          <div
            ref={flowBox}
            className="canvas-flow"
            onDoubleClick={(event) => {
              if (event.target instanceof Element && event.target.classList.contains("react-flow__pane")) {
                openPalette(event.clientX, event.clientY, null);
              }
            }}
          >
            <div className="canvas-flow-fill">
              <ReactFlow<CanvasNode, CanvasEdge>
                nodes={nodes}
                edges={edges}
                nodeTypes={NODE_TYPES}
                edgeTypes={EDGE_TYPES}
                onNodesChange={(changes) => setNodes((current) => changes.applyTo(current))}
                onEdgesChange={(changes) => setEdges((current) => changes.applyTo(current))}
                onConnect={onConnect}
                onConnectEnd={onConnectEnd}
                isValidConnection={isValidConnection}
                onNodeDragStart={(_event, _node, moved) => dragStart(moved)}
                onNodeDrag={(_event, _node, moved) => dragMove(moved)}
                onNodeDragStop={(_event, _node, moved) => dragStop(moved)}
                onSelectionDragStart={(_event, moved) => dragStart(moved)}
                onSelectionDrag={(_event, moved) => dragMove(moved)}
                onSelectionDragStop={(_event, moved) => dragStop(moved)}
                onEdgeDoubleClick={addBend}
                onNodeContextMenu={(event, node) => {
                  event.preventDefault();
                  const box = flowBox.current?.getBoundingClientRect();
                  if (box && node.type === "block") {
                    setPopup({ kind: "menu", x: event.clientX - box.left, y: event.clientY - box.top, nodeId: node.id });
                  }
                }}
                onPaneContextMenu={(event) => {
                  event.preventDefault();
                  openPalette(event.clientX, event.clientY, null);
                }}
                nodesDraggable={!locked}
                nodesConnectable={!locked}
                deleteKeyCode={null}
                snapToGrid={snapOn}
                snapGrid={[GRID, GRID]}
                minZoom={0.2}
                maxZoom={2}
                zoomOnDoubleClick={false}
                elevateNodesOnSelect={false}
                connectionRadius={24}
                proOptions={{ hideAttribution: true }}
              >
                <Background gap={GRID} size={1} />
                <Controls showInteractive={false} />
                <MiniMap
                  pannable
                  zoomable
                  nodeColor={(node) => {
                    const block = shape.byId.get(node.id);
                    return block ? ROLE_COLORS[roleOf(block.type)] : "var(--border)";
                  }}
                />
              </ReactFlow>
            </div>
            {empty ? <div className="canvas-empty">{t("canvas.emptyHint")}</div> : null}
            {popup?.kind === "palette" ? (
              <PalettePopup
                x={popup.x}
                y={popup.y}
                hasStart={hasStart}
                snippets={snippets}
                onPick={(item) => {
                  setPopup(null);
                  void addItem(item, popup.at, popup.from);
                }}
                onClose={() => setPopup(null)}
              />
            ) : null}
            {popup?.kind === "menu" ? (
              <CanvasMenu x={popup.x} y={popup.y} items={menuItems(popup.nodeId)} onClose={() => setPopup(null)} />
            ) : null}
          </div>
          {target ? (
            <Inspector
              target={target}
              issues={target.kind === "node" ? (issues.get(target.node.id) ?? NO_ISSUES) : NO_ISSUES}
              scenarios={scenarios}
              locked={locked}
              store={store}
              onNode={(node: GraphNode) => apply((current) => updateNode(current, node.id, () => node), `node:${node.id}`)}
              onNote={(note: GraphNote) => apply((current) => updateNote(current, note.id, () => note), `note:${note.id}`)}
              onRunFrom={onRunFrom}
              onDuplicate={(id) => duplicate([id])}
              onDelete={(id) => removeIds([id], [])}
              onClose={() => setClosedFor(selected[0] ?? null)}
            />
          ) : null}
        </div>
      </div>
    </CanvasContext.Provider>
  );
}
