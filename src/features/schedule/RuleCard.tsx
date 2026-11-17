import { useT, type TKey } from "../../i18n";
import {
  MAX_INTERVAL_MINUTES,
  MIN_INTERVAL_MINUTES,
  type DayWindow,
  type RuleWhen,
  type ScheduleRule,
} from "../../types/scenario";
import { Button, Checkbox, Field, NumberInput, Select, cx, type Option } from "../../ui";
import {
  DEFAULT_WINDOW,
  changeKind,
  decodeTarget,
  encodeTarget,
  isTime,
  type RuleError,
  type RuleField,
  type RuleKind,
} from "./scheduleRules";

const MAX_DATE_TIME = "9999-12-31T23:59";

/** 1 = Monday .. 7 = Sunday, as the core counts them. */
const DAYS: ReadonlyArray<{ day: number; label: TKey }> = [
  { day: 1, label: "schedule.days.mon" },
  { day: 2, label: "schedule.days.tue" },
  { day: 3, label: "schedule.days.wed" },
  { day: 4, label: "schedule.days.thu" },
  { day: 5, label: "schedule.days.fri" },
  { day: 6, label: "schedule.days.sat" },
  { day: 7, label: "schedule.days.sun" },
];

interface MomentInputProps {
  type: "time" | "datetime-local";
  value: string;
  invalid: boolean;
  label: string;
  onChange: (value: string) => void;
}

