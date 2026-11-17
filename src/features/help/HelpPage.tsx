import { useApp } from "../../app/context";
import { useT, type TKey, type TParams } from "../../i18n";
import { PageFrame, Section } from "../../ui";
import "./help.css";

type HelpPart =
  | { kind: "text"; text: TKey }
  | { kind: "list"; title?: TKey; items: ReadonlyArray<TKey> };

interface HelpTopic {
  title: TKey;
  parts: ReadonlyArray<HelpPart>;
}

/** The whole reference; the texts live in the dictionaries under `help`. */
const TOPICS: ReadonlyArray<HelpTopic> = [
  {
    title: "help.modes.title",
    parts: [
      { kind: "text", text: "help.modes.intro" },
      {
        kind: "list",
        title: "help.modes.screenTitle",
        items: ["help.modes.screenCoords", "help.modes.screenNoPause", "help.modes.screenPoint"],
      },
      {
        kind: "list",
        title: "help.modes.windowTitle",
        items: [
          "help.modes.windowTarget",
          "help.modes.windowFocus",
          "help.modes.windowCoords",
          "help.modes.windowPause",
          "help.modes.windowOutside",
        ],
      },
    ],
  },
  {
    title: "help.steps.title",
    parts: [
      { kind: "text", text: "help.steps.list" },
      { kind: "text", text: "help.steps.pause" },
    ],
  },
  {
    title: "help.cycles.title",
    parts: [
      {
        kind: "list",
        items: [
          "help.cycles.stepRepeat",
          "help.cycles.loop",
          "help.cycles.exitLoop",
          "help.cycles.nesting",
          "help.cycles.scenarioRepeat",
          "help.cycles.queues",
        ],
      },
    ],
  },
  {
    title: "help.conditions.title",
    parts: [
      { kind: "text", text: "help.conditions.intro" },
      {
        kind: "list",
        title: "help.conditions.kindsTitle",
        items: [
          "help.conditions.pixel",
          "help.conditions.image",
          "help.conditions.window",
          "help.conditions.process",
        ],
      },
      { kind: "text", text: "help.conditions.timing" },
      { kind: "text", text: "help.conditions.timeoutZero" },
      { kind: "text", text: "help.conditions.template" },
      { kind: "text", text: "help.conditions.area" },
    ],
  },
  {
    title: "help.ifElse.title",
    parts: [{ kind: "text", text: "help.ifElse.text" }],
  },
  {
    title: "help.recording.title",
    parts: [
      {
        kind: "list",
        items: [
          "help.recording.button",
          "help.recording.badge",
          "help.recording.keys",
          "help.recording.limits",
          "help.recording.shortPauses",
        ],
      },
    ],
  },
  {
    title: "help.startStop.title",
    parts: [
      { kind: "text", text: "help.startStop.hotkeys" },
      { kind: "text", text: "help.startStop.countdown" },
      {
        kind: "list",
        title: "help.startStop.stopTitle",
        items: [
          "help.startStop.stopButton",
          "help.startStop.stopHotkey",
          "help.startStop.stopTray",
          "help.startStop.stopCorner",
        ],
      },
      { kind: "text", text: "help.startStop.release" },
    ],
  },
  {
    title: "help.tray.title",
    parts: [
      { kind: "text", text: "help.tray.tray" },
      { kind: "text", text: "help.tray.schedule" },
    ],
  },
  {
    title: "help.blocked.title",
    parts: [{ kind: "text", text: "help.blocked.text" }],
  },
  {
    title: "help.limits.title",
    parts: [
      {
        kind: "list",
        items: [
          "help.limits.input",
          "help.limits.noHiding",
          "help.limits.presence",
          "help.limits.admin",
          "help.limits.layout",
        ],
      },
    ],
  },
];

function HelpPartView({ part, params }: { part: HelpPart; params: TParams }) {
  const { t } = useT();
  switch (part.kind) {
    case "text":
      return <p className="help-text">{t(part.text, params)}</p>;
    case "list":
      return (
        <>
          {part.title ? <h3 className="help-subtitle">{t(part.title, params)}</h3> : null}
          <ul className="help-list">
            {part.items.map((item) => (
              <li key={item}>{t(item, params)}</li>
            ))}
          </ul>
        </>
      );
    default: {
      const unreachable: never = part;
      throw new Error(`Unknown help part: ${JSON.stringify(unreachable)}`);
    }
  }
}

export function HelpPage() {
  const { t } = useT();
  const { settings } = useApp();
  // Placeholders shared by all texts: the hotkeys as they are set right now.
  const params: TParams = { startStop: settings.hotkeyStartStop, capture: settings.hotkeyCapture };

  return (
    <PageFrame title={t("nav.help")}>
      {TOPICS.map((topic) => (
        <Section key={topic.title} title={t(topic.title)}>
          {topic.parts.map((part, index) => (
            <HelpPartView key={index} part={part} params={params} />
          ))}
        </Section>
      ))}
    </PageFrame>
  );
}
