import { useEffect, useState } from "react";
import { api } from "../../api";
import { useAppEvent } from "../../api/events";
import type { Language } from "../../types/scenario";

/** Language for the small overlay windows: follows the settings, live. */
export function useOverlayLanguage(): Language {
  const [language, setLanguage] = useState<Language>("en");

  const load = () => {
    Promise.all([api.settingsGet(), api.appInfo()])
      .then(([settings, info]) => setLanguage(settings.language ?? info.systemLanguage))
      .catch(() => undefined);
  };

  useEffect(load, []);
  useAppEvent("data:changed", ({ kind }) => {
    if (kind === "settings") {
      load();
    }
  });

  return language;
}
