//! Scenario and queue checks shared by the editor and by every start path.
//! A scenario with at least one `Error` issue does not start.

use serde::Serialize;

use super::queue::Queue;
use super::scenario::{
    Area, Condition, CoordSpace, LoopMode, OnFound, Pause, Position, RepeatMode, Scenario, ScenarioMode, Step,
    StepKind, MAX_DEPTH, MAX_STEPS, MAX_STEP_REPEAT,
};
use crate::input::keys;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Error,
    Warning,
}

/// `code` is a key under `issues.*` in the dictionaries. `step_id` is `None`
/// for problems of the scenario or queue as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub step_id: Option<String>,
    pub code: String,
    pub severity: Severity,
}

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|issue| issue.severity == Severity::Error)
}

struct Checker<'a> {
    mode: ScenarioMode,
    template_exists: &'a dyn Fn(&str) -> bool,
    issues: Vec<Issue>,
}

impl Checker<'_> {
    fn error(&mut self, step: Option<&Step>, code: &str) {
        self.push(step, code, Severity::Error);
    }

    fn push(&mut self, step: Option<&Step>, code: &str, severity: Severity) {
        let issue = Issue { step_id: step.map(|s| s.id.clone()), code: code.to_string(), severity };
        if !self.issues.contains(&issue) {
            self.issues.push(issue);
        }
    }

    fn pause(&mut self, step: Option<&Step>, pause: &Pause) {
        if pause.from_ms < 0 || pause.to_ms < 0 {
            self.error(step, "pauseNegative");
        } else if pause.from_ms > pause.to_ms {
            self.error(step, "pauseOrder");
        }
    }

    fn space_matches(&self, space: CoordSpace) -> bool {
        match (self.mode, space) {
            (ScenarioMode::Screen, CoordSpace::Absolute) | (ScenarioMode::Window, CoordSpace::Client) => true,
            (ScenarioMode::Screen, CoordSpace::Client) | (ScenarioMode::Window, CoordSpace::Absolute) => false,
        }
    }

    /// `allow_cursor` is false where a concrete point is required.
    fn position(&mut self, step: &Step, position: &Position, allow_cursor: bool) {
        match position {
            Position::Cursor => {
                if !allow_cursor {
                    self.error(Some(step), "positionRequired");
                }
            }
            Position::Absolute { .. } => {
                if !self.space_matches(CoordSpace::Absolute) {
                    self.error(Some(step), "positionWrongMode");
                }
            }
            Position::Client { x, y } => {
                if !self.space_matches(CoordSpace::Client) {
                    self.error(Some(step), "positionWrongMode");
                } else if *x < 0 || *y < 0 {
                    self.error(Some(step), "positionNegative");
                }
            }
        }
    }

    fn area(&mut self, step: &Step, area: &Area) {
        match area {
            Area::Screen => {}
            Area::Client => {
                if self.mode != ScenarioMode::Window {
                    self.error(Some(step), "areaWrongMode");
                }
            }
            Area::Rect { space, width, height, .. } => {
                if !self.space_matches(*space) {
                    self.error(Some(step), "areaWrongMode");
                }
                if *width <= 0 || *height <= 0 {
                    self.error(Some(step), "areaEmpty");
                }
            }
        }
    }

    fn condition(&mut self, step: &Step, condition: &Condition) {
        match condition {
            Condition::Pixel { point, color, tolerance } => {
                self.position(step, point, false);
                if parse_color(color).is_none() {
                    self.error(Some(step), "colorInvalid");
                }
                if !(0..=255).contains(tolerance) {
                    self.error(Some(step), "toleranceRange");
                }
            }
            Condition::ImageFound { template, area, threshold }
            | Condition::ImageGone { template, area, threshold } => {
                if template.is_empty() {
                    self.error(Some(step), "templateMissing");
                } else if !(self.template_exists)(template) {
                    self.error(Some(step), "templateFileMissing");
                }
                self.area(step, area);
                if !(0.5..=1.0).contains(threshold) {
                    self.error(Some(step), "thresholdRange");
                }
            }
            Condition::Window { title, .. } => {
                if title.trim().is_empty() {
                    self.error(Some(step), "titleEmpty");
                }
            }
            Condition::Process { name } => {
                if name.trim().is_empty() {
                    self.error(Some(step), "processEmpty");
                }
            }
        }
    }

    fn timing(&mut self, step: &Step, poll_ms: i64, timeout_ms: i64) {
        if !(20..=60_000).contains(&poll_ms) {
            self.error(Some(step), "pollRange");
        }
        if timeout_ms < 0 {
            self.error(Some(step), "timeoutNegative");
        }
    }

    /// `depth` is the number of blocks around `steps`.
    fn steps(&mut self, steps: &[Step], depth: usize, in_loop: bool) {
        for (index, step) in steps.iter().enumerate() {
            self.pause(Some(step), &step.pause_after);
            match &step.repeat {
                Some(repeat) if step.kind.is_repeatable() => {
                    if !(1..=MAX_STEP_REPEAT).contains(&repeat.times) {
                        self.error(Some(step), "repeatRange");
                    }
                    self.pause(Some(step), &repeat.pause_between);
                }
                Some(_) => self.error(Some(step), "repeatNotAllowed"),
                None => {}
            }
            match &step.kind {
                StepKind::Click { position, .. } => self.position(step, position, true),
                StepKind::Move { position } => self.position(step, position, false),
                StepKind::Drag { from, to, .. } => {
                    self.position(step, from, true);
                    self.position(step, to, false);
                }
                StepKind::Scroll { notches, position, .. } => {
                    if !(1..=100).contains(notches) {
                        self.error(Some(step), "notchesRange");
                    }
                    self.position(step, position, true);
                }
                StepKind::Key { key, .. } => {
                    if key.is_empty() {
                        self.error(Some(step), "keyEmpty");
                    } else if !keys::is_known(key) {
                        self.error(Some(step), "keyUnknown");
                    }
                }
                StepKind::Text { text } => {
                    if text.is_empty() {
                        self.error(Some(step), "textEmpty");
                    }
                }
                StepKind::Pause { duration } => self.pause(Some(step), duration),
                StepKind::Wait { condition, poll_ms, timeout_ms, on_found, .. } => {
                    self.condition(step, condition);
                    self.timing(step, *poll_ms, *timeout_ms);
                    let is_image_found = matches!(condition, Condition::ImageFound { .. });
                    if !is_image_found && !matches!(on_found, OnFound::None) {
                        self.error(Some(step), "onFoundNotAllowed");
                    }
                }
                StepKind::If { condition, poll_ms, timeout_ms, then_steps, else_steps } => {
                    self.condition(step, condition);
                    self.timing(step, *poll_ms, *timeout_ms);
                    if depth >= MAX_DEPTH {
                        self.error(Some(step), "tooDeep");
                    }
                    self.steps(then_steps, depth + 1, in_loop);
                    self.steps(else_steps, depth + 1, in_loop);
                }
                StepKind::Loop { mode, pause_between, steps: body } => {
                    match mode {
                        LoopMode::Count { times } => {
                            if *times < 1 {
                                self.error(Some(step), "loopTimesRange");
                            }
                        }
                        LoopMode::While { condition } => self.condition(step, condition),
                        LoopMode::Forever => {
                            let has_later = steps[index + 1..].iter().any(|s| s.enabled);
                            if step.enabled && has_later && !has_own_exit(body) {
                                self.push(Some(step), "unreachableAfterForever", Severity::Warning);
                            }
                        }
                    }
                    self.pause(Some(step), pause_between);
                    if depth >= MAX_DEPTH {
                        self.error(Some(step), "tooDeep");
                    }
                    self.steps(body, depth + 1, true);
                }
                StepKind::BreakLoop => {
                    if !in_loop {
                        self.error(Some(step), "breakOutsideLoop");
                    }
                }
                StepKind::Stop => {}
            }
        }
    }
}

