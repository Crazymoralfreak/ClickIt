import { useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { useAppEvent } from "../../api/events";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { Rect, RegionPurpose } from "../../types/runtime";
import type { Area, Condition, CoordSpace, ScenarioMode, WindowTarget } from "../../types/scenario";
import { Button, Field, NumberInput, Select, type Option } from "../../ui";
import { describeTarget } from "./PositionInput";

export type ImageCondition = Extract<Condition, { kind: "imageFound" | "imageGone" }>;

interface ImageConditionFieldsProps {
  value: ImageCondition;
  onChange: (value: ImageCondition) => void;
  mode: ScenarioMode;
  target: WindowTarget;
  scenarioId: string;
  disabled?: boolean;
}

type Preview = { state: "none" | "loading" | "failed" } | { state: "ready"; url: string };

function useTemplatePreview(scenarioId: string, file: string): Preview {
  const [preview, setPreview] = useState<Preview>({ state: "none" });

  useEffect(() => {
    if (file === "") {
      setPreview({ state: "none" });
      return;
    }
    let cancelled = false;
    setPreview({ state: "loading" });
    api
      .templatePreview(scenarioId, file)
      .then((url) => {
        if (!cancelled) {
          setPreview({ state: "ready", url });
        }
      })
      .catch(() => {
        if (!cancelled) {
          setPreview({ state: "failed" });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [scenarioId, file]);

  return preview;
}

/** Template, search area and similarity of the "image found" and "image gone" conditions. */
export function ImageConditionFields({
  value,
  onChange,
  mode,
  target,
  scenarioId,
  disabled,
}: ImageConditionFieldsProps) {
  const { t } = useT();
  const { showError } = useApp();
  const preview = useTemplatePreview(scenarioId, value.template);
  // Purpose of the selection started here; other editors ignore the result.
  const pending = useRef<RegionPurpose | null>(null);

  const space: CoordSpace = mode === "window" ? "client" : "absolute";
  const area = value.area;

  const startRegion = async (purpose: RegionPurpose) => {
    pending.current = purpose;
    try {
      await api.regionStart(scenarioId, purpose, mode === "window" ? target : null);
    } catch (error) {
      pending.current = null;
      showError(error);
    }
  };

  const applyRect = (rect: Rect, rectSpace: CoordSpace) => {
    onChange({
      ...value,
      area: { kind: "rect", space: rectSpace, x: rect.x, y: rect.y, width: rect.width, height: rect.height },
    });
  };

  useAppEvent("region:done", (result) => {
    if (pending.current === null || pending.current !== result.purpose) {
      return;
    }
    pending.current = null;
    if (result.error) {
      showError(result.error);
      return;
    }
    switch (result.purpose) {
      case "template":
        if (result.template) {
          onChange({ ...value, template: result.template.file });
        }
        break;
      case "area":
        if (mode !== "window") {
          applyRect(result.rect, "absolute");
        } else if (result.clientRect) {
          applyRect(result.clientRect, "client");
        } else {
          showError({ code: "targetNotFound", params: { target: describeTarget(target) } });
        }
        break;
      default: {
        const unreachable: never = result.purpose;
        throw new Error(`Unknown region purpose: ${String(unreachable)}`);
      }
    }
  });

  useAppEvent("region:cancelled", () => {
    pending.current = null;
  });

  const areaOptions: Array<Option<Area["kind"]>> = [{ value: "screen", label: t("conditions.areaScreen") }];
  // An area kept from the "Window" mode stays visible so the user can see what the validation complains about.
  if (mode === "window" || area.kind === "client") {
    areaOptions.push({ value: "client", label: t("conditions.areaClient") });
  }
  areaOptions.push({ value: "rect", label: t("conditions.areaRect") });

  const setAreaKind = (kind: Area["kind"]) => {
    if (kind === area.kind) {
      return;
    }
    switch (kind) {
      case "screen":
        onChange({ ...value, area: { kind: "screen" } });
        break;
      case "client":
        onChange({ ...value, area: { kind: "client" } });
        break;
      case "rect":
        onChange({ ...value, area: { kind: "rect", space, x: 0, y: 0, width: 0, height: 0 } });
        break;
      default: {
        const unreachable: never = kind;
        throw new Error(`Unknown area kind: ${String(unreachable)}`);
      }
    }
  };

  // A rectangle stored for the other mode keeps its space until the user edits or selects it again.
  const patchRect = (patch: Partial<Rect>) => {
    if (area.kind === "rect") {
      onChange({ ...value, area: { ...area, ...patch, space } });
    }
  };

  return (
    <>
      <Field label={t("conditions.template")} hint={t("conditions.templateHint")}>
        <span className="template-preview">
          {preview.state === "ready" ? (
            <img src={preview.url} alt={t("conditions.templateAlt")} />
          ) : (
            <span className="template-preview-empty">
              {preview.state === "failed"
                ? t("conditions.templateFailed")
                : preview.state === "loading"
                  ? t("common.loading")
                  : t("conditions.templateNone")}
            </span>
          )}
        </span>
        <Button disabled={disabled} onClick={() => void startRegion("template")}>
          {t("conditions.captureTemplate")}
        </Button>
      </Field>
      <Field label={t("conditions.area")} hint={t("conditions.areaHint")}>
        <Select value={area.kind} options={areaOptions} disabled={disabled} onChange={setAreaKind} />
        {area.kind === "rect" ? (
          <>
            <label className="position-input-axis">
              <span className="pause-input-word">{t("conditions.rectX")}</span>
              <NumberInput value={area.x} disabled={disabled} onChange={(x) => patchRect({ x })} />
            </label>
            <label className="position-input-axis">
              <span className="pause-input-word">{t("conditions.rectY")}</span>
              <NumberInput value={area.y} disabled={disabled} onChange={(y) => patchRect({ y })} />
            </label>
            <label className="position-input-axis">
              <span className="pause-input-word">{t("conditions.rectWidth")}</span>
              <NumberInput value={area.width} min={1} disabled={disabled} onChange={(width) => patchRect({ width })} />
            </label>
            <label className="position-input-axis">
              <span className="pause-input-word">{t("conditions.rectHeight")}</span>
              <NumberInput
                value={area.height}
                min={1}
                disabled={disabled}
                onChange={(height) => patchRect({ height })}
              />
            </label>
            <Button disabled={disabled} onClick={() => void startRegion("area")}>
              {t("conditions.selectArea")}
            </Button>
          </>
        ) : null}
      </Field>
      <Field label={t("conditions.threshold")} hint={t("conditions.thresholdHint")}>
        <NumberInput
          value={Math.round(value.threshold * 100)}
          min={50}
          max={100}
          suffix="%"
          disabled={disabled}
          onChange={(percent) => onChange({ ...value, threshold: percent / 100 })}
        />
      </Field>
    </>
  );
}
