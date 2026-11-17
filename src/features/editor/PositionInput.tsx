import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { useAppEvent } from "../../api/events";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { CaptureResult } from "../../types/runtime";
import type { Position, ScenarioMode, WindowTarget } from "../../types/scenario";
import { Button, Modal, NumberInput, Select } from "../../ui";
import { pointFor } from "./defaults";

interface PositionInputProps {
  value: Position;
  onChange: (value: Position) => void;
  mode: ScenarioMode;
  /** False where a concrete point is required. */
  allowCursor: boolean;
  target: WindowTarget;
  disabled?: boolean;
  /** Takes a captured point instead of `onChange`, together with the rest of the capture (the colour). */
  onCapture?: (position: Position, result: CaptureResult) => void;
}

/** Window target as the user typed it, for the "window not found" message. */
export function describeTarget(target: WindowTarget): string {
  return [target.title.trim(), target.process.trim()].filter((part) => part !== "").join(", ");
}

/** A point of a step: "at the cursor" or coordinates, typed or captured with the hotkey. */
export function PositionInput({ value, onChange, mode, allowCursor, target, disabled, onCapture }: PositionInputProps) {
  const { t } = useT();
  const { settings, showError } = useApp();
  const [capturing, setCapturing] = useState(false);
  // The event may arrive before the next render, so the flag also lives in a ref.
  const capturingRef = useRef(false);

  const setCapture = useCallback((active: boolean) => {
    capturingRef.current = active;
    setCapturing(active);
  }, []);

  useEffect(
    () => () => {
      if (capturingRef.current) {
        capturingRef.current = false;
        api.captureCancel().catch(() => undefined);
      }
    },
    [],
  );

  useAppEvent("capture:point", (result) => {
    if (!capturingRef.current) {
      return;
    }
    setCapture(false);
    if (result.error) {
      showError(result.error);
      return;
    }
    let next: Position;
    if (mode === "window") {
      if (result.clientX === null || result.clientY === null) {
        showError({ code: "targetNotFound", params: { target: describeTarget(target) } });
        return;
      }
      next = pointFor(mode, result.clientX, result.clientY);
    } else {
      next = pointFor(mode, result.x, result.y);
    }
    if (onCapture) {
      onCapture(next, result);
    } else {
      onChange(next);
    }
  });

  const startCapture = async () => {
    setCapture(true);
    try {
      await api.captureStart(mode === "window" ? target : null);
    } catch (error) {
      setCapture(false);
      showError(error);
    }
  };

  const cancelCapture = () => {
    if (!capturingRef.current) {
      return;
    }
    setCapture(false);
    api.captureCancel().catch(showError);
  };

  // Coordinates stored for the other mode are shown as they are; the kind changes with the first edit.
  const x = value.kind === "cursor" ? 0 : value.x;
  const y = value.kind === "cursor" ? 0 : value.y;
  const atCursor = allowCursor && value.kind === "cursor";

  return (
    <div className="position-input">
      {allowCursor ? (
        <Select
          value={atCursor ? "cursor" : "point"}
          disabled={disabled}
          options={[
            { value: "cursor", label: t("steps.position.cursor") },
            { value: "point", label: t("steps.position.point") },
          ]}
          onChange={(choice) => onChange(choice === "cursor" ? { kind: "cursor" } : pointFor(mode, x, y))}
        />
      ) : null}
      {atCursor ? null : (
        <>
          <label className="position-input-axis">
            <span className="pause-input-word">{t("steps.position.x")}</span>
            <NumberInput value={x} disabled={disabled} onChange={(next) => onChange(pointFor(mode, next, y))} />
          </label>
          <label className="position-input-axis">
            <span className="pause-input-word">{t("steps.position.y")}</span>
            <NumberInput value={y} disabled={disabled} onChange={(next) => onChange(pointFor(mode, x, next))} />
          </label>
        </>
      )}
      <Button disabled={disabled || capturing} onClick={() => void startCapture()}>
        {t("capture.point")}
      </Button>
      {capturing ? (
        <Modal
          title={t("capture.title")}
          onClose={cancelCapture}
          footer={<Button onClick={cancelCapture}>{t("common.cancel")}</Button>}
        >
          <p className="modal-text">{t("capture.text", { hotkey: settings.hotkeyCapture })}</p>
          {mode === "window" ? <p className="modal-text editor-modal-note">{t("capture.windowNote")}</p> : null}
        </Modal>
      ) : null}
    </div>
  );
}
