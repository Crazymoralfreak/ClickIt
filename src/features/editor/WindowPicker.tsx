import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import type { WindowInfo } from "../../types/runtime";
import { Button, Modal } from "../../ui";

interface WindowPickerProps {
  onPick: (window: WindowInfo) => void;
  onClose: () => void;
}

/** List of the open windows; choosing one fills the window target. */
export function WindowPicker({ onPick, onClose }: WindowPickerProps) {
  const { t } = useT();
  const { showError } = useApp();
  const [windows, setWindows] = useState<WindowInfo[] | null>(null);

  const load = useCallback(() => {
    setWindows(null);
    api
      .windowsList()
      .then(setWindows)
      .catch((error: unknown) => {
        setWindows([]);
        showError(error);
      });
  }, [showError]);

  useEffect(load, [load]);

  return (
    <Modal
      title={t("editor.pickWindowTitle")}
      wide
      onClose={onClose}
      footer={
        <>
          <Button onClick={load}>{t("editor.pickWindowRefresh")}</Button>
          <Button onClick={onClose}>{t("common.close")}</Button>
        </>
      }
    >
      {windows === null ? (
        <p className="modal-text">{t("common.loading")}</p>
      ) : windows.length === 0 ? (
        <p className="modal-text">{t("editor.pickWindowEmpty")}</p>
      ) : (
        <ul className="window-list">
          {windows.map((item, index) => (
            <li key={`${index}:${item.process}:${item.title}`}>
              <button type="button" className="window-item" onClick={() => onPick(item)}>
                <span className="window-item-title">
                  {item.title === "" ? t("editor.pickWindowNoTitle") : item.title}
                </span>
                <span className="window-item-process">{item.process}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </Modal>
  );
}
