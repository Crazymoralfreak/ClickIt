import { BaseEdge, EdgeLabelRenderer, getSmoothStepPath, useReactFlow, type EdgeProps } from "@xyflow/react";
import { memo, useState } from "react";
import { useT } from "../../i18n";
import type { GraphPoint } from "../../types/scenario";
import { cx } from "../../ui";
import { useCanvas, type CanvasEdge } from "./canvasContext";
import { useEdgeLive } from "./useRunTrace";

const NO_POINTS: GraphPoint[] = [];
/** How far a link goes straight out of a block before it turns; loops go wider to be seen. */
const STEP_OFFSET = 18;
const BACK_OFFSET = 34;
/** The arrow head, pointing right with its tip at the origin. */
const ARROW = "0,0 -9,-4.5 -9,4.5";

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

  const stored = data?.editable ? data.points : NO_POINTS;
  const points = drag ? stored.map((point, index) => (index === drag.index ? { x: drag.x, y: drag.y } : point)) : stored;

  let path: string;
  // Links come into a block from above, so the arrow points down unless a bend point says otherwise.
  let angle = 90;
  if (points.length > 0) {
    path = [`M ${sourceX},${sourceY}`, ...points.map((point) => `L ${point.x},${point.y}`), `L ${targetX},${targetY}`].join(" ");
    const last = points[points.length - 1];
    angle = (Math.atan2(targetY - last.y, targetX - last.x) * 180) / Math.PI;
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
                event.currentTarget.setPointerCapture(event.pointerId);
                setDrag({ index, x: point.x, y: point.y });
              }}
              onPointerMove={(event) => {
                if (drag && drag.index === index) {
                  const at = flow.screenToFlowPosition({ x: event.clientX, y: event.clientY });
                  setDrag({ index, x: Math.round(at.x), y: Math.round(at.y) });
                }
              }}
              onPointerUp={(event) => {
                if (drag && drag.index === index) {
                  event.currentTarget.releasePointerCapture(event.pointerId);
                  setDrag(null);
                  canvas.setBends(key, points);
                }
              }}
              onDoubleClick={(event) => {
                event.stopPropagation();
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
