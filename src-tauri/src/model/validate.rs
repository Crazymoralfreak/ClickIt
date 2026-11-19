//! Scenario and queue checks shared by the editor and by every start path.
//! A scenario with at least one `Error` issue does not start.

use std::collections::HashSet;

use serde::Serialize;

use super::graph::{Graph, Node, NodeKind, MAX_CALL_DEPTH, MAX_INTERCEPTS, MIN_INTERCEPT_MS};
use super::queue::Queue;
use super::scenario::{
    Area, Condition, CoordSpace, LoopMode, OnFound, Pause, Position, Repeat, RepeatMode, Scenario, ScenarioMode,
    Step, StepKind, MAX_DEPTH, MAX_STEPS, MAX_STEP_REPEAT,
};
use crate::input::keys;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Error,
    Warning,
}

/// `code` is a key under `issues.*` in the dictionaries. `step_id` is the id
/// of a step or of a block; `None` for problems of the scenario or queue as a whole.
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

/// What the checks ask the disk about.
pub struct Lookup<'a> {
    /// Whether a template file of the scenario with this id is on disk.
    pub template_exists: &'a dyn Fn(&str, &str) -> bool,
    /// Reads a scenario that another one runs; `None` when it is gone.
    pub scenario: &'a dyn Fn(&str) -> Option<Scenario>,
}

struct Checker<'a> {
    mode: ScenarioMode,
    scenario_id: &'a str,
    lookup: &'a Lookup<'a>,
    issues: Vec<Issue>,
}

