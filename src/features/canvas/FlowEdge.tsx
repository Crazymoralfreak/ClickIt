import { BaseEdge, EdgeLabelRenderer, getSmoothStepPath, Position, useReactFlow, type EdgeProps } from "@xyflow/react";
import { memo, useRef, useState } from "react";
import { useT } from "../../i18n";
import type { GraphPoint } from "../../types/scenario";
import { cx } from "../../ui";
import { useCanvas, type CanvasEdge } from "./canvasContext";
import { alignBend } from "./graph";
import { useEdgeLive } from "./useRunTrace";

const NO_POINTS: GraphPoint[] = [];
/** How far a link goes straight out of a block before it turns; loops go wider to be seen. */
const STEP_OFFSET = 18;
const BACK_OFFSET = 34;
/** Same rounding as a link without bend points. */
const CORNER_RADIUS = 10;
/** The arrow head, pointing right with its tip at the origin. */
const ARROW = "0,0 -9,-4.5 -9,4.5";

function forward(point: GraphPoint, position: Position, distance: number): GraphPoint {
  switch (position) {
    case Position.Left:
      return { x: point.x - distance, y: point.y };
    case Position.Right:
      return { x: point.x + distance, y: point.y };
    case Position.Top:
      return { x: point.x, y: point.y - distance };
    case Position.Bottom:
      return { x: point.x, y: point.y + distance };
    default: {
      const unreachable: never = position;
      return unreachable;
    }
  }
}

function addCorner(corners: GraphPoint[], point: GraphPoint): void {
  const last = corners[corners.length - 1];
  if (!last || last.x !== point.x || last.y !== point.y) {
    corners.push(point);
  }
}

/** One right angle between two points. The second point is where the pointer put the corner. */
function elbow(from: GraphPoint, to: GraphPoint, horizontalFirst: boolean): GraphPoint[] {
  if (Math.abs(from.x - to.x) < 0.5 || Math.abs(from.y - to.y) < 0.5) {
    return [to];
  }
  return [horizontalFirst ? { x: to.x, y: from.y } : { x: from.x, y: to.y }, to];
}

/**
 * The corners of a link that has bend points: straight out of the block, right angles
 * through the points, straight into the next block. A diagonal never reaches the screen.
 */
function bendCorners(
  sourceX: number,
  sourceY: number,
  sourcePosition: Position,
  targetX: number,
  targetY: number,
  targetPosition: Position,
  points: readonly GraphPoint[],
  offset: number,
): GraphPoint[] {
  const source = { x: sourceX, y: sourceY };
  const target = { x: targetX, y: targetY };
  const leave = forward(source, sourcePosition, offset);
  const enter = forward(target, targetPosition, offset);
  const corners = [source];
  addCorner(corners, leave);
  let at = leave;
  let horizontal = sourcePosition === Position.Left || sourcePosition === Position.Right;
  for (const next of [...points, enter]) {
    for (const corner of elbow(at, next, !horizontal)) {
      addCorner(corners, corner);
    }
    at = corners[corners.length - 1];
    const before = corners[corners.length - 2];
    horizontal = before !== undefined && Math.abs(at.y - before.y) < 0.5;
  }
  addCorner(corners, target);
  return corners;
}

/** The same corners, with the turn rounded instead of sharp. */
function roundedPath(corners: readonly GraphPoint[]): string {
  const first = corners[0];
  if (!first) {
    return "";
  }
  let path = `M ${first.x},${first.y}`;
  for (let index = 1; index < corners.length - 1; index += 1) {
    const prev = corners[index - 1];
    const corner = corners[index];
    const next = corners[index + 1];
    const inLength = Math.hypot(corner.x - prev.x, corner.y - prev.y);
    const outLength = Math.hypot(next.x - corner.x, next.y - corner.y);
    const radius = Math.min(CORNER_RADIUS, inLength / 2, outLength / 2);
    if (radius < 1 || inLength === 0 || outLength === 0) {
      path += ` L ${corner.x},${corner.y}`;
      continue;
    }
    const before = { x: corner.x - ((corner.x - prev.x) / inLength) * radius, y: corner.y - ((corner.y - prev.y) / inLength) * radius };
    const after = { x: corner.x + ((next.x - corner.x) / outLength) * radius, y: corner.y + ((next.y - corner.y) / outLength) * radius };
    path += ` L ${before.x},${before.y} Q ${corner.x},${corner.y} ${after.x},${after.y}`;
  }
  const last = corners[corners.length - 1];
  return `${path} L ${last.x},${last.y}`;
}

