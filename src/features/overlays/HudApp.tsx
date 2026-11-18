import { useEffect, useState } from "react";
import { api } from "../../api";
import { useAppEvent } from "../../api/events";
import { IDLE_STATUS } from "../../app/context";
import { I18nProvider, useT } from "../../i18n";
import type { RunStatus } from "../../types/runtime";
import { statusDetails, statusHeadline } from "../run/statusText";
import { useOverlayLanguage } from "./useOverlayLanguage";

/** Small window above the tray for the whole run while the main window is hidden or minimized. */
export function HudApp() {
  const language = useOverlayLanguage();
  return (
    <I18nProvider language={language}>
      <Hud />
    </I18nProvider>
  );
}

function Hud() {
  const translator = useT();
  const [status, setStatus] = useState<RunStatus>(IDLE_STATUS);

  useEffect(() => {
    api
      .runStatus()
      .then(setStatus)
      .catch(() => undefined);
  }, []);
  useAppEvent("run:status", setStatus);

  const name = status.target?.name || status.scenarioName || "";
  const details = [name, ...statusDetails(status, translator)].filter((part) => part !== "");

  return (
    <div className="hud" onClick={() => void api.windowShow().catch(() => undefined)}>
      <span className="badge-text">
        <span className="badge-headline">{statusHeadline(status, translator)}</span>
        <span className="badge-details">{details.join(" · ")}</span>
      </span>
      <button
        type="button"
        className="badge-stop"
        onClick={(event) => {
          event.stopPropagation();
          void api.runStop().catch(() => undefined);
        }}
      >
        {translator.t("overlay.badgeStop")}
      </button>
    </div>
  );
}