impl Checker<'_> {
    fn error(&mut self, id: Option<&str>, code: &str) {
        self.push(id, code, Severity::Error);
    }

    fn push(&mut self, id: Option<&str>, code: &str, severity: Severity) {
        let issue = Issue { step_id: id.map(str::to_string), code: code.to_string(), severity };
        if !self.issues.contains(&issue) {
            self.issues.push(issue);
        }
    }

    fn pause(&mut self, id: Option<&str>, pause: &Pause) {
        if pause.from_ms < 0 || pause.to_ms < 0 {
            self.error(id, "pauseNegative");
        } else if pause.from_ms > pause.to_ms {
            self.error(id, "pauseOrder");
        }
    }

    fn space_matches(&self, space: CoordSpace) -> bool {
        match (self.mode, space) {
            (ScenarioMode::Screen, CoordSpace::Absolute) | (ScenarioMode::Window, CoordSpace::Client) => true,
            (ScenarioMode::Screen, CoordSpace::Client) | (ScenarioMode::Window, CoordSpace::Absolute) => false,
        }
    }

    /// `allow_cursor` is false where a concrete point is required.
    fn position(&mut self, id: &str, position: &Position, allow_cursor: bool) {
        match position {
            Position::Cursor => {
                if !allow_cursor {
                    self.error(Some(id), "positionRequired");
                }
            }
            Position::Absolute { .. } => {
                if !self.space_matches(CoordSpace::Absolute) {
                    self.error(Some(id), "positionWrongMode");
                }
            }
            Position::Client { x, y } => {
                if !self.space_matches(CoordSpace::Client) {
                    self.error(Some(id), "positionWrongMode");
                } else if *x < 0 || *y < 0 {
                    self.error(Some(id), "positionNegative");
                }
            }
        }
    }

    fn area(&mut self, id: &str, area: &Area) {
        match area {
            Area::Screen => {}
            Area::Client => {
                if self.mode != ScenarioMode::Window {
                    self.error(Some(id), "areaWrongMode");
                }
            }
            Area::Rect { space, width, height, .. } => {
                if !self.space_matches(*space) {
                    self.error(Some(id), "areaWrongMode");
                }
                if *width <= 0 || *height <= 0 {
                    self.error(Some(id), "areaEmpty");
                }
            }
        }
    }

    fn condition(&mut self, id: &str, condition: &Condition) {
        match condition {
            Condition::Pixel { point, color, tolerance } => {
                self.position(id, point, false);
                if parse_color(color).is_none() {
                    self.error(Some(id), "colorInvalid");
                }
                if !(0..=255).contains(tolerance) {
                    self.error(Some(id), "toleranceRange");
                }
            }
            Condition::ImageFound { template, area, threshold }
            | Condition::ImageGone { template, area, threshold } => {
                if template.is_empty() {
                    self.error(Some(id), "templateMissing");
                } else if !(self.lookup.template_exists)(self.scenario_id, template) {
                    self.error(Some(id), "templateFileMissing");
                }
                self.area(id, area);
                if !(0.5..=1.0).contains(threshold) {
                    self.error(Some(id), "thresholdRange");
                }
            }
            Condition::Window { title, .. } => {
                if title.trim().is_empty() {
                    self.error(Some(id), "titleEmpty");
                }
            }
            Condition::Process { name } => {
                if name.trim().is_empty() {
                    self.error(Some(id), "processEmpty");
                }
            }
        }
    }

    fn timing(&mut self, id: &str, poll_ms: i64, timeout_ms: i64) {
        if !(20..=60_000).contains(&poll_ms) {
            self.error(Some(id), "pollRange");
        }
        if timeout_ms < 0 {
            self.error(Some(id), "timeoutNegative");
        }
    }

    fn repeat(&mut self, id: &str, repeatable: bool, repeat: &Option<Repeat>) {
        match repeat {
            Some(repeat) if repeatable => {
                if !(1..=MAX_STEP_REPEAT).contains(&repeat.times) {
                    self.error(Some(id), "repeatRange");
                }
                self.pause(Some(id), &repeat.pause_between);
            }
            Some(_) => self.error(Some(id), "repeatNotAllowed"),
            None => {}
        }
    }

    /// Fields of the mouse, keyboard, text and pause actions. A step of a list
    /// and a block of a graph carry the same ones.
    fn action(&mut self, id: &str, kind: &StepKind) {
        match kind {
            StepKind::Click { position, .. } => self.position(id, position, true),
            StepKind::Move { position } => self.position(id, position, false),
            StepKind::Drag { from, to, .. } => {
                self.position(id, from, true);
                self.position(id, to, false);
            }
            StepKind::Scroll { notches, position, .. } => {
                if !(1..=100).contains(notches) {
                    self.error(Some(id), "notchesRange");
                }
                self.position(id, position, true);
            }
            StepKind::Key { key, .. } => {
                if key.is_empty() {
                    self.error(Some(id), "keyEmpty");
                } else if !keys::is_known(key) {
                    self.error(Some(id), "keyUnknown");
                }
            }
            StepKind::Text { text } => {
                if text.is_empty() {
                    self.error(Some(id), "textEmpty");
                }
            }
            StepKind::Pause { duration } => self.pause(Some(id), duration),
            StepKind::Wait { .. } | StepKind::If { .. } | StepKind::Loop { .. } | StepKind::BreakLoop | StepKind::Stop => {}
        }
    }

    fn wait(&mut self, id: &str, condition: &Condition, poll_ms: i64, timeout_ms: i64, on_found: &OnFound) {
        self.condition(id, condition);
        self.timing(id, poll_ms, timeout_ms);
        let is_image_found = matches!(condition, Condition::ImageFound { .. });
        if !is_image_found && !matches!(on_found, OnFound::None) {
            self.error(Some(id), "onFoundNotAllowed");
        }
    }

    /// `depth` is the number of blocks around `steps`.
    fn steps(&mut self, steps: &[Step], depth: usize, in_loop: bool) {
        for (index, step) in steps.iter().enumerate() {
            let id = step.id.as_str();
            self.pause(Some(id), &step.pause_after);
            self.repeat(id, step.kind.is_repeatable(), &step.repeat);
            self.action(id, &step.kind);
            match &step.kind {
                StepKind::Wait { condition, poll_ms, timeout_ms, on_found, .. } => {
                    self.wait(id, condition, *poll_ms, *timeout_ms, on_found);
                }
                StepKind::If { condition, poll_ms, timeout_ms, then_steps, else_steps } => {
                    self.condition(id, condition);
                    self.timing(id, *poll_ms, *timeout_ms);
                    if depth >= MAX_DEPTH {
                        self.error(Some(id), "tooDeep");
                    }
                    self.steps(then_steps, depth + 1, in_loop);
                    self.steps(else_steps, depth + 1, in_loop);
                }
                StepKind::Loop { mode, pause_between, steps: body } => {
                    match mode {
                        LoopMode::Count { times } => {
                            if *times < 1 {
                                self.error(Some(id), "loopTimesRange");
                            }
                        }
                        LoopMode::While { condition } => self.condition(id, condition),
                        LoopMode::Forever => {
                            let has_later = steps[index + 1..].iter().any(|s| s.enabled);
                            if step.enabled && has_later && !has_own_exit(body) {
                                self.push(Some(id), "unreachableAfterForever", Severity::Warning);
                            }
                        }
                    }
                    self.pause(Some(id), pause_between);
                    if depth >= MAX_DEPTH {
                        self.error(Some(id), "tooDeep");
                    }
                    self.steps(body, depth + 1, true);
                }
                StepKind::BreakLoop => {
                    if !in_loop {
                        self.error(Some(id), "breakOutsideLoop");
                    }
                }
                StepKind::Click { .. }
                | StepKind::Move { .. }
                | StepKind::Drag { .. }
                | StepKind::Scroll { .. }
                | StepKind::Key { .. }
                | StepKind::Text { .. }
                | StepKind::Pause { .. }
                | StepKind::Stop => {}
            }
        }
    }

    // Graph

    fn node(&mut self, scenario: &Scenario, node: &Node) {
        let id = node.id.as_str();
        self.pause(Some(id), &node.pause_after);
        let action = node.kind.action();
        // A "Run scenario" block repeats the scenario it runs.
        let repeatable =
            action.as_ref().is_some_and(StepKind::is_repeatable) || matches!(node.kind, NodeKind::Call { .. });
        self.repeat(id, repeatable, &node.repeat);
        if let Some(kind) = &action {
            self.action(id, kind);
        }
        match &node.kind {
            NodeKind::Wait { condition, poll_ms, timeout_ms, on_found } => {
                self.wait(id, condition, *poll_ms, *timeout_ms, on_found);
            }
            NodeKind::Branch { condition, poll_ms, timeout_ms } => {
                self.condition(id, condition);
                self.timing(id, *poll_ms, *timeout_ms);
            }
            NodeKind::Counter { times, pause_between } => {
                if *times < 1 {
                    self.error(Some(id), "loopTimesRange");
                }
                self.pause(Some(id), pause_between);
            }
            NodeKind::Intercept { condition, min_interval_ms } => {
                self.condition(id, condition);
                if *min_interval_ms < MIN_INTERCEPT_MS {
                    self.error(Some(id), "interceptInterval");
                }
            }
            NodeKind::Call { scenario_id } => self.call(scenario, id, scenario_id),
            NodeKind::Start
            | NodeKind::Stop
            | NodeKind::Click { .. }
            | NodeKind::Move { .. }
            | NodeKind::Drag { .. }
            | NodeKind::Scroll { .. }
            | NodeKind::Key { .. }
            | NodeKind::Text { .. }
            | NodeKind::Pause { .. } => {}
        }
    }

    /// A "Run scenario" block: what it runs has to exist, to work in the same
    /// mode, to be free of errors and not to lead back here.
    fn call(&mut self, scenario: &Scenario, id: &str, callee_id: &str) {
        if callee_id.is_empty() {
            self.error(Some(id), "callEmpty");
            return;
        }
        let mut path = vec![scenario.id.clone()];
        if let Err(code) = call_depth(self.lookup, callee_id, &mut path) {
            self.error(Some(id), code);
            return;
        }
        let Some(callee) = (self.lookup.scenario)(callee_id) else {
            self.error(Some(id), "callMissing");
            return;
        };
        if callee.mode != scenario.mode {
            self.error(Some(id), "callModeMismatch");
        } else if callee.mode == ScenarioMode::Window && callee.target != scenario.target {
            self.push(Some(id), "callOtherWindow", Severity::Warning);
        }
        if has_errors(&validate_with(&callee, self.lookup)) {
            self.error(Some(id), "callInvalid");
        }
    }

    fn graph(&mut self, scenario: &Scenario, graph: &Graph) {
        if graph.nodes.len() > MAX_STEPS {
            self.error(None, "tooManySteps");
        }
        let mut ids = HashSet::new();
        let mut starts = 0usize;
        let mut intercepts = 0usize;
        for node in &graph.nodes {
            if !ids.insert(node.id.as_str()) {
                self.error(Some(&node.id), "idDuplicate");
            }
            match node.kind {
                NodeKind::Start => {
                    starts += 1;
                    if starts > 1 {
                        self.error(Some(&node.id), "startMany");
                    }
                }
                NodeKind::Intercept { .. } => {
                    intercepts += 1;
                    if intercepts > MAX_INTERCEPTS {
                        self.error(Some(&node.id), "interceptTooMany");
                    }
                }
                _ => {}
            }
            self.node(scenario, node);
        }
        if starts == 0 {
            self.error(None, "startMissing");
        }

        let wiring = graph.wiring();
        let mut used = HashSet::new();
        for edge in &graph.edges {
            let (Some(from), Some(to)) = (wiring.index_of(&edge.from), wiring.index_of(&edge.to)) else {
                let known = wiring.index_of(&edge.from).map(|_| edge.from.as_str());
                self.error(known, "edgeBroken");
                continue;
            };
            if !graph.nodes[from].kind.ports().contains(&edge.port) {
                self.error(Some(&edge.from), "edgeBadPort");
            }
            if !used.insert((from, edge.port)) {
                self.error(Some(&edge.from), "portTwice");
            }
            if graph.nodes[to].kind.is_entry() {
                self.error(Some(&edge.to), "edgeIntoEntry");
            }
        }

        let Some(start) = graph.start() else {
            return;
        };
        let main = graph.reachable(&wiring, start);
        let mut reached = main.clone();
        for (at, node) in graph.nodes.iter().enumerate() {
            if !matches!(node.kind, NodeKind::Intercept { .. }) {
                continue;
            }
            let branch = graph.reachable(&wiring, at);
            if branch.iter().any(|index| main.contains(index)) {
                self.error(Some(&node.id), "interceptJoinsMain");
            }
            reached.extend(branch);
        }
        for (at, node) in graph.nodes.iter().enumerate() {
            if !reached.contains(&at) {
                self.push(Some(&node.id), "unreachable", Severity::Warning);
            }
        }
    }
}