/// True when the loop body can leave this very loop: an enabled `BreakLoop`
/// that is not inside a nested loop.
fn has_own_exit(steps: &[Step]) -> bool {
    steps.iter().filter(|step| step.enabled).any(|step| match &step.kind {
        StepKind::BreakLoop => true,
        StepKind::If { then_steps, else_steps, .. } => has_own_exit(then_steps) || has_own_exit(else_steps),
        _ => false,
    })
}

/// Parses `#RRGGBB` into channels.
pub fn parse_color(text: &str) -> Option<(u8, u8, u8)> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some((channel(0)?, channel(2)?, channel(4)?))
}

fn repeat_ok(repeat: &RepeatMode) -> bool {
    match repeat {
        RepeatMode::Once | RepeatMode::Forever => true,
        RepeatMode::Count { times } => *times >= 1,
    }
}

/// `template_exists` answers whether a template file of this scenario is on disk.
pub fn validate_scenario(scenario: &Scenario, template_exists: &dyn Fn(&str) -> bool) -> Vec<Issue> {
    let mut checker = Checker { mode: scenario.mode, template_exists, issues: Vec::new() };
    if scenario.name.trim().is_empty() {
        checker.error(None, "nameEmpty");
    }
    if scenario.mode == ScenarioMode::Window && scenario.target.is_empty() {
        checker.error(None, "targetEmpty");
    }
    if !repeat_ok(&scenario.repeat) {
        checker.error(None, "roundsRange");
    }
    checker.pause(None, &scenario.pause_between_rounds);
    if scenario.step_count() > MAX_STEPS {
        checker.error(None, "tooManySteps");
    }
    checker.steps(&scenario.steps, 0, false);
    checker.issues
}

