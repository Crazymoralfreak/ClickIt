// Shared UI primitives. Feature screens build on these and on the classes in styles.css.

import {
  useEffect,
  useState,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
} from "react";
import { useT } from "../i18n";
import type { Pause, RepeatMode } from "../types/scenario";

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

type ButtonVariant = "default" | "primary" | "danger" | "ghost";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  small?: boolean;
}

export function Button({ variant = "default", small, className, type, ...rest }: ButtonProps) {
  return (
    <button
      type={type ?? "button"}
      className={cx("btn", `btn-${variant}`, small && "btn-small", className)}
      {...rest}
    />
  );
}

interface TextInputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange"> {
  value: string;
  onChange: (value: string) => void;
  invalid?: boolean;
}

export function TextInput({ value, onChange, invalid, className, ...rest }: TextInputProps) {
  return (
    <input
      type="text"
      className={cx("input", invalid && "input-invalid", className)}
      value={value}
      onChange={(event) => onChange(event.target.value)}
      {...rest}
    />
  );
}

interface NumberInputProps {
  value: number;
  /** Called with every whole number typed; values out of range are left to validation. */
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  disabled?: boolean;
  invalid?: boolean;
  /** Short unit shown after the field, e.g. "ms". */
  suffix?: string;
  wide?: boolean;
  title?: string;
}

export function NumberInput({ value, onChange, min, max, disabled, invalid, suffix, wide, title }: NumberInputProps) {
  const [text, setText] = useState(String(value));

  useEffect(() => {
    setText((current) => (Number(current) === value && current.trim() !== "" ? current : String(value)));
  }, [value]);

  const input = (
    <input
      type="number"
      className={cx("input", "input-number", wide && "input-number-wide", invalid && "input-invalid")}
      value={text}
      min={min}
      max={max}
      step={1}
      disabled={disabled}
      title={title}
      onChange={(event) => {
        const next = event.target.value;
        setText(next);
        const parsed = Number(next);
        if (next.trim() !== "" && Number.isFinite(parsed)) {
          onChange(Math.trunc(parsed));
        }
      }}
      onBlur={() => setText(String(value))}
    />
  );
  if (!suffix) {
    return input;
  }
  return (
    <span className="input-with-suffix">
      {input}
      <span className="input-suffix">{suffix}</span>
    </span>
  );
}

export interface Option<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
}

interface SelectProps<T extends string> {
  value: T;
  options: ReadonlyArray<Option<T>>;
  onChange: (value: T) => void;
  disabled?: boolean;
  invalid?: boolean;
  title?: string;
}

export function Select<T extends string>({ value, options, onChange, disabled, invalid, title }: SelectProps<T>) {
  return (
    <select
      className={cx("input", "select", invalid && "input-invalid")}
      value={value}
      disabled={disabled}
      title={title}
      onChange={(event) => onChange(event.target.value as T)}
    >
      {options.map((option) => (
        <option key={option.value} value={option.value} disabled={option.disabled}>
          {option.label}
        </option>
      ))}
    </select>
  );
}

