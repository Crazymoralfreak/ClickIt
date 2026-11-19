// Names, one-line descriptions and output labels of the blocks.

import type { TKey, Translator } from "../../i18n";
import type { GraphNode, NodeType, Port, Step } from "../../types/scenario";
import { conditionSummary, stepSummary } from "../editor/summary";
import type { Role } from "./graph";

const TYPE_KEYS: Record<NodeType, TKey> = {
  start: "canvas.types.start",
  click: "steps.types.click",
  move: "steps.types.move",
  drag: "steps.types.drag",
  scroll: "steps.types.scroll",
  key: "steps.types.key",
  text: "steps.types.text",
  pause: "steps.types.pause",
  stop: "steps.types.stop",
  wait: "steps.types.wait",
  branch: "canvas.types.branch",
  counter: "canvas.types.counter",
  call: "canvas.types.call",
  intercept: "canvas.types.intercept",
};

export const ROLE_KEYS: Record<Role, TKey> = {
  entry: "canvas.roles.entry",
  action: "canvas.roles.action",
  check: "canvas.roles.check",
  control: "canvas.roles.control",
};

const PORT_KEYS: Record<Port, TKey> = {
  next: "canvas.ports.next",
  timeout: "canvas.ports.timeout",
  yes: "canvas.ports.yes",
  no: "canvas.ports.no",
  body: "canvas.ports.body",
  done: "canvas.ports.done",
};

/** Colour of an output and of the link that leaves it. */
export type PortTone = "good" | "bad" | "plain";

export function typeName(type: NodeType, { t }: Translator): string {
  return t(TYPE_KEYS[type]);
}

/** The title of a block: the user's own name, or the name of its type. */
export function blockTitle(node: GraphNode, translator: Translator): string {
  const own = node.title?.trim();
  return own ? own : typeName(node.type, translator);
}

/** The "next" output of a wait is the one a met condition takes, so it reads "found". */
export function portLabel(type: NodeType, port: Port, { t }: Translator): string {
  return type === "wait" && port === "next" ? t("canvas.ports.found") : t(PORT_KEYS[port]);
}

export function portTone(type: NodeType, port: Port): PortTone {
  switch (port) {
    case "yes":
    case "body":
      return "good";
    case "no":
    case "timeout":
      return "bad";
    case "next":
      return type === "wait" ? "good" : "plain";
    case "done":
      return "plain";
    default: {
      const unreachable: never = port;
      throw new Error(`Unknown output: ${String(unreachable)}`);
    }
  }
}

/** One line under the title: what the block does. `scenarioNames` name the scenarios a block may run. */
export function blockSummary(node: GraphNode, translator: Translator, scenarioNames: ReadonlyMap<string, string>): string {
  const { t } = translator;
  switch (node.type) {
    case "start":
      return t("canvas.summary.start");
    case "click":
    case "move":
    case "drag":
    case "scroll":
    case "key":
    case "text":
    case "pause":
    case "stop": {
      const step: Step = node;
      return stepSummary(step, translator);
    }
    case "wait":
      return stepSummary({ ...node, type: "wait", onTimeout: "stop" }, translator);
    case "branch":
      return t("canvas.summary.branch", { condition: conditionSummary(node.condition, translator) });
    case "counter":
      return t("canvas.summary.counter", { times: node.times });
    case "call": {
      if (node.scenarioId === "") {
        return t("canvas.summary.callNone");
      }
      const name = scenarioNames.get(node.scenarioId);
      return name === undefined ? t("canvas.summary.callGone") : t("canvas.summary.call", { name });
    }
    case "intercept":
      return t("canvas.summary.intercept", { condition: conditionSummary(node.condition, translator) });
    default: {
      const unreachable: never = node;
      throw new Error(`Unknown block: ${String(unreachable)}`);
    }
  }
}

/** File name of the template image a block looks for, if it has one. */
export function blockTemplate(node: GraphNode): string | null {
  if (node.type !== "wait" && node.type !== "branch" && node.type !== "intercept") {
    return null;
  }
  const condition = node.condition;
  if (condition.kind !== "imageFound" && condition.kind !== "imageGone") {
    return null;
  }
  return condition.template === "" ? null : condition.template;
}
