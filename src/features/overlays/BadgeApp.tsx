import { useEffect, useState } from "react";
import { api } from "../../api";
import { useAppEvent } from "../../api/events";
import { IDLE_STATUS } from "../../app/context";
import { I18nProvider, useT } from "../../i18n";
import type { RunStatus } from "../../types/runtime";
import { statusDetails, statusHeadline } from "../run/statusText";
import { useOverlayLanguage } from "./useOverlayLanguage";

/** Always-on-top badge shown for the whole recording. */
export function BadgeApp() {
  const language = useOverlayLanguage();
  return (
    <I18nProvider language={language}>
      <Badge />
    </I18nProvider>
  );
}

function Badge() {
  const translator = useT();
  const [status, setStatus] = useState<RunStatus>(IDLE_STATUS);

  useEffect(() => {
    api
      .runStatus()
      .then(setStatus)
      .catch(() => undefined);
  }, []);
  useAppEvent("run:status", setStatus);

  const quiet = status.recording?.held === true || status.recording?.paused === true;
  return (
    <div className={quiet ? "badge badge-paused" : "badge"} data-tauri-drag-region>
      <span className="badge-dot" />
      <span className="badge-text" data-tauri-drag-region>
        <span className="badge-headline" data-tauri-drag-region>
          {status.state === "recording" ? statusHeadline(status, translator) : translator.t("status.recording")}
        </span>
        <span className="badge-details" data-tauri-drag-region>
          {statusDetails(status, translator).join(" · ")}
        </span>
      </span>
      <button type="button" className="badge-stop" onClick={() => void api.recordStop().catch(() => undefined)}>
        {translator.t("overlay.badgeStop")}
      </button>
    </div>
  );
}
