import type { Translator } from "../../i18n";
import type { RunStatus } from "../../types/runtime";

/** Headline of the current state: "Running", "Paused: another window is active", ... */
export function statusHeadline(status: RunStatus, { t }: Translator): string {
  switch (status.state) {
    case "idle":
      return t("status.idle");
    case "countdown":
      return t("status.countdown", { seconds: status.countdownLeft ?? 0 });
    case "running":
      return t("status.running");
    case "waiting":
      return t("status.waiting");
    case "pausedWindow":
      return t("status.pausedWindow");
    case "pausedProcess":
      return t("status.pausedProcess", { process: status.blockedProcess ?? "" });
    case "recording":
      return status.recording?.paused ? t("status.recordingPaused") : t("status.recording");
    default: {
      const unreachable: never = status.state;
      throw new Error(`Unknown run state: ${String(unreachable)}`);
    }
  }
}

/** Position inside the run: queue item, round, step, loop pass, step repeat. */
export function statusDetails(status: RunStatus, { t }: Translator): string[] {
  if (status.state === "idle" || status.state === "countdown") {
    return [];
  }
  if (status.state === "recording") {
    return status.recording ? [t("status.recordedSteps", { steps: status.recording.steps })] : [];
  }

  const parts: string[] = [];
  const queue = status.queue;
  if (queue) {
    parts.push(
      queue.roundsTotal === null
        ? t("status.queueRoundOpen", { round: queue.round })
        : t("status.queueRound", { round: queue.round, total: queue.roundsTotal }),
    );
    parts.push(t("status.queueItem", { index: queue.itemIndex, count: queue.itemCount }));
    if (status.scenarioName) {
      parts.push(`«${status.scenarioName}»`);
    }
  }
  if (status.round > 0) {
    parts.push(
      status.roundsTotal === null
        ? t("status.roundOpen", { round: status.round })
        : t("status.round", { round: status.round, total: status.roundsTotal }),
    );
  }
  if (status.stepLabel) {
    parts.push(t("status.step", { step: status.stepLabel }));
  }
  if (status.loopPass !== null) {
    parts.push(
      status.loopTotal === null
        ? t("status.loopPassOpen", { pass: status.loopPass })
        : t("status.loopPass", { pass: status.loopPass, total: status.loopTotal }),
    );
  }
  if (status.stepRepeat !== null && status.stepRepeatTotal !== null) {
    parts.push(t("status.stepRepeat", { repeat: status.stepRepeat, total: status.stepRepeatTotal }));
  }
  return parts;
}