/** Row of mutually exclusive buttons. */
export function Segmented<T extends string>({ value, options, onChange, disabled }: SelectProps<T>) {
  return (
    <div className="segmented" role="radiogroup">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          className={cx("segmented-item", option.value === value && "segmented-item-active")}
          disabled={disabled || option.disabled}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: ReactNode;
  disabled?: boolean;
  title?: string;
}

export function Checkbox({ checked, onChange, label, disabled, title }: CheckboxProps) {
  return (
    <label className={cx("checkbox", disabled && "checkbox-disabled")} title={title}>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span>{label}</span>
    </label>
  );
}

interface FieldProps {
  label: ReactNode;
  /** Grey explanatory line under the control. */
  hint?: ReactNode;
  /** Red line under the control. */
  error?: ReactNode;
  children: ReactNode;
}

/** Label on the left, control with hint and error on the right. */
export function Field({ label, hint, error, children }: FieldProps) {
  return (
    <div className="field">
      <div className="field-label">{label}</div>
      <div className="field-body">
        <div className="field-control">{children}</div>
        {hint ? <div className="field-hint">{hint}</div> : null}
        {error ? <div className="field-error">{error}</div> : null}
      </div>
    </div>
  );
}

interface PauseInputProps {
  value: Pause;
  onChange: (value: Pause) => void;
  disabled?: boolean;
  invalid?: boolean;
}

/** Pause in milliseconds: an exact value or a "from–to" range. */
export function PauseInput({ value, onChange, disabled, invalid }: PauseInputProps) {
  const { t } = useT();
  const [range, setRange] = useState(value.fromMs !== value.toMs);

  useEffect(() => {
    if (value.fromMs !== value.toMs) {
      setRange(true);
    }
  }, [value.fromMs, value.toMs]);

  return (
    <span className="pause-input">
      <Select
        value={range ? "range" : "exact"}
        disabled={disabled}
        options={[
          { value: "exact", label: t("common.pauseExact") },
          { value: "range", label: t("common.pauseRange") },
        ]}
        onChange={(mode) => {
          setRange(mode === "range");
          if (mode === "exact") {
            onChange({ fromMs: value.fromMs, toMs: value.fromMs });
          }
        }}
      />
      {range ? (
        <>
          <span className="pause-input-word">{t("common.from")}</span>
          <NumberInput
            value={value.fromMs}
            min={0}
            disabled={disabled}
            invalid={invalid}
            onChange={(fromMs) => onChange({ fromMs, toMs: value.toMs })}
          />
          <span className="pause-input-word">{t("common.to")}</span>
          <NumberInput
            value={value.toMs}
            min={0}
            disabled={disabled}
            invalid={invalid}
            suffix={t("common.ms")}
            onChange={(toMs) => onChange({ fromMs: value.fromMs, toMs })}
          />
        </>
      ) : (
        <NumberInput
          value={value.fromMs}
          min={0}
          disabled={disabled}
          invalid={invalid}
          suffix={t("common.ms")}
          onChange={(ms) => onChange({ fromMs: ms, toMs: ms })}
        />
      )}
    </span>
  );
}

interface RepeatModeInputProps {
  value: RepeatMode;
  onChange: (value: RepeatMode) => void;
  disabled?: boolean;
  invalid?: boolean;
}

/** Repeat of a scenario or a queue: once, N times or until stopped. */
export function RepeatModeInput({ value, onChange, disabled, invalid }: RepeatModeInputProps) {
  const { t } = useT();
  return (
    <span className="pause-input">
      <Select
        value={value.kind}
        disabled={disabled}
        options={[
          { value: "once", label: t("common.repeatOnce") },
          { value: "count", label: t("common.repeatCount") },
          { value: "forever", label: t("common.repeatForever") },
        ]}
        onChange={(kind) => {
          switch (kind) {
            case "once":
              onChange({ kind: "once" });
              break;
            case "count":
              onChange({ kind: "count", times: value.kind === "count" ? value.times : 2 });
              break;
            case "forever":
              onChange({ kind: "forever" });
              break;
            default: {
              const unreachable: never = kind;
              throw new Error(`Unknown repeat mode: ${String(unreachable)}`);
            }
          }
        }}
      />
      {value.kind === "count" ? (
        <NumberInput
          value={value.times}
          min={1}
          disabled={disabled}
          invalid={invalid}
          suffix={t("common.times")}
          onChange={(times) => onChange({ kind: "count", times })}
        />
      ) : null}
    </span>
  );
}

interface ModalProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Buttons at the bottom, right aligned. */
  footer?: ReactNode;
  wide?: boolean;
}

export function Modal({ title, onClose, children, footer, wide }: ModalProps) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className={cx("modal", wide && "modal-wide")} role="dialog" aria-modal="true" aria-label={title}>
        <div className="modal-title">{title}</div>
        <div className="modal-body">{children}</div>
        {footer ? <div className="modal-footer">{footer}</div> : null}
      </div>
    </div>
  );
}

interface EmptyStateProps {
  title: string;
  text?: ReactNode;
  action?: ReactNode;
}

export function EmptyState({ title, text, action }: EmptyStateProps) {
  return (
    <div className="empty-state">
      <div className="empty-state-title">{title}</div>
      {text ? <div className="empty-state-text">{text}</div> : null}
      {action ? <div className="empty-state-action">{action}</div> : null}
    </div>
  );
}

/** Page frame: title row with actions, then scrollable content. */
export function PageFrame({ title, actions, children }: { title: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <div className="page">
      <div className="page-header">
        <h1 className="page-title">{title}</h1>
        <div className="page-actions">{actions}</div>
      </div>
      <div className="page-content">{children}</div>
    </div>
  );
}

/** Titled group of fields inside a page. A collapsible group starts closed and opens on a click of its title. */
export function Section({
  title,
  children,
  collapsible = false,
  defaultOpen = true,
}: {
  title?: string;
  children: ReactNode;
  collapsible?: boolean;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const shown = !collapsible || !title || open;
  return (
    <section className={cx("section", collapsible && !open && "section-collapsed")}>
      {title ? (
        collapsible ? (
          <h2 className="section-title">
            <button
              type="button"
              className="section-toggle"
              aria-expanded={open}
              onClick={() => setOpen((current) => !current)}
            >
              <span className={open ? "section-mark section-mark-open" : "section-mark"} aria-hidden="true" />
              {title}
            </button>
          </h2>
        ) : (
          <h2 className="section-title">{title}</h2>
        )
      ) : null}
      {shown ? children : null}
    </section>
  );
}
