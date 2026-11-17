import { useState, type KeyboardEvent } from "react";
import { useT } from "../../i18n";
import { cx } from "../../ui";

const MODIFIER_CODES: ReadonlySet<string> = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
]);

/** `Ctrl+Shift+KeyR`: modifiers in the order the core expects, then the W3C key code.
 *  Null for a press of a modifier alone and for keys without a code. */
function hotkeyFromEvent(event: KeyboardEvent<HTMLInputElement>): string | null {
  const code = event.code;
  if (code === "" || code === "Unidentified" || MODIFIER_CODES.has(code)) {
    return null;
  }
  const parts: string[] = [];
  if (event.ctrlKey) {
    parts.push("Ctrl");
  }
  if (event.altKey) {
    parts.push("Alt");
  }
  if (event.shiftKey) {
    parts.push("Shift");
  }
  if (event.metaKey) {
    parts.push("Win");
  }
  parts.push(code);
  return parts.join("+");
}

interface HotkeyInputProps {
  value: string;
  /** Called with a combination that differs from `value`. */
  onChange: (hotkey: string) => void;
  /** Accessible name of the field. */
  label: string;
  disabled?: boolean;
}

/** Field that takes the next key press as the new combination while it has the focus. */
export function HotkeyInput({ value, onChange, label, disabled }: HotkeyInputProps) {
  const { t } = useT();
  const [listening, setListening] = useState(false);

  return (
    <input
      type="text"
      readOnly
      className={cx("input", "settings-hotkey", listening && "settings-hotkey-listening")}
      value={listening ? t("settings.hotkeyPress") : value}
      disabled={disabled}
      aria-label={label}
      onFocus={() => setListening(true)}
      onBlur={() => setListening(false)}
      onKeyDown={(event) => {
        // Plain Tab and Shift+Tab keep moving the focus, otherwise the field would trap the keyboard.
        if (event.code === "Tab" && !event.ctrlKey && !event.altKey && !event.metaKey) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        if (event.code === "Escape") {
          event.currentTarget.blur();
          return;
        }
        const hotkey = hotkeyFromEvent(event);
        if (hotkey === null) {
          return;
        }
        event.currentTarget.blur();
        if (hotkey !== value) {
          onChange(hotkey);
        }
      }}
    />
  );
}
