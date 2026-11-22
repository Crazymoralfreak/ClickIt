import { Handle, Position, type NodeProps } from "@xyflow/react";
import { Fragment, memo, useEffect, useState } from "react";
import { api } from "../../api";
import { useT } from "../../i18n";
import { cx } from "../../ui";
import { blockTemplate, portLabel, portTone } from "./blockText";
import { useCanvas, type BlockFlowNode } from "./canvasContext";
import { failsUnlinked, isEntry, portsOf, roleOf } from "./graph";
import { BlockIcon } from "./icons";
import { useNodeLive, type NodeLive } from "./useRunTrace";

const THUMB_CACHE_LIMIT = 200;
/** Data URLs of template images, by scenario and file. A template file is never rewritten. */
const thumbs = new Map<string, Promise<string>>();

function loadThumb(scenarioId: string, file: string): Promise<string> {
  const key = `${scenarioId}/${file}`;
  let pending = thumbs.get(key);
  if (!pending) {
    if (thumbs.size >= THUMB_CACHE_LIMIT) {
      thumbs.clear();
    }
    pending = api.templatePreview(scenarioId, file);
    thumbs.set(key, pending);
    // A failed load is asked for again the next time the block is shown.
    pending.catch(() => thumbs.delete(key));
  }
  return pending;
}