/// `scenario_exists` answers whether a scenario id is still on disk. Items of
/// a deleted scenario are reported with the item id in `step_id`.
pub fn validate_queue(queue: &Queue, scenario_exists: &dyn Fn(&str) -> bool) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut error = |item: Option<&str>, code: &str| {
        issues.push(Issue { step_id: item.map(str::to_string), code: code.to_string(), severity: Severity::Error });
    };
    if queue.name.trim().is_empty() {
        error(None, "nameEmpty");
    }
    if queue.items.is_empty() {
        error(None, "queueEmpty");
    }
    if !repeat_ok(&queue.repeat) {
        error(None, "roundsRange");
    }
    let pause = &queue.pause_between_items;
    if pause.from_ms < 0 || pause.to_ms < 0 {
        error(None, "pauseNegative");
    } else if pause.from_ms > pause.to_ms {
        error(None, "pauseOrder");
    }
    for item in &queue.items {
        if !scenario_exists(&item.scenario_id) {
            error(Some(&item.id), "scenarioMissing");
        }
        if item.rounds < 1 {
            error(Some(&item.id), "roundsRange");
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::queue::QueueItem;
    use crate::model::scenario::{ClickAction, MouseButton, Repeat, WindowTarget};

    fn click(position: Position) -> Step {
        Step::new(StepKind::Click { button: MouseButton::Left, action: ClickAction::Click, position })
    }

    fn looped(mode: LoopMode, steps: Vec<Step>) -> Step {
        Step::new(StepKind::Loop { mode, pause_between: Pause::default(), steps })
    }

    fn codes(scenario: &Scenario) -> Vec<String> {
        validate_scenario(scenario, &|_| true).into_iter().map(|issue| issue.code).collect()
    }

    fn scenario(steps: Vec<Step>) -> Scenario {
        let mut scenario = Scenario::new("test");
        scenario.steps = steps;
        scenario
    }

    #[test]
    fn valid_scenario_has_no_issues() {
        let mut step = click(Position::Absolute { x: 10, y: 20 });
        step.repeat = Some(Repeat { times: 5, pause_between: Pause::exact(100) });
        assert!(codes(&scenario(vec![step, click(Position::Cursor)])).is_empty());
    }

    #[test]
    fn pause_bounds_are_checked() {
        let mut step = click(Position::Cursor);
        step.pause_after = Pause { from_ms: 300, to_ms: 100 };
        assert_eq!(codes(&scenario(vec![step])), vec!["pauseOrder"]);
        let negative = Step::new(StepKind::Pause { duration: Pause { from_ms: -1, to_ms: 5 } });
        assert_eq!(codes(&scenario(vec![negative])), vec!["pauseNegative"]);
    }

    #[test]
    fn repeat_is_limited_and_only_for_simple_steps() {
        for times in [0, MAX_STEP_REPEAT + 1] {
            let mut step = click(Position::Cursor);
            step.repeat = Some(Repeat { times, pause_between: Pause::default() });
            assert_eq!(codes(&scenario(vec![step])), vec!["repeatRange"]);
        }
        let mut stop = Step::new(StepKind::Stop);
        stop.repeat = Some(Repeat { times: 2, pause_between: Pause::default() });
        assert_eq!(codes(&scenario(vec![stop])), vec!["repeatNotAllowed"]);
    }

    #[test]
    fn coordinates_must_match_the_mode() {
        assert_eq!(codes(&scenario(vec![click(Position::Client { x: 1, y: 1 })])), vec!["positionWrongMode"]);
        let mut windowed = scenario(vec![click(Position::Absolute { x: 1, y: 1 })]);
        windowed.mode = ScenarioMode::Window;
        assert_eq!(codes(&windowed), vec!["targetEmpty", "positionWrongMode"]);
        windowed.target = WindowTarget { title: "Notepad".into(), ..WindowTarget::default() };
        windowed.steps = vec![click(Position::Client { x: 1, y: 1 })];
        assert!(codes(&windowed).is_empty());
    }

    #[test]
    fn break_needs_an_enclosing_loop() {
        assert_eq!(codes(&scenario(vec![Step::new(StepKind::BreakLoop)])), vec!["breakOutsideLoop"]);
        let branch = Step::new(StepKind::If {
            condition: Condition::Process { name: "a.exe".into() },
            poll_ms: 200,
            timeout_ms: 0,
            then_steps: vec![Step::new(StepKind::BreakLoop)],
            else_steps: vec![],
        });
        assert!(codes(&scenario(vec![looped(LoopMode::Forever, vec![branch])])).is_empty());
    }

    #[test]
    fn fourth_nesting_level_is_rejected_for_mixed_blocks() {
        let level4 = looped(LoopMode::Count { times: 2 }, vec![]);
        let level3 = Step::new(StepKind::If {
            condition: Condition::Process { name: "a.exe".into() },
            poll_ms: 200,
            timeout_ms: 0,
            then_steps: vec![level4],
            else_steps: vec![],
        });
        let level2 = looped(LoopMode::Count { times: 2 }, vec![level3.clone()]);
        let level1 = looped(LoopMode::Count { times: 2 }, vec![level2]);
        assert_eq!(codes(&scenario(vec![level1])), vec!["tooDeep"]);

        let StepKind::If { condition, .. } = level3.kind else { unreachable!() };
        let three = looped(
            LoopMode::Count { times: 2 },
            vec![looped(LoopMode::While { condition }, vec![looped(LoopMode::Forever, vec![])])],
        );
        assert!(codes(&scenario(vec![three])).is_empty());
    }

    #[test]
    fn steps_after_an_endless_loop_give_a_warning() {
        let endless = looped(LoopMode::Forever, vec![click(Position::Cursor)]);
        let issues = validate_scenario(&scenario(vec![endless, click(Position::Cursor)]), &|_| true);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "unreachableAfterForever");
        assert_eq!(issues[0].severity, Severity::Warning);
        assert!(!has_errors(&issues));
    }

    #[test]
    fn missing_template_file_is_an_error() {
        let wait = Step::new(StepKind::Wait {
            condition: Condition::ImageFound { template: "a.png".into(), area: Area::Screen, threshold: 0.9 },
            poll_ms: 200,
            timeout_ms: 0,
            on_timeout: Default::default(),
            on_found: OnFound::None,
        });
        let issues = validate_scenario(&scenario(vec![wait]), &|_| false);
        assert_eq!(issues[0].code, "templateFileMissing");
    }

    #[test]
    fn colors_parse_strictly() {
        assert_eq!(parse_color("#FF8000"), Some((255, 128, 0)));
        assert_eq!(parse_color("FF8000"), None);
        assert_eq!(parse_color("#FF80"), None);
        assert_eq!(parse_color("#GG8000"), None);
    }

    #[test]
    fn queue_with_a_deleted_scenario_is_invalid() {
        let mut queue = Queue::new("q");
        queue.items = vec![
            QueueItem { id: "i1".into(), scenario_id: "alive".into(), rounds: 1 },
            QueueItem { id: "i2".into(), scenario_id: "gone".into(), rounds: 0 },
        ];
        let issues = validate_queue(&queue, &|id| id == "alive");
        let found: Vec<_> = issues.iter().map(|i| (i.step_id.as_deref(), i.code.as_str())).collect();
        assert_eq!(found, vec![(Some("i2"), "scenarioMissing"), (Some("i2"), "roundsRange")]);
        assert_eq!(validate_queue(&Queue::new("q"), &|_| true)[0].code, "queueEmpty");
    }
}
