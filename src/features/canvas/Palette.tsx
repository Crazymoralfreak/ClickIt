import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type RefObject } from "react";
import { useT } from "../../i18n";
import type { NodeType, Snippet } from "../../types/scenario";
import { cx } from "../../ui";
import { ROLE_KEYS, typeName } from "./blockText";
import { PALETTE, roleOf } from "./graph";
import { BlockIcon } from "./icons";
import { BUILTIN_SNIPPETS, type PaletteItem } from "./snippets";

/** A press that moves further than this is a drag, not a click. */
const DRAG_THRESHOLD = 6;

interface PaletteListProps {
  /** A graph has one start block; the list offers it only when it is missing. */
  hasStart: boolean;
  snippets: ReadonlyArray<Snippet>;
  disabled: boolean;
  onPick: (item: PaletteItem) => void;
  /** Set for the docked list: an item dragged onto the canvas is put where it was dropped. */
  onDrop?: (item: PaletteItem, clientX: number, clientY: number) => void;
  onDeleteSnippet?: (snippet: Snippet) => void;
}

interface Ghost {
  label: string;
  x: number;
  y: number;
}

function PaletteList({ hasStart, snippets, disabled, onPick, onDrop, onDeleteSnippet }: PaletteListProps) {
  const translator = useT();
  const { t } = translator;
  const [ghost, setGhost] = useState<Ghost | null>(null);
  const press = useRef<{ x: number; y: number; dragged: boolean } | null>(null);

  // The pointer handlers do the dragging; HTML drag and drop is taken by the window for dropped files.
  const dragProps = (item: PaletteItem, label: string) => {
    if (!onDrop) {
      return {};
    }
    return {
      onPointerDown: (event: ReactPointerEvent<HTMLButtonElement>) => {
        if (event.button !== 0 || disabled) {
          return;
        }
        press.current = { x: event.clientX, y: event.clientY, dragged: false };
        event.currentTarget.setPointerCapture(event.pointerId);
      },
      onPointerMove: (event: ReactPointerEvent<HTMLButtonElement>) => {
        const start = press.current;
        if (!start) {
          return;
        }
        if (!start.dragged && Math.hypot(event.clientX - start.x, event.clientY - start.y) < DRAG_THRESHOLD) {
          return;
        }
        start.dragged = true;
        setGhost({ label, x: event.clientX, y: event.clientY });
      },
      onPointerUp: (event: ReactPointerEvent<HTMLButtonElement>) => {
        const start = press.current;
        if (start?.dragged) {
          onDrop(item, event.clientX, event.clientY);
        }
        setGhost(null);
        // The click that follows a drag must not add a second block.
        window.setTimeout(() => {
          press.current = null;
        }, 0);
      },
      onPointerCancel: () => {
        press.current = null;
        setGhost(null);
      },
    };
  };

  const pick = (item: PaletteItem) => {
    if (!press.current?.dragged) {
      onPick(item);
    }
  };

  const blockButton = (type: NodeType) => {
    const label = typeName(type, translator);
    const item: PaletteItem = { kind: "block", type };
    return (
      <button
        key={type}
        type="button"
        className={cx("palette-item", `palette-item-${roleOf(type)}`)}
        disabled={disabled}
        onClick={() => pick(item)}
        {...dragProps(item, label)}
      >
        <BlockIcon type={type} />
        <span className="palette-item-name">{label}</span>
      </button>
    );
  };

  return (
    <>
      {PALETTE.map((group) => {
        const types = group.role === "entry" && !hasStart ? (["start", ...group.types] as NodeType[]) : group.types;
        return (
          <div key={group.role} className="palette-group">
            <div className="palette-group-title">{t(ROLE_KEYS[group.role])}</div>
            {types.map(blockButton)}
          </div>
        );
      })}
      <div className="palette-group">
        <div className="palette-group-title">{t("canvas.palette.snippets")}</div>
        {BUILTIN_SNIPPETS.map((snippet) => {
          const item: PaletteItem = { kind: "builtin", id: snippet.id };
          const label = t(snippet.name);
          return (
            <button
              key={snippet.id}
              type="button"
              className="palette-item palette-item-snippet"
              disabled={disabled}
              onClick={() => pick(item)}
              {...dragProps(item, label)}
            >
              <span className="palette-item-name">{label}</span>
            </button>
          );
        })}
        {snippets.map((snippet) => {
          const item: PaletteItem = { kind: "snippet", id: snippet.id };
          return (
            <div key={snippet.id} className="palette-snippet">
              <button
                type="button"
                className="palette-item palette-item-snippet"
                disabled={disabled}
                onClick={() => pick(item)}
                {...dragProps(item, snippet.name)}
              >
                <span className="palette-item-name">{snippet.name}</span>
              </button>
              {onDeleteSnippet ? (
                <button
                  type="button"
                  className="palette-snippet-delete"
                  title={t("canvas.palette.snippetDelete")}
                  aria-label={t("canvas.palette.snippetDelete")}
                  onClick={() => onDeleteSnippet(snippet)}
                >
                  ×
                </button>
              ) : null}
            </div>
          );
        })}
        {snippets.length === 0 && onDeleteSnippet ? (
          <div className="palette-empty">{t("canvas.palette.snippetsEmpty")}</div>
        ) : null}
      </div>
      {ghost ? (
        <div className="palette-ghost" style={{ left: ghost.x, top: ghost.y }}>
          {ghost.label}
        </div>
      ) : null}
    </>
  );
}

