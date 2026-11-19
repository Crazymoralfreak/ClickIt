// Ready-made pieces of a graph that come with ClickIt.

import type { TKey } from "../../i18n";
import type { NodeType, ScenarioMode } from "../../types/scenario";
import { createNode, type Fragment } from "./graph";

export type BuiltinSnippet = "waitClick" | "repeat" | "retry" | "until";

export const BUILTIN_SNIPPETS: ReadonlyArray<{ id: BuiltinSnippet; name: TKey }> = [
  { id: "waitClick", name: "canvas.palette.builtinWaitClick" },
  { id: "repeat", name: "canvas.palette.builtinRepeat" },
  { id: "until", name: "canvas.palette.builtinUntil" },
  { id: "retry", name: "canvas.palette.builtinRetry" },
];

/** What the list of blocks can put on the canvas. */
export type PaletteItem =
  | { kind: "block"; type: NodeType }
  | { kind: "builtin"; id: BuiltinSnippet }
  | { kind: "snippet"; id: string };

const COLUMN = 280;
const ROW = 140;

export function builtinFragment(id: BuiltinSnippet, mode: ScenarioMode): Fragment {
  switch (id) {
    case "waitClick": {
      const wait = createNode("wait", mode, { x: 0, y: 0 });
      if (wait.type !== "wait") {
        throw new Error("Not a wait block");
      }
      return {
        nodes: [{ ...wait, onFound: { kind: "click", button: "left", offsetX: 0, offsetY: 0 } }],
        edges: [],
      };
    }
    case "repeat": {
      const counter = createNode("counter", mode, { x: 0, y: 0 });
      const click = createNode("click", mode, { x: 0, y: ROW });
      if (counter.type !== "counter") {
        throw new Error("Not a counter block");
      }
      return {
        nodes: [{ ...counter, times: 5 }, click],
        edges: [
          { from: counter.id, port: "body", to: click.id, points: [] },
          { from: click.id, port: "next", to: counter.id, points: [] },
        ],
      };
    }
    case "until": {
      const gate = createNode("while", mode, { x: 0, y: 0 });
      const pause = createNode("pause", mode, { x: 0, y: ROW });
      if (gate.type !== "while" || pause.type !== "pause") {
        throw new Error("Not a while and a pause block");
      }
      return {
        nodes: [{ ...gate, until: true, pauseBetween: { fromMs: 1000, toMs: 1000 } }, pause],
        edges: [
          { from: gate.id, port: "body", to: pause.id, points: [] },
          { from: pause.id, port: "next", to: gate.id, points: [] },
        ],
      };
    }
    case "retry": {
      const branch = createNode("branch", mode, { x: 0, y: 0 });
      const key = createNode("key", mode, { x: COLUMN, y: ROW });
      const pause = createNode("pause", mode, { x: COLUMN, y: ROW * 2 });
      if (key.type !== "key" || pause.type !== "pause") {
        throw new Error("Not a key and a pause block");
      }
      return {
        nodes: [branch, { ...key, key: "F5" }, { ...pause, duration: { fromMs: 2000, toMs: 2000 } }],
        edges: [
          { from: branch.id, port: "no", to: key.id, points: [] },
          { from: key.id, port: "next", to: pause.id, points: [] },
          { from: pause.id, port: "next", to: branch.id, points: [] },
        ],
      };
    }
    default: {
      const unreachable: never = id;
      throw new Error(`Unknown snippet: ${String(unreachable)}`);
    }
  }
}
