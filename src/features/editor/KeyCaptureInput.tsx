import { useState, type KeyboardEvent } from "react";
import { useT } from "../../i18n";
import { cx } from "../../ui";

interface KeyCaptureInputProps {
  /** `KeyboardEvent.code` name, empty when no key is chosen. */
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  invalid?: boolean;
  label: string;
}

// Keys that the system, the webview or a global hotkey may take before the field sees them.
const SPECIAL_KEYS: ReadonlyArray<string> = [
  "Escape",
  "Tab",
  "Enter",
  "Space",
  "Backspace",
  "Delete",
  "Insert",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "ArrowLeft",
  "ArrowUp",
  "ArrowRight",
  "ArrowDown",
  "PrintScreen",
  "Pause",
  "CapsLock",
  "NumLock",
  "ScrollLock",
  "ContextMenu",
  "MetaLeft",
  "MetaRight",
  "F1",
  "F2",
  "F3",
  "F4",
  "F5",
  "F6",
  "F7",
  "F8",
  "F9",
  "F10",
  "F11",
  "F12",
];

/**
 * Field that stores the next pressed key as its `KeyboardEvent.code`.
 * A click (or Enter / Space) arms it, so that moving the focus with Tab does not get captured.
 */
export function KeyCaptureInput({ value, onChange, disabled, invalid, label }: KeyCaptureInputProps) {
  const { t } = useT();
  const [listening, setListening] = useState(false);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!listening) {
      // A captured Enter that is still held down would "click" the field and arm it again.
      if (event.repeat && (event.code === "Enter" || event.code === "NumpadEnter" || event.code === "Space")) {
        event.preventDefault();
      }
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (event.code === "" || event.code === "Unidentified") {
      return;
    }
    onChange(event.code);
    setListening(false);
  };

  return (
    <span className="key-capture">
      <button
        type="button"
        className={cx(
          "input",
          "key-capture-field",
          listening && "key-capture-listening",
          value === "" && "key-capture-empty",
          invalid && "input-invalid",
        )}
        disabled={disabled}
        aria-label={label}
        onClick={() => setListening(true)}
        onBlur={() => setListening(false)}
        onKeyDown={onKeyDown}
      >
        {listening ? t("steps.keyInput.listening") : value === "" ? t("steps.keyInput.empty") : value}
      </button>
      <select
        className="input select"
        value=""
        disabled={disabled}
        aria-label={t("steps.keyInput.special")}
        onChange={(event) => {
          if (event.target.value !== "") {
            onChange(event.target.value);
          }
        }}
      >
        <option value="">{t("steps.keyInput.special")}</option>
        {SPECIAL_KEYS.map((code) => (
          <option key={code} value={code}>
            {code}
          </option>
        ))}
      </select>
    </span>
  );
}
