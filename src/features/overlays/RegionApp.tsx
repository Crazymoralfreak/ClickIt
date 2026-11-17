import { useEffect, useState, type MouseEvent } from "react";
import { api } from "../../api";
import { I18nProvider, useT } from "../../i18n";
import { useOverlayLanguage } from "./useOverlayLanguage";

interface Drag {
  startX: number;
  startY: number;
  x: number;
  y: number;
}

/** Full-screen transparent overlay for selecting a rectangle. The real pixel
 *  coordinates are read by the core from the cursor position, not from here. */
export function RegionApp() {
  const language = useOverlayLanguage();
  return (
    <I18nProvider language={language}>
      <Region />
    </I18nProvider>
  );
}

function Region() {
  const { t } = useT();
  const [drag, setDrag] = useState<Drag | null>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void api.regionCancel().catch(() => undefined);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const begin = (event: MouseEvent) => {
    if (event.button !== 0) {
      void api.regionCancel().catch(() => undefined);
      return;
    }
    setDrag({ startX: event.clientX, startY: event.clientY, x: event.clientX, y: event.clientY });
    void api.regionMark().catch(() => undefined);
  };

  const move = (event: MouseEvent) => {
    setDrag((current) => (current ? { ...current, x: event.clientX, y: event.clientY } : current));
  };

  const end = (event: MouseEvent) => {
    if (event.button !== 0 || !drag) {
      return;
    }
    setDrag(null);
    void api.regionFinish().catch(() => undefined);
  };

  const box = drag
    ? {
        left: Math.min(drag.startX, drag.x),
        top: Math.min(drag.startY, drag.y),
        width: Math.abs(drag.x - drag.startX),
        height: Math.abs(drag.y - drag.startY),
      }
    : null;

  return (
    <div className="region" onMouseDown={begin} onMouseMove={move} onMouseUp={end}>
      {box ? <div className="region-box" style={box} /> : <div className="region-hint">{t("overlay.regionHint")}</div>}
    </div>
  );
}
