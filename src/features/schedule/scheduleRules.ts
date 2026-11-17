// Pure helpers of the schedule page: local date-time strings, defaults and local validation.

import type { TKey } from "../../i18n";
import {
  MAX_INTERVAL_MINUTES,
  MIN_INTERVAL_MINUTES,
  type QueueSummary,
  type RuleWhen,
  type RunTarget,
  type ScenarioSummary,
  type ScheduleRule,
} from "../../types/scenario";

export type RuleKind = RuleWhen["kind"];

export interface Targets {
  scenarios: ScenarioSummary[];
  queues: QueueSummary[];
}

export type RuleField = "target" | "at" | "time" | "days" | "every" | "startAt" | "windowFrom" | "windowTo";

export interface RuleError {
  field: RuleField;
  key: TKey;
}

const DATE_TIME = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])T([01]\d|2[0-3]):[0-5]\d$/;
const TIME = /^([01]\d|2[0-3]):[0-5]\d$/;

const DEFAULT_WEEK_DAYS = [1, 2, 3, 4, 5];
const DEFAULT_INTERVAL_MINUTES = 60;
export const DEFAULT_WINDOW = { from: "09:00", to: "18:00" };

function pad(value: number, length = 2): string {
  return String(value).padStart(length, "0");
}

export function isTime(value: string): boolean {
  return TIME.test(value);
}

/** Local `HH:MM`. */
export function formatTime(date: Date): string {
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** Local `YYYY-MM-DDTHH:MM`, the value format of `datetime-local` inputs. */
export function formatDateTime(date: Date): string {
  const day = `${pad(date.getFullYear(), 4)}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  return `${day}T${formatTime(date)}`;
}

export function nextFullHour(now: Date): Date {
  const next = new Date(now);
  next.setMinutes(0, 0, 0);
  next.setHours(next.getHours() + 1);
  return next;
}

export function newRuleId(): string {
  const random = Math.floor(Math.random() * 0x1000000)
    .toString(16)
    .padStart(6, "0");
  return `r${Date.now().toString(16)}${random}`;
}

export function encodeTarget(target: RunTarget): string {
  return `${target.kind}:${target.id}`;
}

export function decodeTarget(value: string): RunTarget | null {
  const split = value.indexOf(":");
  if (split < 0) {
    return null;
  }
  const kind = value.slice(0, split);
  if (kind !== "scenario" && kind !== "queue") {
    return null;
  }
  return { kind, id: value.slice(split + 1) };
}

export function targetExists(target: RunTarget, targets: Targets): boolean {
  switch (target.kind) {
    case "scenario":
      return targets.scenarios.some((scenario) => scenario.id === target.id);
    case "queue":
      return targets.queues.some((queue) => queue.id === target.id);
    default: {
      const unreachable: never = target.kind;
      throw new Error(`Unknown target kind: ${String(unreachable)}`);
    }
  }
}

/** Target of a new rule: the first scenario, or the first queue when there are no scenarios. */
export function firstTarget(targets: Targets): RunTarget | null {
  if (targets.scenarios.length > 0) {
    return { kind: "scenario", id: targets.scenarios[0].id };
  }
  if (targets.queues.length > 0) {
    return { kind: "queue", id: targets.queues[0].id };
  }
  return null;
}

export function defaultRule(target: RunTarget, now: Date): ScheduleRule {
  return {
    id: newRuleId(),
    enabled: true,
    target,
    when: { kind: "daily", time: formatTime(nextFullHour(now)) },
  };
}

/** Time of day of a rule, when it has a valid one. */
function timeOf(when: RuleWhen): string | null {
  switch (when.kind) {
    case "once":
      return DATE_TIME.test(when.at) ? when.at.slice(11) : null;
    case "daily":
    case "weekly":
      return isTime(when.time) ? when.time : null;
    case "interval":
      return null;
    default: {
      const unreachable: never = when;
      throw new Error(`Unknown rule kind: ${JSON.stringify(unreachable)}`);
    }
  }
}

/** The next moment with this time of day: today, or tomorrow when it has already passed. */
function nextMomentAt(time: string, now: Date): Date {
  const moment = new Date(now);
  moment.setHours(Number(time.slice(0, 2)), Number(time.slice(3, 5)), 0, 0);
  if (moment.getTime() <= now.getTime()) {
    moment.setDate(moment.getDate() + 1);
  }
  return moment;
}

/** Fields of another kind, keeping the time of day where both kinds have one. */
export function changeKind(when: RuleWhen, kind: RuleKind, now: Date): RuleWhen {
  if (when.kind === kind) {
    return when;
  }
  const time = timeOf(when) ?? formatTime(nextFullHour(now));
  switch (kind) {
    case "once":
      return { kind, at: formatDateTime(nextMomentAt(time, now)) };
    case "daily":
      return { kind, time };
    case "weekly":
      return { kind, days: [...DEFAULT_WEEK_DAYS], time };
    case "interval":
      return { kind, everyMinutes: DEFAULT_INTERVAL_MINUTES, startAt: formatDateTime(now) };
    default: {
      const unreachable: never = kind;
      throw new Error(`Unknown rule kind: ${String(unreachable)}`);
    }
  }
}

/** Local checks of one rule. `targets` is null while the lists are still loading. */
export function ruleErrors(rule: ScheduleRule, targets: Targets | null): RuleError[] {
  const errors: RuleError[] = [];
  if (targets && !targetExists(rule.target, targets)) {
    errors.push({ field: "target", key: "schedule.errTargetMissing" });
  }
  const when = rule.when;
  switch (when.kind) {
    case "once":
      if (!DATE_TIME.test(when.at)) {
        errors.push({ field: "at", key: "schedule.errDateTime" });
      }
      break;
    case "daily":
      if (!isTime(when.time)) {
        errors.push({ field: "time", key: "schedule.errTime" });
      }
      break;
    case "weekly":
      if (when.days.length === 0) {
        errors.push({ field: "days", key: "schedule.errDays" });
      }
      if (!isTime(when.time)) {
        errors.push({ field: "time", key: "schedule.errTime" });
      }
      break;
    case "interval":
      if (
        !Number.isInteger(when.everyMinutes) ||
        when.everyMinutes < MIN_INTERVAL_MINUTES ||
        when.everyMinutes > MAX_INTERVAL_MINUTES
      ) {
        errors.push({ field: "every", key: "schedule.errInterval" });
      }
      if (!DATE_TIME.test(when.startAt)) {
        errors.push({ field: "startAt", key: "schedule.errDateTime" });
      }
      if (when.window) {
        if (!isTime(when.window.from)) {
          errors.push({ field: "windowFrom", key: "schedule.errTime" });
        }
        if (!isTime(when.window.to)) {
          errors.push({ field: "windowTo", key: "schedule.errTime" });
        }
      }
      break;
    default: {
      const unreachable: never = when;
      throw new Error(`Unknown rule kind: ${JSON.stringify(unreachable)}`);
    }
  }
  return errors;
}
