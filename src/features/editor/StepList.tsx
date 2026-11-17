import { memo, useCallback, useEffect, useRef, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { useT } from "../../i18n";
import type { Issue } from "../../types/runtime";
import { MAX_DEPTH, MAX_STEPS, type Step } from "../../types/scenario";
import { cx } from "../../ui";
import { isBlockType, isRepeatable, isStepType, STEP_TYPES } from "./defaults";
import { NO_ISSUES, type IssueMap, type StepActions } from "./editorContext";
import { ArrowDownIcon, ArrowUpIcon, CopyIcon, TrashIcon } from "./icons";
import { StepForm } from "./StepForm";
import { STEP_TYPE_KEYS, stepSummary } from "./summary";
import { containsStep, isBlock, type ListSlot } from "./tree";

interface StepListProps {
  steps: Step[];
  /** Block that owns the list, null for the top level. */
  parentId: string | null;
  slot: ListSlot;
  /** Label of the owning block with a trailing dot, empty for the top level. */
  prefix: string;
  /** Number of blocks around the list. */
  depth: number;
  /** Selected step, when it is somewhere inside this list. */
  selectedId: string | null;
  issues: IssueMap;
  disabled: boolean;
  /** True when the scenario has reached the step limit. */
  full: boolean;
  actions: StepActions;
}

const EMPTY_ISSUES: IssueMap = new Map();

function stopClick(event: MouseEvent) {
  event.stopPropagation();
}

// The list and the row render each other, so both spell out their return type
// to keep the compiler from inferring one through the other.

/** One list of the step tree with its own "Add step" control. */
export const StepList = memo(function StepList({
  steps,
  parentId,
  slot,
  prefix,
  depth,
  selectedId,
  issues,
  disabled,
  full,
  actions,
}: StepListProps): ReactNode {
  return (
    <div className="step-list">
      {steps.length > 0 ? (
        <ol className="step-list-items">
          {steps.map((step, index) => {
            const block = isBlock(step);
            const selected = step.id === selectedId;
            return (
              <StepRow
                key={step.id}
                step={step}
                label={`${prefix}${index + 1}`}
                depth={depth}
                selected={selected}
                selectedInside={
                  block && !selected && selectedId !== null && containsStep(step, selectedId) ? selectedId : null
                }
                ownIssues={issues.get(step.id) ?? NO_ISSUES}
                issues={block ? issues : null}
                first={index === 0}
                last={index === steps.length - 1}
                disabled={disabled}
                full={full}
                actions={actions}
              />
            );
          })}
        </ol>
      ) : null}
      <AddStep parentId={parentId} slot={slot} depth={depth} disabled={disabled} full={full} actions={actions} />
    </div>
  );
});

interface AddStepProps {
  parentId: string | null;
  slot: ListSlot;
  depth: number;
  disabled: boolean;
  full: boolean;
  actions: StepActions;
}

function AddStep({ parentId, slot, depth, disabled, full, actions }: AddStepProps) {
  const { t } = useT();
  const noBlocks = depth >= MAX_DEPTH;
  return (
    <div className="step-add">
      <select
        className="input select step-add-select"
        value=""
        disabled={disabled || full}
        title={full ? t("errors.stepLimit", { max: MAX_STEPS }) : undefined}
        aria-label={t("editor.addStep")}
        onChange={(event) => {
          const type = event.target.value;
          if (isStepType(type)) {
            actions.add({ parentId, slot }, type);
          }
        }}
      >
        <option value="">{t("editor.addStep")}</option>
        {STEP_TYPES.map((type) => (
          <option key={type} value={type} disabled={noBlocks && isBlockType(type)}>
            {t(STEP_TYPE_KEYS[type])}
          </option>
        ))}
      </select>
      {noBlocks ? <span className="step-add-note">{t("editor.limitDepth", { max: MAX_DEPTH })}</span> : null}
    </div>
  );
}

interface StepRowProps {
  step: Step;
  /** 1-based position, nested ones as "4.2.1". */
  label: string;
  depth: number;
  selected: boolean;
  /** Selected step, when it is nested in this block. */
  selectedInside: string | null;
  ownIssues: ReadonlyArray<Issue>;
  /** Issues of the whole scenario for the nested lists; null for plain steps so they stay memoized. */
  issues: IssueMap | null;
  first: boolean;
  last: boolean;
  disabled: boolean;
  full: boolean;
  actions: StepActions;
}

const StepRow = memo(function StepRow({
  step,
  label,
  depth,
  selected,
  selectedInside,
  ownIssues,
  issues,
  first,
  last,
  disabled,
  full,
  actions,
}: StepRowProps): ReactNode {
  const translator = useT();
  const { t, td } = translator;
  const row = useRef<HTMLDivElement>(null);
  const id = step.id;

  // A step added to the end of a long list may be off screen.
  useEffect(() => {
    if (selected) {
      row.current?.scrollIntoView({ block: "nearest" });
    }
  }, [selected]);

  const onChange = useCallback((next: Step) => actions.update(id, () => next), [actions, id]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) {
      event.preventDefault();
      actions.toggle(id);
    }
  };

  const hasError = ownIssues.some((issue) => issue.severity === "error");
  const hasWarning = ownIssues.length > 0 && !hasError;
  const repeat = isRepeatable(step) ? step.repeat : undefined;
  const nested = issues ?? EMPTY_ISSUES;
  const nestedList = (steps: Step[], slot: ListSlot) => (
    <StepList
      steps={steps}
      parentId={id}
      slot={slot}
      prefix={`${label}.`}
      depth={depth + 1}
      selectedId={selectedInside}
      issues={nested}
      disabled={disabled}
      full={full}
      actions={actions}
    />
  );

  return (
    <li className={cx("step", hasError && "step-error", hasWarning && "step-warning")}>
      <div
        ref={row}
        className={cx("step-row", selected && "step-row-selected", !step.enabled && "step-row-off")}
        role="button"
        tabIndex={0}
        aria-expanded={selected}
        onClick={() => actions.toggle(id)}
        onKeyDown={onKeyDown}
      >
        <span className="step-label">{label}</span>
        <input
          type="checkbox"
          className="step-enabled"
          checked={step.enabled}
          disabled={disabled}
          title={t("editor.stepEnabled")}
          aria-label={t("editor.stepEnabled")}
          onClick={stopClick}
          onChange={(event) => {
            const enabled = event.target.checked;
            actions.update(id, (current) => ({ ...current, enabled }));
          }}
        />
        <span className="step-summary">{stepSummary(step, translator)}</span>
        {repeat ? (
          <span className="step-badge" title={t("editor.repeatBadge", { times: repeat.times })}>
            ×{repeat.times}
          </span>
        ) : null}
        {hasError || hasWarning ? (
          <span
            className="step-marker"
            role="img"
            aria-label={hasError ? t("editor.stepError") : t("editor.stepWarning")}
            title={hasError ? t("editor.stepError") : t("editor.stepWarning")}
          >
            !
          </span>
        ) : null}
        <span className="step-actions" onClick={stopClick}>
          <button
            type="button"
            className="step-action"
            disabled={disabled || first}
            title={t("common.moveUp")}
            aria-label={t("common.moveUp")}
            onClick={() => actions.move(id, -1)}
          >
            <ArrowUpIcon />
          </button>
          <button
            type="button"
            className="step-action"
            disabled={disabled || last}
            title={t("common.moveDown")}
            aria-label={t("common.moveDown")}
            onClick={() => actions.move(id, 1)}
          >
            <ArrowDownIcon />
          </button>
          <button
            type="button"
            className="step-action"
            disabled={disabled || full}
            title={t("common.duplicate")}
            aria-label={t("common.duplicate")}
            onClick={() => actions.duplicate(id)}
          >
            <CopyIcon />
          </button>
          <button
            type="button"
            className="step-action step-action-danger"
            disabled={disabled}
            title={t("common.delete")}
            aria-label={t("common.delete")}
            onClick={() => actions.remove(id)}
          >
            <TrashIcon />
          </button>
        </span>
      </div>
      {ownIssues.length > 0 ? (
        <ul className="step-issues">
          {ownIssues.map((issue) => (
            <li key={issue.code} className={cx("issue", `issue-${issue.severity}`)}>
              {td(`issues.${issue.code}`)}
            </li>
          ))}
        </ul>
      ) : null}
      {selected ? <StepForm step={step} onChange={onChange} disabled={disabled} /> : null}
      {step.type === "if" ? (
        <div className="step-branches">
          <div className="step-branch">
            <div className="step-branch-caption">{t("editor.branchYes")}</div>
            {nestedList(step.thenSteps, "then")}
          </div>
          <div className="step-branch">
            <div className="step-branch-caption">{t("editor.branchNo")}</div>
            {nestedList(step.elseSteps, "else")}
          </div>
        </div>
      ) : null}
      {step.type === "loop" ? (
        <div className="step-branches">
          <div className="step-branch">{nestedList(step.steps, "body")}</div>
        </div>
      ) : null}
    </li>
  );
});