/// Walks the scenarios `id` runs. `path` holds the ones around it, the
/// scenario being checked first.
fn call_depth(lookup: &Lookup, id: &str, path: &mut Vec<String>) -> Result<(), &'static str> {
    if path.iter().any(|seen| seen == id) {
        return Err("callCircular");
    }
    if path.len() > MAX_CALL_DEPTH {
        return Err("callTooDeep");
    }
    let Some(callee) = (lookup.scenario)(id) else {
        // Reported by the block that refers to it.
        return Ok(());
    };
    let Some(graph) = &callee.graph else {
        return Ok(());
    };
    path.push(id.to_string());
    let result = graph.callees().iter().try_for_each(|next| call_depth(lookup, next, path));
    path.pop();
    result
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

/// Checks a scenario that runs nothing else. `template_exists` answers whether
/// a template file of this scenario is on disk. A "Run scenario" block is
/// reported as pointing nowhere; `validate_with` knows the other scenarios.
pub fn validate_scenario(scenario: &Scenario, template_exists: &dyn Fn(&str) -> bool) -> Vec<Issue> {
    validate_with(scenario, &Lookup { template_exists: &|_, file| template_exists(file), scenario: &|_| None })
}

pub fn validate_with(scenario: &Scenario, lookup: &Lookup) -> Vec<Issue> {
    let mut checker = Checker { mode: scenario.mode, scenario_id: &scenario.id, lookup, issues: Vec::new() };
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
    match &scenario.graph {
        Some(graph) => checker.graph(scenario, graph),
        None => {
            if scenario.step_count() > MAX_STEPS {
                checker.error(None, "tooManySteps");
            }
            checker.steps(&scenario.steps, 0, false);
        }
    }
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
    use crate::model::graph::{Edge, Port};
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

    // Graph

    fn node(id: &str, kind: NodeKind) -> Node {
        let mut node = Node::new(kind);
        node.id = id.to_string();
        node
    }

    fn edge(from: &str, port: Port, to: &str) -> Edge {
        Edge { from: from.to_string(), port, to: to.to_string(), points: Vec::new() }
    }

    fn process(name: &str) -> Condition {
        Condition::Process { name: name.to_string() }
    }

    fn canvas(nodes: Vec<Node>, edges: Vec<Edge>) -> Scenario {
        let mut scenario = Scenario::new_canvas("graph");
        scenario.graph = Some(Graph { nodes, edges, notes: Vec::new() });
        scenario
    }

    fn found(scenario: &Scenario, lookup: &Lookup) -> Vec<(Option<String>, String)> {
        validate_with(scenario, lookup).into_iter().map(|issue| (issue.step_id, issue.code)).collect()
    }

    fn issue(id: &str, code: &str) -> (Option<String>, String) {
        (Some(id.to_string()), code.to_string())
    }

    #[test]
    fn a_wired_graph_has_no_issues_and_block_fields_are_checked_like_steps() {
        let good = canvas(
            vec![node("a", NodeKind::Start), node("b", NodeKind::Text { text: "hi".into() })],
            vec![edge("a", Port::Next, "b")],
        );
        assert!(codes(&good).is_empty());
        let bad = canvas(
            vec![
                node("a", NodeKind::Start),
                node("b", NodeKind::Text { text: String::new() }),
                node("c", NodeKind::Counter { times: 0, pause_between: Pause::default() }),
            ],
            vec![edge("a", Port::Next, "b"), edge("b", Port::Next, "c")],
        );
        assert_eq!(codes(&bad), vec!["textEmpty", "loopTimesRange"]);
    }

    #[test]
    fn a_graph_needs_exactly_one_start() {
        assert_eq!(codes(&canvas(vec![], vec![])), vec!["startMissing"]);
        let two = canvas(vec![node("a", NodeKind::Start), node("b", NodeKind::Start)], vec![]);
        assert_eq!(codes(&two), vec!["startMany", "unreachable"]);
    }

    #[test]
    fn links_are_checked() {
        let scenario = canvas(
            vec![node("a", NodeKind::Start), node("b", NodeKind::Stop), node("c", NodeKind::Stop)],
            vec![
                edge("a", Port::Next, "b"),
                edge("a", Port::Next, "c"),
                edge("b", Port::Next, "gone"),
                edge("a", Port::Yes, "b"),
                edge("b", Port::Next, "a"),
            ],
        );
        let lookup = Lookup { template_exists: &|_, _| true, scenario: &|_| None };
        assert_eq!(
            found(&scenario, &lookup),
            vec![
                issue("a", "portTwice"),
                issue("b", "edgeBroken"),
                issue("a", "edgeBadPort"),
                issue("b", "edgeBadPort"),
                issue("a", "edgeIntoEntry"),
                issue("c", "unreachable"),
            ]
        );
    }

    #[test]
    fn an_unreachable_block_is_only_a_warning() {
        let scenario = canvas(vec![node("a", NodeKind::Start), node("b", NodeKind::Stop)], vec![]);
        let issues = validate_scenario(&scenario, &|_| true);
        assert_eq!(issues[0].code, "unreachable");
        assert!(!has_errors(&issues));
    }

    #[test]
    fn an_intercept_keeps_its_own_branch() {
        let watch = NodeKind::Intercept { condition: process("a.exe"), min_interval_ms: 100 };
        let joined = canvas(
            vec![node("a", NodeKind::Start), node("b", NodeKind::Text { text: "x".into() }), node("i", watch.clone())],
            vec![edge("a", Port::Next, "b"), edge("i", Port::Next, "b")],
        );
        assert_eq!(codes(&joined), vec!["interceptInterval", "interceptJoinsMain"]);

        let mut nodes = vec![node("a", NodeKind::Start)];
        for at in 0..=MAX_INTERCEPTS {
            let kind = NodeKind::Intercept { condition: process("a.exe"), min_interval_ms: 2000 };
            nodes.push(node(&format!("i{at}"), kind));
        }
        assert_eq!(codes(&canvas(nodes, vec![])), vec!["interceptTooMany"]);
    }

    #[test]
    fn a_called_scenario_has_to_exist_and_must_not_lead_back() {
        let call = |id: &str, callee: &str| {
            let mut scenario = canvas(
                vec![node("a", NodeKind::Start), node("b", NodeKind::Call { scenario_id: callee.to_string() })],
                vec![edge("a", Port::Next, "b")],
            );
            scenario.id = id.to_string();
            scenario
        };
        let first = call("one", "two");
        let second = call("two", "one");
        let mut list = scenario(vec![click(Position::Cursor)]);
        list.id = "list".to_string();
        let mut windowed = list.clone();
        windowed.id = "windowed".to_string();
        windowed.mode = ScenarioMode::Window;
        windowed.target = WindowTarget { title: "Notepad".into(), ..WindowTarget::default() };
        windowed.steps = vec![];
        let mut broken = scenario(vec![Step::new(StepKind::Text { text: String::new() })]);
        broken.id = "broken".to_string();

        let all = [first.clone(), second, list, windowed, broken];
        let lookup = Lookup {
            template_exists: &|_, _| true,
            scenario: &|id| all.iter().find(|scenario| scenario.id == id).cloned(),
        };
        assert_eq!(found(&first, &lookup), vec![issue("b", "callCircular")]);
        assert_eq!(found(&call("x", "x"), &lookup), vec![issue("b", "callCircular")]);
        assert_eq!(found(&call("x", "gone"), &lookup), vec![issue("b", "callMissing")]);
        assert_eq!(found(&call("x", ""), &lookup), vec![issue("b", "callEmpty")]);
        assert_eq!(found(&call("x", "windowed"), &lookup), vec![issue("b", "callModeMismatch")]);
        assert_eq!(found(&call("x", "broken"), &lookup), vec![issue("b", "callInvalid")]);
        assert!(found(&call("x", "list"), &lookup).is_empty());
    }

    #[test]
    fn calls_nested_too_deep_are_rejected() {
        // s0 runs s1, s1 runs s2 and so on.
        let chain: Vec<Scenario> = (0..=MAX_CALL_DEPTH + 1)
            .map(|at| {
                let mut scenario = canvas(
                    vec![
                        node("a", NodeKind::Start),
                        node("b", NodeKind::Call { scenario_id: format!("s{}", at + 1) }),
                    ],
                    vec![edge("a", Port::Next, "b")],
                );
                scenario.id = format!("s{at}");
                scenario
            })
            .chain(std::iter::once({
                let mut last = scenario(vec![]);
                last.id = format!("s{}", MAX_CALL_DEPTH + 2);
                last
            }))
            .collect();
        let lookup = Lookup {
            template_exists: &|_, _| true,
            scenario: &|id| chain.iter().find(|scenario| scenario.id == id).cloned(),
        };
        assert_eq!(found(&chain[0], &lookup), vec![issue("b", "callTooDeep")]);
        // Eight levels below the caller are fine.
        assert!(found(&chain[2], &lookup).is_empty());
    }
}
