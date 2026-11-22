// Events emitted by the Rust core.

import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import type {
  AgentEntry,
  ChatInfo,
  HarnessAsk,
  HarnessDialogEvent,
  CaptureResult,
  DataKind,
  RecordFinished,
  RegionResult,
  RunFinished,
  RunStatus,
  RunTrace,
  UpdateStatus,
} from "../types/runtime";

export interface AppEvents {
  /** Every state or position change of a run or a recording (throttled). */
  "run:status": RunStatus;
  "run:finished": RunFinished;
  /** Where a canvas scenario is right now and which links the pass took. */
  "run:trace": RunTrace;
  "record:finished": RecordFinished;
  "capture:point": CaptureResult;
  "region:done": RegionResult;
  "region:cancelled": null;
  /** Data changed outside the main window: recording, import, tray. */
  "data:changed": { kind: DataKind };
  "journal:changed": null;
  /** Every step of a check for updates and of the installation of one. */
  "update:status": UpdateStatus;
  /** A harness saved this scenario. The open editor drops its draft and reads the file. */
  "harness:scenario": { id: string };
  /** ClickIt is asking whether a harness may start this scenario or save new commands into it. */
  "harness:confirm": HarnessAsk;
  /** A new line of a chat with the model. */
  "agent:entry": { chatId: string; entry: AgentEntry };
  /** The model finished its answer in this chat. */
  "agent:idle": { chatId: string };
  /** A chat got its title from its first message. */
  "agent:chat": ChatInfo;
  /** A line from an external harness connection, or a change of that connection. */
  "harness:dialog": HarnessDialogEvent;
}

export type AppEventName = keyof AppEvents;

/** Subscribes for the lifetime of the component; the handler may change between renders. */
export function useAppEvent<K extends AppEventName>(name: K, handler: (payload: AppEvents[K]) => void): void {
  const latest = useRef(handler);
  latest.current = handler;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<AppEvents[K]>(name, (event) => latest.current(event.payload)).then((stop) => {
      if (cancelled) {
        stop();
      } else {
        unlisten = stop;
      }
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [name]);
}
