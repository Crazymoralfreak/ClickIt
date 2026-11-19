import { useEffect, useRef, useState } from "react";
import { cx } from "../../ui";

export interface TabItem {
  id: string;
  label: string;
  /** Tooltip of the label, e.g. the hint about renaming. */
  title?: string;
  /** Tooltip and accessible name of the cross. */
  closeTitle: string;
  closeDisabled?: boolean;
  /** Something is going on in this tab: an answer is being written. */
  busy?: boolean;
}

interface TabStripProps {
  /** Accessible name of the whole strip. */
  label: string;
  items: TabItem[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  /** Shows a "+" after the last tab. */
  onAdd?: () => void;
  addTitle?: string;
  /** A double click on a label turns it into a field; Enter keeps the new text. */
  onRename?: (id: string, label: string) => void;
}

const MAX_LABEL = 80;

/** A row of tabs with a cross on each one. It scrolls sideways when the tabs do not fit. */
export function TabStrip({ label, items, activeId, onSelect, onClose, onAdd, addTitle, onRename }: TabStripProps) {
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const strip = useRef<HTMLDivElement>(null);

  // The chosen tab stays in sight when the strip is scrolled or a new tab appears at its end.
  useEffect(() => {
    strip.current?.querySelector(".assistant-tab-active")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeId, items.length]);

  const finish = (keep: boolean) => {
    const text = editing?.text.trim() ?? "";
    const item = items.find((tab) => tab.id === editing?.id);
    setEditing(null);
    if (keep && item && text !== "" && text !== item.label) {
      onRename?.(item.id, text);
    }
  };

  return (
    <div className="assistant-tabs" role="tablist" aria-label={label} ref={strip}>
      {items.map((item) => (
        <div
          key={item.id}
          className={cx("assistant-tab", item.id === activeId && "assistant-tab-active", item.busy && "assistant-tab-busy")}
        >
          {editing?.id === item.id ? (
            <input
              className="assistant-tab-input"
              value={editing.text}
              maxLength={MAX_LABEL}
              autoFocus
              onFocus={(event) => event.target.select()}
              onChange={(event) => setEditing({ id: item.id, text: event.target.value })}
              onBlur={() => finish(true)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  finish(true);
                } else if (event.key === "Escape") {
                  // The page has nothing else to close: the key only leaves the field.
                  event.stopPropagation();
                  finish(false);
                }
              }}
            />
          ) : (
            <button
              type="button"
              role="tab"
              aria-selected={item.id === activeId}
              className="assistant-tab-label"
              title={item.title}
              onClick={() => onSelect(item.id)}
              onDoubleClick={onRename ? () => setEditing({ id: item.id, text: item.label }) : undefined}
            >
              {item.label}
            </button>
          )}
          <button
            type="button"
            className="assistant-tab-close"
            aria-label={item.closeTitle}
            title={item.closeTitle}
            disabled={item.closeDisabled}
            onClick={() => onClose(item.id)}
          >
            ×
          </button>
        </div>
      ))}
      {onAdd ? (
        <button type="button" className="assistant-tab-add" aria-label={addTitle} title={addTitle} onClick={onAdd}>
          +
        </button>
      ) : null}
    </div>
  );
}
