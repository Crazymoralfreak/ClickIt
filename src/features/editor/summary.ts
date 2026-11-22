// One-line descriptions of steps and conditions for the step list.

import type { TKey, Translator } from "../../i18n";
import type {
  ClickStep,
  CommandSpec,
  Condition,
  ConditionKind,
  KeyStep,
  Language,
  LoopStep,
  Modifiers,
  MouseButton,
  Pause,
  Position,
  Step,
  StepType,
} from "../../types/scenario";

export const STEP_TYPE_KEYS: Record<StepType, TKey> = {
  click: "steps.types.click",
  move: "steps.types.move",
  drag: "steps.types.drag",
  scroll: "steps.types.scroll",
  key: "steps.types.key",
  text: "steps.types.text",
  pause: "steps.types.pause",
  wait: "steps.types.wait",
  if: "steps.types.if",
  loop: "steps.types.loop",
  breakLoop: "steps.types.breakLoop",
  stop: "steps.types.stop",
  command: "steps.types.command",
};

export const CONDITION_KIND_KEYS: Record<ConditionKind, TKey> = {
  pixel: "conditions.kinds.pixel",
  imageFound: "conditions.kinds.imageFound",
  imageGone: "conditions.kinds.imageGone",
  window: "conditions.kinds.window",
  process: "conditions.kinds.process",
};

export const BUTTON_KEYS: Record<MouseButton, TKey> = {
  left: "steps.buttons.left",
  right: "steps.buttons.right",
  middle: "steps.buttons.middle",
};

const BUTTON_WITH_KEYS: Record<MouseButton, TKey> = {
  left: "steps.buttonWith.left",
  right: "steps.buttonWith.right",
  middle: "steps.buttonWith.middle",
};

const BUTTON_ACC_KEYS: Record<MouseButton, TKey> = {
  left: "steps.buttonAcc.left",
  right: "steps.buttonAcc.right",
  middle: "steps.buttonAcc.middle",
};

/** Key names as they are written in hotkeys; not translated. */
export const MODIFIERS: ReadonlyArray<{ id: keyof Modifiers; name: string }> = [
  { id: "ctrl", name: "Ctrl" },
  { id: "alt", name: "Alt" },
  { id: "shift", name: "Shift" },
  { id: "win", name: "Win" },
];

const TEXT_PREVIEW_LENGTH = 40;

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function pointText(position: Position, { t }: Translator): string {
  return position.kind === "cursor" ? t("steps.summary.pointNotSet") : `${position.x}, ${position.y}`;
}

function whereText(position: Position, { t }: Translator): string {
  return position.kind === "cursor"
    ? t("steps.summary.atCursor")
    : t("steps.summary.atPoint", { x: position.x, y: position.y });
}

export function pauseText(pause: Pause, { t }: Translator): string {
  return pause.fromMs === pause.toMs
    ? t("steps.summary.pauseExact", { ms: pause.fromMs })
    : t("steps.summary.pauseRange", { from: pause.fromMs, to: pause.toMs });
}

/** `Ctrl+Shift+KeyC`; an empty key is shown as "(not chosen)". */
export function keyCombo(modifiers: Modifiers, key: string, { t }: Translator): string {
  const parts = MODIFIERS.filter((modifier) => modifiers[modifier.id]).map((modifier) => modifier.name);
  parts.push(key === "" ? t("steps.summary.keyNotSet") : key);
  return parts.join("+");
}

type PluralForm = "one" | "few" | "many";

function pluralForm(count: number, language: Language): PluralForm {
  switch (language) {
    case "ru": {
      const tens = Math.abs(count) % 100;
      const units = tens % 10;
      if (tens >= 11 && tens <= 14) {
        return "many";
      }
      if (units === 1) {
        return "one";
      }
      return units >= 2 && units <= 4 ? "few" : "many";
    }
    case "en":
      return count === 1 ? "one" : "many";
    default: {
      const unreachable: never = language;
      throw new Error(`Unknown language: ${String(unreachable)}`);
    }
  }
}

const LOOP_COUNT_KEYS: Record<PluralForm, TKey> = {
  one: "steps.summary.loopOne",
  few: "steps.summary.loopFew",
  many: "steps.summary.loopMany",
};

export function conditionSummary(condition: Condition, translator: Translator): string {
  const { t } = translator;
  switch (condition.kind) {
    case "pixel":
      return t("conditions.summary.pixel", { color: condition.color, point: pointText(condition.point, translator) });
    case "imageFound":
      return t("conditions.summary.imageFound");
    case "imageGone":
      return t("conditions.summary.imageGone");
    case "window":
      return t("conditions.summary.window", { title: condition.title });
    case "process":
      return t("conditions.summary.process", { name: condition.name });
    case "all":
    case "any": {
      const first = condition.items[0];
      const second = condition.items[1];
      const blank = "…";
      return t(condition.kind === "all" ? "conditions.summary.all" : "conditions.summary.any", {
        first: first ? conditionSummary(first, translator) : blank,
        second: second ? conditionSummary(second, translator) : blank,
      });
    }
    default: {
      const unreachable: never = condition;
      throw new Error(`Unknown condition: ${String(unreachable)}`);
    }
  }
}