/** A link between an output and a block: the colour of its output, dashed when it is a loop. */
export const FlowEdge = memo(function FlowEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  data,
  selected,
}: EdgeProps<CanvasEdge>) {
  const { t } = useT();
  const canvas = useCanvas();
  const flow = useReactFlow();
  const key = data?.key ?? id;
  const live = useEdgeLive(canvas.store, key);
  /** The bend point being dragged; the graph gets it when the button is released. */
  const [drag, setDrag] = useState<{ index: number; x: number; y: number } | null>(null);
  // The move handler must not read `drag`: that value is still empty on the press that starts the drag.
  const dragRef = useRef(drag);

  const stored = data?.editable ? data.points : NO_POINTS;
  const points = drag ? stored.map((point, index) => (index === drag.index ? { x: drag.x, y: drag.y } : point)) : stored;

  let path: string;
  // Links come into a block from above, so the arrow points down unless a bend point says otherwise.
  let angle = 90;
  if (points.length > 0) {
    const corners = bendCorners(
      sourceX,
      sourceY,
      sourcePosition,
      targetX,
      targetY,
      targetPosition,
      points,
      data?.back ? BACK_OFFSET : STEP_OFFSET,
    );
    path = roundedPath(corners);
    const last = corners[corners.length - 1];
    const before = corners[corners.length - 2] ?? last;
    angle = (Math.atan2(last.y - before.y, last.x - before.x) * 180) / Math.PI;
  } else {
    [path] = getSmoothStepPath({
      sourceX,
      sourceY,
      sourcePosition,
      targetX,
      targetY,
      targetPosition,
      borderRadius: 10,
      offset: data?.back ? BACK_OFFSET : STEP_OFFSET,
    });
  }

  return (
    <g
      className={cx(
        "flow-edge",
        `flow-edge-${data?.tone ?? "plain"}`,
        data?.back && "flow-edge-back",
        data?.dim && "flow-edge-dim",
        selected && "flow-edge-selected",
        live.passes > 0 && "flow-edge-passed",
      )}
    >
      <BaseEdge id={id} path={path} interactionWidth={18} className="flow-edge-line" />
      <polygon className="flow-edge-arrow" points={ARROW} transform={`translate(${targetX},${targetY}) rotate(${angle})`} />
      {live.flash > 0 ? <path key={`f${live.flash}`} d={path} className="flow-edge-flash" /> : null}
      {live.flash > 0 ? <path key={`r${live.flash}`} d={path} pathLength={100} className="flow-edge-runner" /> : null}
      {selected && data?.editable ? (
        <EdgeLabelRenderer>
          {points.map((point, index) => (
            <div
              key={index}
              className="flow-bend nodrag nopan"
              style={{ transform: `translate(-50%, -50%) translate(${point.x}px, ${point.y}px)` }}
              title={t("canvas.bendHint")}
              onPointerDown={(event) => {
                event.stopPropagation();
                event.preventDefault();
                const next = { index, x: point.x, y: point.y };
                dragRef.current = next;
                setDrag(next);
                event.currentTarget.setPointerCapture(event.pointerId);
              }}
              onPointerMove={(event) => {
                const current = dragRef.current;
                if (!current || current.index !== index) {
                  return;
                }
                event.stopPropagation();
                const at = flow.screenToFlowPosition({ x: event.clientX, y: event.clientY });
                const anchors = [
                  { x: sourceX, y: sourceY },
                  { x: targetX, y: targetY },
                  ...stored.filter((_, at) => at !== index),
                ];
                const next = { index, ...alignBend(at, anchors, flow.getZoom()) };
                dragRef.current = next;
                setDrag(next);
              }}
              onPointerUp={(event) => {
                const current = dragRef.current;
                if (!current || current.index !== index) {
                  return;
                }
                event.stopPropagation();
                if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                  event.currentTarget.releasePointerCapture(event.pointerId);
                }
                dragRef.current = null;
                setDrag(null);
                const next = stored.map((bend, at) => (at === current.index ? { x: current.x, y: current.y } : bend));
                canvas.setBends(key, next);
              }}
              onDoubleClick={(event) => {
                event.stopPropagation();
                event.preventDefault();
                dragRef.current = null;
                setDrag(null);
                canvas.setBends(
                  key,
                  stored.filter((_, at) => at !== index),
                );
              }}
            />
          ))}
        </EdgeLabelRenderer>
      ) : null}
    </g>
  );
});
