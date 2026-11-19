import { Handle, NodeResizer, Position, type NodeProps } from "@xyflow/react";
import { memo, type CSSProperties } from "react";
import { useT } from "../../i18n";
import { cx } from "../../ui";
import { noteColor, useCanvas, type NoteFlowNode } from "./canvasContext";
import { FRAME_MIN_HEIGHT, FRAME_MIN_WIDTH } from "./graph";
import { useActiveNode } from "./useRunTrace";

const NOTE_MIN_WIDTH = 120;
const NOTE_MIN_HEIGHT = 60;

/** A note, or a frame around blocks. A collapsed frame looks like one block. */
export const NoteNode = memo(function NoteNode({ id, data, selected }: NodeProps<NoteFlowNode>) {
  const { t } = useT();
  const canvas = useCanvas();
  const active = useActiveNode(canvas.store);
  const { note } = data;
  const style = { "--note-color": noteColor(note.color) } as CSSProperties;

  if (note.kind === "note") {
    return (
      <div className={cx("note", selected && "note-selected")} style={style}>
        <NodeResizer
          isVisible={selected && !data.locked}
          minWidth={NOTE_MIN_WIDTH}
          minHeight={NOTE_MIN_HEIGHT}
          lineClassName="note-resize-line"
          handleClassName="note-resize-handle"
          onResizeEnd={(_event, rect) => canvas.resizeNote(id, rect)}
        />
        <div className={cx("note-text", note.text === "" && "note-text-empty")}>
          {note.text === "" ? t("canvas.noteDefault") : note.text}
        </div>
      </div>
    );
  }

  const title = note.text === "" ? t("canvas.frameDefault") : note.text;
  // A frame is at work while a block inside it is.
  const running = active !== null && data.members !== "" && data.members.split(",").includes(active);

  if (note.collapsed) {
    return (
      <div className={cx("frame-collapsed", selected && "frame-selected", running && "frame-running")} style={style}>
        <Handle type="target" id="in" position={Position.Top} isConnectable={false} className="block-handle block-handle-in" />
        <button
          type="button"
          className="frame-toggle nodrag nopan"
          title={t("canvas.expand")}
          aria-label={t("canvas.expand")}
          onClick={(event) => {
            event.stopPropagation();
            canvas.toggleCollapsed(id);
          }}
        >
          <span className="frame-mark" aria-hidden="true" />
        </button>
        <span className="frame-collapsed-text">
          <span className="frame-title" title={title}>
            {title}
          </span>
          <span className="frame-count">{t("canvas.block.frameBlocks", { count: data.blocks })}</span>
        </span>
        <Handle type="source" id="out" position={Position.Bottom} isConnectable={false} className="block-handle" />
      </div>
    );
  }

  return (
    <div className={cx("frame", selected && "frame-selected", running && "frame-running")} style={style}>
      <NodeResizer
        isVisible={selected && !data.locked}
        minWidth={FRAME_MIN_WIDTH}
        minHeight={FRAME_MIN_HEIGHT}
        lineClassName="note-resize-line"
        handleClassName="note-resize-handle"
        onResizeEnd={(_event, rect) => canvas.resizeNote(id, rect)}
      />
      <div className="frame-head">
        <button
          type="button"
          className="frame-toggle nodrag nopan"
          title={t("canvas.collapse")}
          aria-label={t("canvas.collapse")}
          onClick={(event) => {
            event.stopPropagation();
            canvas.toggleCollapsed(id);
          }}
        >
          <span className="frame-mark frame-mark-open" aria-hidden="true" />
        </button>
        <span className="frame-title" title={title}>
          {title}
        </span>
      </div>
    </div>
  );
});