function useThumb(scenarioId: string, file: string | null): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    setUrl(null);
    if (file === null) {
      return;
    }
    let cancelled = false;
    loadThumb(scenarioId, file)
      .then((next) => {
        if (!cancelled) {
          setUrl(next);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [scenarioId, file]);
  return url;
}

const TICK_MS = 200;

/** Seconds left of a timeout or a pause that began at `since`. */
function Countdown({ since, ms }: { since: number; ms: number }) {
  const { t, language } = useT();
  const [now, setNow] = useState(() => performance.now());
  useEffect(() => {
    setNow(performance.now());
    const timer = window.setInterval(() => setNow(performance.now()), TICK_MS);
    return () => window.clearInterval(timer);
  }, [since, ms]);
  const left = Math.max(0, since + ms - now) / 1000;
  const text = left.toLocaleString(language, { maximumFractionDigits: left < 10 ? 1 : 0, minimumFractionDigits: left < 10 ? 1 : 0 });
  return (
    <span className="block-live-time">
      {text} {t("common.sec")}
    </span>
  );
}

/** What the block is doing right now, or how the last run left it. */
function LiveLabel({ live }: { live: NodeLive }) {
  const { t } = useT();
  if (live.active) {
    const parts: string[] = [];
    switch (live.phase) {
      case "break":
        parts.push(t("canvas.block.phaseBreak"));
        break;
      case "check":
        parts.push(t("canvas.block.phaseCheck"));
        break;
      case "pause":
        parts.push(t("canvas.block.phasePause"));
        break;
      case "action":
      case null:
        parts.push(t("canvas.block.phaseAction"));
        break;
      default: {
        const unreachable: never = live.phase;
        throw new Error(`Unknown phase: ${String(unreachable)}`);
      }
    }
    if (live.inner !== null) {
      parts.push(t("canvas.block.inner", { step: live.inner }));
    }
    if (live.repeat !== null && live.repeatTotal !== null) {
      parts.push(t("canvas.block.counter", { pass: live.repeat, total: live.repeatTotal }));
    }
    if (live.phase === "check" && live.checks > 0) {
      parts.push(t("canvas.block.checks", { count: live.checks }));
    }
    // An action is timed only in a command block: the time its command has to end.
    const timed = live.phaseMs !== null && live.phase !== "break";
    return (
      <span className="block-live block-live-active" role="status">
        {parts.join(" · ")}
        {timed && live.phaseMs !== null ? (
          <>
            {" · "}
            <Countdown since={live.phaseAt} ms={live.phaseMs} />
          </>
        ) : null}
      </span>
    );
  }
  if (live.ended !== null) {
    const text =
      live.ended === "completed"
        ? t("canvas.block.endedCompleted")
        : live.ended === "stopped"
          ? t("canvas.block.endedStopped")
          : t("canvas.block.endedFailed");
    return <span className={cx("block-live", `block-live-${live.ended}`)}>{text}</span>;
  }
  return null;
}

/** One block of the graph: title, what it does, its outputs and what a run is doing with it. */
export const BlockNode = memo(function BlockNode({ id, data, selected }: NodeProps<BlockFlowNode>) {
  const translator = useT();
  const { t } = translator;
  const canvas = useCanvas();
  const live = useNodeLive(canvas.store, id);
  const { node } = data;
  const ports = portsOf(node.type);
  const wired = data.wired === "" ? [] : data.wired.split(",");
  const thumb = useThumb(canvas.scenarioId, blockTemplate(node));
  const repeat = "repeat" in node && node.repeat ? node.repeat.times : null;
  const timed = live.active && live.phaseMs !== null && live.phase !== "break";
  const counting = live.counterPass !== null && live.counterTotal !== null;

  return (
    <div
      className={cx(
        "block",
        `block-${roleOf(node.type)}`,
        selected && "block-selected",
        !node.enabled && "block-off",
        data.dim && "block-dim",
        data.unreachable && "block-unreachable",
        data.changed && "block-changed",
        live.active && "block-active",
        live.active && live.phase === "break" && "block-paused",
        live.ended !== null && `block-ended-${live.ended}`,
      )}
    >
      <span className="block-shape" aria-hidden="true" />
      {isEntry(node.type) ? null : (
        <Handle type="target" id="in" position={Position.Top} className="block-handle block-handle-in" />
      )}
      <button
        type="button"
        className={cx("block-breakpoint", "nodrag", "nopan", data.breakpoint && "block-breakpoint-set")}
        title={data.breakpoint ? t("canvas.block.breakpointRemove") : t("canvas.block.breakpointSet")}
        aria-label={data.breakpoint ? t("canvas.block.breakpointRemove") : t("canvas.block.breakpointSet")}
        aria-pressed={data.breakpoint}
        onClick={(event) => {
          event.stopPropagation();
          canvas.toggleBreakpoint(id);
        }}
      />
      <div className="block-head">
        <BlockIcon type={node.type} />
        <span className="block-title" title={data.title}>
          {data.title}
        </span>
        {repeat !== null ? (
          <span className="block-tag" title={t("canvas.block.repeatBadge", { times: repeat })}>
            ×{repeat}
          </span>
        ) : null}
        {counting ? (
          <span className="block-tag block-tag-live">
            {t("canvas.block.counter", { pass: live.counterPass ?? 0, total: live.counterTotal ?? 0 })}
          </span>
        ) : null}
        {!live.active && live.passes > 0 ? (
          <span className="block-tag block-tag-trace" title={t("canvas.block.passes", { count: live.passes })}>
            {live.passes}
          </span>
        ) : null}
        {data.issue !== null ? (
          <span className={cx("block-issue", `block-issue-${data.issue}`)} title={data.issueText}>
            !
          </span>
        ) : null}
        <span className="block-number">{data.number}</span>
      </div>
      <div className="block-body">
        {thumb !== null ? <img className="block-thumb" src={thumb} alt="" draggable={false} /> : null}
        <span className="block-summary" title={data.summary}>
          {node.enabled ? data.summary : `${t("canvas.block.off")} · ${data.summary}`}
        </span>
      </div>
      {timed && live.phaseMs !== null ? (
        <span
          key={`p${live.stamp}`}
          className="block-progress"
          style={{ animationDuration: `${Math.max(live.phaseMs, 1)}ms` }}
          aria-hidden="true"
        />
      ) : null}
      {live.active && live.phase === "check" && live.checks > 0 ? (
        <span key={`c${live.checks}`} className="block-check-flash" aria-hidden="true" />
      ) : null}
      <LiveLabel live={live} />
      {ports.map((port, index) => {
        const side = index === 0 ? "bottom" : "right";
        const tone = portTone(node.type, port);
        return (
          <Fragment key={port}>
            <Handle
              type="source"
              id={port}
              position={index === 0 ? Position.Bottom : Position.Right}
              className={cx("block-handle", `block-handle-${tone}`)}
            />
            {ports.length > 1 ? (
              <span
                className={cx(
                  "block-port",
                  `block-port-${side}`,
                  `block-port-${tone}`,
                  live.outcome === port && "block-port-taken",
                )}
              >
                {portLabel(node.type, port, translator)}
              </span>
            ) : null}
            {wired.includes(port) ? null : (
              <span
                className={cx(
                  "block-end",
                  `block-end-${side}`,
                  ports.length > 1 && "block-end-labelled",
                  failsUnlinked(port) && "block-end-bad",
                )}
                title={
                  port === "timeout"
                    ? t("canvas.portEndTimeout")
                    : port === "error"
                      ? t("canvas.portEndError")
                      : t("canvas.portEnd")
                }
              >
                {t("canvas.portEndMark")}
              </span>
            )}
          </Fragment>
        );
      })}
    </div>
  );
});