/** The list of blocks and snippets docked at the left side of the canvas. */
export function Palette(props: PaletteListProps) {
  const { t } = useT();
  return (
    <aside className="canvas-palette" aria-label={t("canvas.toolbar.blocks")}>
      <div className="palette-hint">{t("canvas.palette.hint")}</div>
      <PaletteList {...props} />
    </aside>
  );
}

/** Closes a popup on Escape and on a press anywhere outside it. */
function useDismiss(box: RefObject<HTMLDivElement | null>, onClose: () => void): void {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    const onDown = (event: PointerEvent) => {
      if (box.current && event.target instanceof Node && !box.current.contains(event.target)) {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onDown, true);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onDown, true);
    };
  }, [box, onClose]);
}

interface PalettePopupProps {
  /** Pixels from the top left corner of the canvas. */
  x: number;
  y: number;
  hasStart: boolean;
  snippets: ReadonlyArray<Snippet>;
  onPick: (item: PaletteItem) => void;
  onClose: () => void;
}

/** The same list at the pointer: after a link was dropped on empty space, or from the menu of the canvas. */
export function PalettePopup({ x, y, hasStart, snippets, onPick, onClose }: PalettePopupProps) {
  const { t } = useT();
  const box = useRef<HTMLDivElement>(null);

  useDismiss(box, onClose);

  return (
    <div
      ref={box}
      className="canvas-popup canvas-popup-palette"
      style={{ left: x, top: y }}
      role="menu"
      aria-label={t("canvas.toolbar.blocks")}
    >
      <PaletteList hasStart={hasStart} snippets={snippets} disabled={false} onPick={onPick} />
    </div>
  );
}

export interface MenuItem {
  label: string;
  danger?: boolean;
  disabled?: boolean;
  run: () => void;
}

interface CanvasMenuProps {
  x: number;
  y: number;
  items: ReadonlyArray<MenuItem>;
  onClose: () => void;
}

/** The menu of a block, opened with the right mouse button. */
export function CanvasMenu({ x, y, items, onClose }: CanvasMenuProps) {
  const box = useRef<HTMLDivElement>(null);

  useDismiss(box, onClose);

  return (
    <div ref={box} className="canvas-popup canvas-menu" style={{ left: x, top: y }} role="menu">
      {items.map((item) => (
        <button
          key={item.label}
          type="button"
          role="menuitem"
          className={cx("canvas-menu-item", item.danger && "canvas-menu-item-danger")}
          disabled={item.disabled}
          onClick={() => {
            onClose();
            item.run();
          }}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