/** Native time or date-time field; an unfinished entry arrives as an empty string. */
function MomentInput({ type, value, invalid, label, onChange }: MomentInputProps) {
  return (
    <input
      type={type}
      className={cx("input", invalid && "input-invalid")}
      value={value}
      max={type === "datetime-local" ? MAX_DATE_TIME : undefined}
      aria-label={label}
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

interface DayWindowInputProps {
  value: DayWindow;
  invalidFrom: boolean;
  invalidTo: boolean;
  onChange: (value: DayWindow) => void;
}

function DayWindowInput({ value, invalidFrom, invalidTo, onChange }: DayWindowInputProps) {
  const { t } = useT();
  return (
    <>
      <span className="schedule-word">{t("common.from")}</span>
      <MomentInput
        type="time"
        value={value.from}
        invalid={invalidFrom}
        label={t("schedule.windowFrom")}
        onChange={(from) => onChange({ ...value, from })}
      />
      <span className="schedule-word">{t("common.to")}</span>
      <MomentInput
        type="time"
        value={value.to}
        invalid={invalidTo}
        label={t("schedule.windowTo")}
        onChange={(to) => onChange({ ...value, to })}
      />
    </>
  );
}

interface RuleCardProps {
  rule: ScheduleRule;
  /** Scenarios and queues as options; null while the lists are loading. */
  targetOptions: ReadonlyArray<Option<string>> | null;
  errors: RuleError[];
  onChange: (rule: ScheduleRule) => void;
  onDelete: () => void;
}

export function RuleCard({ rule, targetOptions, errors, onChange, onDelete }: RuleCardProps) {
  const { t } = useT();
  const when = rule.when;

  const invalid = (field: RuleField) => errors.some((error) => error.field === field);
  const errorText = (...fields: RuleField[]): string | undefined => {
    const keys = new Set(errors.filter((error) => fields.includes(error.field)).map((error) => error.key));
    if (keys.size === 0) {
      return undefined;
    }
    const limits = { min: MIN_INTERVAL_MINUTES, max: MAX_INTERVAL_MINUTES };
    return Array.from(keys, (key) => t(key, limits)).join(" ");
  };
  const setWhen = (next: RuleWhen) => onChange({ ...rule, when: next });

  const targetValue = encodeTarget(rule.target);
  let options: ReadonlyArray<Option<string>>;
  if (targetOptions === null) {
    options = [{ value: targetValue, label: t("common.loading"), disabled: true }];
  } else if (invalid("target")) {
    // The deleted target has no option of its own: a disabled placeholder stands in for it.
    options = [{ value: targetValue, label: t("schedule.targetDeleted"), disabled: true }, ...targetOptions];
  } else {
    options = targetOptions;
  }

  const kindOptions: Option<RuleKind>[] = [
    { value: "once", label: t("schedule.kindOnce") },
    { value: "daily", label: t("schedule.kindDaily") },
    { value: "weekly", label: t("schedule.kindWeekly") },
    { value: "interval", label: t("schedule.kindInterval") },
  ];

  const kindFields = () => {
    switch (when.kind) {
      case "once":
        return (
          <MomentInput
            type="datetime-local"
            value={when.at}
            invalid={invalid("at")}
            label={t("schedule.dateTime")}
            onChange={(at) => setWhen({ ...when, at })}
          />
        );
      case "daily":
        return (
          <>
            <span className="schedule-word">{t("schedule.at")}</span>
            <MomentInput
              type="time"
              value={when.time}
              invalid={invalid("time")}
              label={t("schedule.time")}
              onChange={(time) => setWhen({ ...when, time })}
            />
          </>
        );
      case "weekly":
        return (
          <>
            <span className={cx("schedule-days", invalid("days") && "schedule-days-invalid")} role="group">
              {DAYS.map(({ day, label }) => {
                const on = when.days.includes(day);
                return (
                  <button
                    key={day}
                    type="button"
                    aria-pressed={on}
                    className={cx("schedule-day", on && "schedule-day-on")}
                    onClick={() =>
                      setWhen({
                        ...when,
                        days: on
                          ? when.days.filter((other) => other !== day)
                          : [...when.days, day].sort((a, b) => a - b),
                      })
                    }
                  >
                    {t(label)}
                  </button>
                );
              })}
            </span>
            <span className="schedule-word">{t("schedule.at")}</span>
            <MomentInput
              type="time"
              value={when.time}
              invalid={invalid("time")}
              label={t("schedule.time")}
              onChange={(time) => setWhen({ ...when, time })}
            />
          </>
        );
      case "interval":
        return (
          <>
            <span className="schedule-word">{t("schedule.every")}</span>
            <NumberInput
              value={when.everyMinutes}
              min={MIN_INTERVAL_MINUTES}
              max={MAX_INTERVAL_MINUTES}
              invalid={invalid("every")}
              suffix={t("common.min")}
              onChange={(everyMinutes) => setWhen({ ...when, everyMinutes })}
            />
          </>
        );
      default: {
        const unreachable: never = when;
        throw new Error(`Unknown rule kind: ${JSON.stringify(unreachable)}`);
      }
    }
  };

  const wraps =
    when.kind === "interval" &&
    when.window !== undefined &&
    isTime(when.window.from) &&
    isTime(when.window.to) &&
    when.window.from > when.window.to;

  return (
    <div className={cx("schedule-rule", errors.length > 0 && "schedule-rule-invalid")}>
      <div className="schedule-rule-head">
        <Checkbox
          checked={rule.enabled}
          label={t("schedule.enabled")}
          onChange={(enabled) => onChange({ ...rule, enabled })}
        />
        <Button small variant="ghost" onClick={onDelete}>
          {t("schedule.deleteRule")}
        </Button>
      </div>
      <Field label={t("schedule.target")} error={errorText("target")}>
        <Select
          value={targetValue}
          options={options}
          disabled={targetOptions === null}
          invalid={invalid("target")}
          onChange={(value) => {
            const target = decodeTarget(value);
            if (target) {
              onChange({ ...rule, target });
            }
          }}
        />
      </Field>
      <Field label={t("schedule.when")} error={errorText("at", "time", "days", "every")}>
        <Select
          value={when.kind}
          options={kindOptions}
          onChange={(kind) => setWhen(changeKind(when, kind, new Date()))}
        />
        {kindFields()}
      </Field>
      {when.kind === "interval" ? (
        <>
          <Field label={t("schedule.startAt")} error={errorText("startAt")}>
            <MomentInput
              type="datetime-local"
              value={when.startAt}
              invalid={invalid("startAt")}
              label={t("schedule.startAt")}
              onChange={(startAt) => setWhen({ ...when, startAt })}
            />
          </Field>
          <Field
            label={t("schedule.window")}
            hint={wraps ? t("schedule.windowWraps") : undefined}
            error={errorText("windowFrom", "windowTo")}
          >
            <Checkbox
              checked={when.window !== undefined}
              label={t("schedule.windowEnabled")}
              onChange={(enabled) =>
                setWhen(
                  enabled
                    ? { ...when, window: { ...DEFAULT_WINDOW } }
                    : { kind: "interval", everyMinutes: when.everyMinutes, startAt: when.startAt },
                )
              }
            />
            {when.window !== undefined ? (
              <DayWindowInput
                value={when.window}
                invalidFrom={invalid("windowFrom")}
                invalidTo={invalid("windowTo")}
                onChange={(dayWindow) => setWhen({ ...when, window: dayWindow })}
              />
            ) : null}
          </Field>
        </>
      ) : null}
    </div>
  );
}