function clickSummary(step: ClickStep, translator: Translator): string {
  const { t } = translator;
  const params = {
    button: t(BUTTON_WITH_KEYS[step.button]),
    buttonAcc: t(BUTTON_ACC_KEYS[step.button]),
    where: whereText(step.position, translator),
  };
  switch (step.action) {
    case "click":
      return t("steps.summary.clickClick", params);
    case "double":
      return t("steps.summary.clickDouble", params);
    case "down":
      return t("steps.summary.clickDown", params);
    case "up":
      return t("steps.summary.clickUp", params);
    default: {
      const unreachable: never = step.action;
      throw new Error(`Unknown click action: ${String(unreachable)}`);
    }
  }
}

function keySummary(step: KeyStep, translator: Translator): string {
  const { t } = translator;
  const combo = keyCombo(step.modifiers, step.key, translator);
  switch (step.action) {
    case "press":
      return t("steps.summary.keyPress", { combo });
    case "down":
      return t("steps.summary.keyDown", { combo });
    case "up":
      return t("steps.summary.keyUp", { combo });
    default: {
      const unreachable: never = step.action;
      throw new Error(`Unknown key action: ${String(unreachable)}`);
    }
  }
}

function loopSummary(step: LoopStep, translator: Translator): string {
  const { t, language } = translator;
  const mode = step.mode;
  switch (mode.kind) {
    case "count":
      return t(LOOP_COUNT_KEYS[pluralForm(mode.times, language)], { times: mode.times });
    case "while":
      return t("steps.summary.loopWhile", { condition: conditionSummary(mode.condition, translator) });
    case "forever":
      return t("steps.summary.loopForever");
    default: {
      const unreachable: never = mode;
      throw new Error(`Unknown loop mode: ${String(unreachable)}`);
    }
  }
}

function textPreview(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > TEXT_PREVIEW_LENGTH ? `${flat.slice(0, TEXT_PREVIEW_LENGTH)}…` : flat;
}

/** "Start: backup.bat nightly", "cmd script: echo done"; the folder of the program is left out. */
export function commandSummary(command: CommandSpec, { t }: Translator): string {
  const run = command.run;
  switch (run.kind) {
    case "program": {
      const path = run.path.trim().replace(/^"|"$/g, "");
      if (path === "") {
        return t("steps.summary.commandEmpty");
      }
      const file = path.split(/[\\/]/).pop() ?? path;
      return t("steps.summary.commandProgram", { command: textPreview(`${file} ${run.args}`) });
    }
    case "script":
      if (run.text.trim() === "") {
        return t("steps.summary.commandEmpty");
      }
      return t(run.shell === "cmd" ? "steps.summary.commandCmd" : "steps.summary.commandPowerShell", {
        text: textPreview(run.text),
      });
    default: {
      const unreachable: never = run;
      throw new Error(`Unknown command: ${String(unreachable)}`);
    }
  }
}

function describe(step: Step, translator: Translator): string {
  const { t } = translator;
  switch (step.type) {
    case "click":
      return clickSummary(step, translator);
    case "move":
      return t("steps.summary.move", { point: pointText(step.position, translator) });
    case "drag":
      return t("steps.summary.drag", {
        button: t(BUTTON_WITH_KEYS[step.button]),
        from:
          step.from.kind === "cursor"
            ? t("steps.summary.fromCursor")
            : t("steps.summary.fromPoint", { x: step.from.x, y: step.from.y }),
        to: pointText(step.to, translator),
      });
    case "scroll": {
      const params = { where: whereText(step.position, translator), notches: step.notches };
      return step.direction === "up" ? t("steps.summary.scrollUp", params) : t("steps.summary.scrollDown", params);
    }
    case "key":
      return keySummary(step, translator);
    case "text":
      return step.text === ""
        ? t("steps.summary.textEmpty")
        : t("steps.summary.text", { text: textPreview(step.text) });
    case "pause":
      return t("steps.summary.pause", { pause: pauseText(step.duration, translator) });
    case "wait": {
      const condition = conditionSummary(step.condition, translator);
      return step.onFound.kind === "click"
        ? t("steps.summary.waitClick", { condition })
        : t("steps.summary.wait", { condition });
    }
    case "if":
      return t("steps.summary.if", { condition: conditionSummary(step.condition, translator) });
    case "loop":
      return loopSummary(step, translator);
    case "breakLoop":
      return t("steps.types.breakLoop");
    case "stop":
      return t("steps.types.stop");
    case "command":
      return commandSummary(step.command, translator);
    default: {
      const unreachable: never = step;
      throw new Error(`Unknown step: ${String(unreachable)}`);
    }
  }
}

/** "Left click at 640, 360", "Key Ctrl+KeyC", "Pause 100–250 ms", "Repeat 20 times". */
export function stepSummary(step: Step, translator: Translator): string {
  return capitalize(describe(step, translator));
}
