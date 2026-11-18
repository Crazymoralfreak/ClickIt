use serde::{Deserialize, Serialize};

/// Steps stored in one scenario file, nested ones included.
pub const MAX_STEPS: usize = 500;
/// Shared nesting limit for "If" and "Loop" blocks.
pub const MAX_DEPTH: usize = 3;
pub const MAX_STEP_REPEAT: i64 = 10_000;
pub const SCENARIO_VERSION: u32 = 1;

/// Pause in whole milliseconds, picked uniformly from `from_ms..=to_ms`.
/// Equal bounds mean an exact pause. Signed so that a bad value typed in the
/// editor reaches validation instead of failing to deserialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pause {
    pub from_ms: i64,
    pub to_ms: i64,
}

impl Pause {
    pub fn exact(ms: i64) -> Self {
        Self { from_ms: ms, to_ms: ms }
    }

    pub fn is_zero(&self) -> bool {
        self.from_ms <= 0 && self.to_ms <= 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScenarioMode {
    #[default]
    Screen,
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TitleMatch {
    #[default]
    Contains,
    Exact,
}

/// Target of the "Window" mode. Empty strings mean "not set"; when both are
/// set a window has to match both.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowTarget {
    pub title: String,
    pub title_match: TitleMatch,
    pub process: String,
}

impl WindowTarget {
    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty() && self.process.trim().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Position {
    /// Wherever the cursor is when the step runs.
    Cursor,
    /// Virtual-screen pixels, used by the "Screen" mode.
    Absolute { x: i32, y: i32 },
    /// Pixels from the top-left corner of the target client area ("Window" mode).
    Client { x: i32, y: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClickAction {
    Click,
    Double,
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScrollDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyAction {
    Press,
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Modifiers {
    pub fn any(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.win
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CoordSpace {
    Absolute,
    Client,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Area {
    /// Whole virtual screen.
    Screen,
    /// Client area of the target window ("Window" mode only).
    Client,
    Rect { space: CoordSpace, x: i32, y: i32, width: i32, height: i32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Condition {
    /// `color` is `#RRGGBB`, `tolerance` is per channel, 0..=255.
    Pixel { point: Position, color: String, tolerance: i64 },
    /// `template` is a PNG file name inside the scenario folder,
    /// `threshold` is the required similarity, 0.5..=1.0.
    ImageFound { template: String, area: Area, threshold: f64 },
    ImageGone { template: String, area: Area, threshold: f64 },
    Window { title: String, title_match: TitleMatch },
    Process { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnTimeout {
    #[default]
    Stop,
    Continue,
}

/// What a "Wait" step does once an image has been found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum OnFound {
    #[default]
    None,
    /// Click at the centre of the match plus the offset in pixels.
    Click { button: MouseButton, offset_x: i32, offset_y: i32 },
}

/// Repeat of a single input or pause step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repeat {
    pub times: i64,
    #[serde(default)]
    pub pause_between: Pause,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LoopMode {
    Count { times: i64 },
    /// Checked once before every pass, without waiting.
    While { condition: Condition },
    Forever,
}

/// Repeat of a whole scenario or queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RepeatMode {
    #[default]
    Once,
    Count { times: i64 },
    Forever,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum StepKind {
    Click { button: MouseButton, action: ClickAction, position: Position },
    Move { position: Position },
    Drag { button: MouseButton, from: Position, to: Position },
    Scroll { direction: ScrollDirection, notches: i64, position: Position },
    /// `key` is a W3C `KeyboardEvent.code` name such as `KeyA`, `Enter`, `F5`.
    Key {
        key: String,
        action: KeyAction,
        #[serde(default)]
        modifiers: Modifiers,
    },
    Text { text: String },
    Pause { duration: Pause },
    /// `timeout_ms == 0` waits until the run is stopped by hand.
    Wait {
        condition: Condition,
        poll_ms: i64,
        timeout_ms: i64,
        #[serde(default)]
        on_timeout: OnTimeout,
        #[serde(default)]
        on_found: OnFound,
    },
    /// `timeout_ms == 0` checks the condition once, right away.
    If {
        condition: Condition,
        poll_ms: i64,
        timeout_ms: i64,
        #[serde(default)]
        then_steps: Vec<Step>,
        #[serde(default)]
        else_steps: Vec<Step>,
    },
    Loop {
        mode: LoopMode,
        #[serde(default)]
        pause_between: Pause,
        #[serde(default)]
        steps: Vec<Step>,
    },
    /// Leaves the nearest enclosing `Loop`.
    BreakLoop,
    /// Ends the run like the Stop button.
    Stop,
}

impl StepKind {
    /// Mouse, keyboard, text and pause steps: the ones that may carry `repeat`.
    pub fn is_repeatable(&self) -> bool {
        matches!(
            self,
            StepKind::Click { .. }
                | StepKind::Move { .. }
                | StepKind::Drag { .. }
                | StepKind::Scroll { .. }
                | StepKind::Key { .. }
                | StepKind::Text { .. }
                | StepKind::Pause { .. }
        )
    }

    /// Steps that send mouse or keyboard input.
    pub fn is_input(&self) -> bool {
        self.is_repeatable() && !matches!(self, StepKind::Pause { .. })
    }
}

fn enabled_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Empty until a save fills it in. The editor always sends one.
    #[serde(default)]
    pub id: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    #[serde(default)]
    pub pause_after: Pause,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Repeat>,
    #[serde(flatten)]
    pub kind: StepKind,
}

impl Step {
    pub fn new(kind: StepKind) -> Self {
        Self { id: new_id("s"), enabled: true, pause_after: Pause::default(), repeat: None, kind }
    }

    /// Nested step lists of a block, in display order.
    pub fn children(&self) -> Vec<&Vec<Step>> {
        match &self.kind {
            StepKind::If { then_steps, else_steps, .. } => vec![then_steps, else_steps],
            StepKind::Loop { steps, .. } => vec![steps],
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    #[serde(default)]
    pub version: u32,
    /// Grows by one on every successful save. A save carrying an older number is rejected.
    #[serde(default)]
    pub revision: u64,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mode: ScenarioMode,
    #[serde(default)]
    pub target: WindowTarget,
    #[serde(default)]
    pub repeat: RepeatMode,
    #[serde(default)]
    pub pause_between_rounds: Pause,
    #[serde(default)]
    pub steps: Vec<Step>,
}

impl Scenario {
    pub fn new(name: &str) -> Self {
        Self {
            version: SCENARIO_VERSION,
            revision: 0,
            id: new_id("sc"),
            name: name.to_string(),
            mode: ScenarioMode::Screen,
            target: WindowTarget::default(),
            repeat: RepeatMode::Once,
            pause_between_rounds: Pause::default(),
            steps: Vec::new(),
        }
    }

    pub fn step_count(&self) -> usize {
        count_steps(&self.steps)
    }

    /// Template file names referenced by any condition in the scenario.
    pub fn templates(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_templates(&self.steps, &mut out);
        out.sort();
        out.dedup();
        out
    }

    /// Fills steps the caller left without an id, including nested ones.
    pub fn fill_empty_ids(&mut self) {
        fill_empty_ids(&mut self.steps);
    }

    /// Gives every step a fresh id; used by duplicate and import.
    pub fn renew_step_ids(&mut self) {
        renew_ids(&mut self.steps);
    }
}

/// Counts steps in the list and in every nested list.
pub fn count_steps(steps: &[Step]) -> usize {
    steps
        .iter()
        .map(|step| 1 + step.children().iter().map(|list| count_steps(list)).sum::<usize>())
        .sum()
}

fn condition_template(condition: &Condition) -> Option<&str> {
    match condition {
        Condition::ImageFound { template, .. } | Condition::ImageGone { template, .. } => Some(template),
        Condition::Pixel { .. } | Condition::Window { .. } | Condition::Process { .. } => None,
    }
}

fn collect_templates(steps: &[Step], out: &mut Vec<String>) {
    for step in steps {
        let condition = match &step.kind {
            StepKind::Wait { condition, .. } | StepKind::If { condition, .. } => Some(condition),
            StepKind::Loop { mode: LoopMode::While { condition }, .. } => Some(condition),
            _ => None,
        };
        if let Some(name) = condition.and_then(condition_template) {
            if !name.is_empty() {
                out.push(name.to_string());
            }
        }
        for list in step.children() {
            collect_templates(list, out);
        }
    }
}

fn fill_empty_ids(steps: &mut [Step]) {
    for step in steps {
        if step.id.is_empty() {
            step.id = new_id("s");
        }
        match &mut step.kind {
            StepKind::If { then_steps, else_steps, .. } => {
                fill_empty_ids(then_steps);
                fill_empty_ids(else_steps);
            }
            StepKind::Loop { steps, .. } => fill_empty_ids(steps),
            StepKind::Click { .. }
            | StepKind::Move { .. }
            | StepKind::Drag { .. }
            | StepKind::Scroll { .. }
            | StepKind::Key { .. }
            | StepKind::Text { .. }
            | StepKind::Pause { .. }
            | StepKind::Wait { .. }
            | StepKind::BreakLoop
            | StepKind::Stop => {}
        }
    }
}

fn renew_ids(steps: &mut [Step]) {
    for step in steps {
        step.id = new_id("s");
        match &mut step.kind {
            StepKind::If { then_steps, else_steps, .. } => {
                renew_ids(then_steps);
                renew_ids(else_steps);
            }
            StepKind::Loop { steps, .. } => renew_ids(steps),
            _ => {}
        }
    }
}

/// Short unique id: prefix, milliseconds since the epoch and a random tail.
pub fn new_id(prefix: &str) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{prefix}{millis:x}{:06x}", fastrand::u32(..0x100_0000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_round_trips_through_json() {
        let json = r#"{
            "id": "s3", "type": "click", "enabled": true, "button": "left", "action": "click",
            "position": { "kind": "absolute", "x": 640, "y": 360 },
            "repeat": { "times": 50, "pauseBetween": { "fromMs": 80, "toMs": 120 } },
            "pauseAfter": { "fromMs": 100, "toMs": 250 }
        }"#;
        let step: Step = serde_json::from_str(json).unwrap();
        assert_eq!(step.repeat.unwrap().times, 50);
        assert_eq!(step.pause_after, Pause { from_ms: 100, to_ms: 250 });
        assert!(matches!(
            step.kind,
            StepKind::Click { position: Position::Absolute { x: 640, y: 360 }, .. }
        ));
        let back: Step = serde_json::from_str(&serde_json::to_string(&step).unwrap()).unwrap();
        assert_eq!(back, step);
    }

    #[test]
    fn loop_block_and_unit_steps_parse() {
        let json = r#"{
            "id": "s4", "type": "loop", "mode": { "kind": "count", "times": 20 },
            "pauseBetween": { "fromMs": 300, "toMs": 600 },
            "steps": [ { "id": "s5", "type": "breakLoop" }, { "id": "s6", "type": "stop" } ]
        }"#;
        let step: Step = serde_json::from_str(json).unwrap();
        assert!(step.enabled);
        assert_eq!(count_steps(std::slice::from_ref(&step)), 3);
        let value = serde_json::to_value(&step).unwrap();
        assert_eq!(value["steps"][0]["type"], "breakLoop");
    }

    #[test]
    fn templates_are_collected_from_nested_conditions() {
        let image = |name: &str| Condition::ImageFound {
            template: name.to_string(),
            area: Area::Screen,
            threshold: 0.9,
        };
        let mut scenario = Scenario::new("t");
        scenario.steps = vec![Step::new(StepKind::Loop {
            mode: LoopMode::While { condition: image("b.png") },
            pause_between: Pause::default(),
            steps: vec![Step::new(StepKind::If {
                condition: image("a.png"),
                poll_ms: 200,
                timeout_ms: 0,
                then_steps: vec![],
                else_steps: vec![],
            })],
        })];
        assert_eq!(scenario.templates(), vec!["a.png".to_string(), "b.png".to_string()]);
        assert_eq!(scenario.step_count(), 2);
    }
}
