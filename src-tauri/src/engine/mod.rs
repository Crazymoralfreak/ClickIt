//! Executor of scenarios and queues. A run lives on its own thread; the rest of
//! the app starts it, asks it to stop and reads the status hub.
//!
//! Rules that shape the code below:
//! - before every input action and every condition check the guards run: the
//!   stop request, the screen corner, the "do not run while running" list and,
//!   in the "Window" mode, the foreground window. A failed guard pauses the run,
//!   releases what was pressed and keeps every counter;
//! - a step interrupted by such a pause starts over;
//! - time spent paused does not count towards condition timeouts.

pub mod conditions;
pub mod timing;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};
use crate::input::keys::vk_from_code;
use crate::input::{self, Input};
use crate::journal::Journal;
use crate::model::queue::OnItemFailure;
use crate::model::scenario::{
    ClickAction, Condition, KeyAction, LoopMode, Modifiers, MouseButton, OnFound, OnTimeout, Pause, Position,
    RepeatMode, Scenario, ScenarioMode, ScrollDirection, Step, StepKind,
};
use crate::model::schedule::{RunTarget, TargetKind};
use crate::model::validate::{has_errors, parse_color, validate_queue, validate_scenario};
use crate::runtime::{Outcome, QueueProgress, Rect, RunFinished, RunState, RunStatus, StatusHub, TargetInfo};
use crate::screen::{self, Template};
use crate::storage::Store;
use crate::target::{self, Hwnd};
use conditions::area_rect;
use timing::{cursor_in_corner, pick_ms, TimerResolution};

/// Longest single sleep between two looks at the stop request.
const SLICE: Duration = Duration::from_millis(10);
/// A loop pass or a round without input and without a pause ends with this delay.
const EMPTY_PASS_DELAY_MS: u64 = 10;
const BLOCKED_CHECK_EVERY: Duration = Duration::from_millis(500);
const PAUSE_POLL_MS: u64 = 100;
/// How long a button or a key stays down in a click or a key press.
const HOLD_MS: u64 = 20;
const DOUBLE_CLICK_GAP_MS: u64 = 60;
const DRAG_STEPS: i32 = 10;
const DRAG_STEP_MS: u64 = 10;
const WHEEL_GAP_MS: u64 = 5;
const TEXT_CHUNK_CHARS: usize = 20;
const FOCUS_WAIT: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Manual,
    Hotkey,
    Tray,
    Schedule,
    /// Started by a harness or by the assistant inside the window.
    Harness,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Manual => "manual",
            Source::Hotkey => "hotkey",
            Source::Tray => "tray",
            Source::Schedule => "schedule",
            Source::Harness => "harness",
        }
    }

    /// Starts by the user's hand get the countdown, scheduled ones do not.
    pub fn has_countdown(self) -> bool {
        !matches!(self, Source::Schedule)
    }
}

/// One scenario to run: the only item of a scenario run or an item of a queue.
pub struct PlanItem {
    pub scenario: Scenario,
    /// Folder with the template images.
    pub dir: PathBuf,
    pub rounds: RepeatMode,
    /// The scenario has validation errors; inside a queue this fails the item.
    pub invalid: bool,
}

pub struct QueuePlan {
    pub pause_between_items: Pause,
    pub repeat: RepeatMode,
    pub on_item_failure: OnItemFailure,
}

/// Everything a run needs, read from the disk before it starts.
pub struct Plan {
    pub info: TargetInfo,
    pub items: Vec<PlanItem>,
    pub queue: Option<QueuePlan>,
}

/// Loads and checks what is to be run. A scenario with errors, a queue with
/// errors or a queue that refers to a deleted scenario does not produce a plan.
pub fn build_plan(store: &Store, target: &RunTarget) -> AppResult<Plan> {
    let scenario_ok = |scenario: &Scenario| {
        !has_errors(&validate_scenario(scenario, &|file| store.template_exists(&scenario.id, file)))
    };
    match target.kind {
        TargetKind::Scenario => {
            let scenario = store.load_scenario(&target.id)?;
            if !scenario_ok(&scenario) {
                return Err(AppError::new("invalid"));
            }
            let info = TargetInfo { kind: TargetKind::Scenario, id: scenario.id.clone(), name: scenario.name.clone() };
            let item =
                PlanItem { dir: store.scenario_dir(&scenario.id), rounds: scenario.repeat, invalid: false, scenario };
            Ok(Plan { info, items: vec![item], queue: None })
        }
        TargetKind::Queue => {
            let queue = store.load_queue(&target.id)?;
            if has_errors(&validate_queue(&queue, &|id| store.scenario_exists(id))) {
                return Err(AppError::new("queueInvalid"));
            }
            let mut items = Vec::with_capacity(queue.items.len());
            for item in &queue.items {
                let scenario = store.load_scenario(&item.scenario_id).map_err(|_| AppError::new("queueInvalid"))?;
                items.push(PlanItem {
                    dir: store.scenario_dir(&scenario.id),
                    rounds: RepeatMode::Count { times: item.rounds },
                    invalid: !scenario_ok(&scenario),
                    scenario,
                });
            }
            Ok(Plan {
                info: TargetInfo { kind: TargetKind::Queue, id: queue.id.clone(), name: queue.name.clone() },
                items,
                queue: Some(QueuePlan {
                    pause_between_items: queue.pause_between_items,
                    repeat: queue.repeat,
                    on_item_failure: queue.on_item_failure,
                }),
            })
        }
    }
}

pub struct RunOptions {
    pub source: Source,
    pub countdown_sec: u32,
    pub corner_stop: bool,
    pub blocked: Vec<String>,
}

type FinishedSink = Box<dyn Fn(&RunFinished) + Send + Sync>;

pub struct Engine {
    journal: Arc<Journal>,
    hub: Arc<StatusHub>,
    running: AtomicBool,
    stop: AtomicBool,
    stop_reason: Mutex<Option<AppError>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    on_finished: FinishedSink,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Engine {
    pub fn new(journal: Arc<Journal>, hub: Arc<StatusHub>, on_finished: FinishedSink) -> Arc<Self> {
        Arc::new(Self {
            journal,
            hub,
            running: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            stop_reason: Mutex::new(None),
            thread: Mutex::new(None),
            on_finished,
        })
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Starts the plan on a new thread. Fails with `busy` while another run is going.
    pub fn start(self: &Arc<Self>, plan: Plan, options: RunOptions) -> AppResult<()> {
        if self.running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err(AppError::new("busy"));
        }
        self.stop.store(false, Ordering::SeqCst);
        *lock(&self.stop_reason) = None;
        let engine = Arc::clone(self);
        let spawned = thread::Builder::new().name("clickit-run".to_string()).spawn(move || {
            let _resolution = TimerResolution::enter();
            let info = plan.info.clone();
            let (outcome, reason) = Runner::new(&engine, plan, options).run();
            engine.hub.set(&RunStatus::default());
            engine.hub.flush();
            let name = ("name", info.name.clone());
            let outcome_text = ("outcome", outcome.as_str().to_string());
            match &reason {
                Some(reason) => engine.journal.add_with_reason("runEndReason", &[name, outcome_text], reason),
                None => engine.journal.add("runEnd", &[name, outcome_text]),
            }
            engine.running.store(false, Ordering::SeqCst);
            (engine.on_finished)(&RunFinished { target: info, outcome, reason });
        });
        match spawned {
            Ok(handle) => {
                *lock(&self.thread) = Some(handle);
                Ok(())
            }
            Err(error) => {
                self.running.store(false, Ordering::SeqCst);
                Err(AppError::from(error))
            }
        }
    }

    /// Asks the run to stop; `reason` ends up in the journal and in the finish event.
    pub fn stop(&self, reason: AppError) {
        if self.is_running() {
            lock(&self.stop_reason).get_or_insert(reason);
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    /// Waits for the run thread to end. Used on exit, after `stop`.
    pub fn join(&self) {
        let handle = lock(&self.thread).take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    fn stop_requested(&self) -> Option<AppError> {
        self.stop
            .load(Ordering::SeqCst)
            .then(|| lock(&self.stop_reason).clone().unwrap_or_else(|| AppError::new("userStop")))
    }
}

/// Why execution left the normal path.
enum Halt {
    /// Stop button, hotkey, tray, screen corner, exit: ends the whole run.
    Stop(AppError),
    /// A "Stop scenario" step.
    StopStep(AppError),
    /// The scenario cannot go on: timeout, closed window, rejected input.
    Fail(AppError),
}

type Exec<T> = Result<T, Halt>;

#[derive(PartialEq, Eq)]
enum Flow {
    Next,
    /// An "Exit loop" step is unwinding to the nearest loop.
    Break,
}

enum Attempt {
    Done,
    /// A pause interrupted the step; it has to start over.
    Retry,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PauseKind {
    Window,
    Process,
}

/// A condition that holds; `center` is set for a found image.
struct Hit {
    center: Option<(i32, i32)>,
}

struct Runner<'a> {
    engine: &'a Engine,
    plan: Plan,
    options: RunOptions,
    status: RunStatus,
    input: Input,
    /// Index into `plan.items` of the scenario being executed.
    current: usize,
    /// Target window of the current scenario in the "Window" mode.
    hwnd: Option<Hwnd>,
    templates: HashMap<String, Arc<Template>>,
    /// 1-based positions of the current step and of the blocks around it.
    path: Vec<usize>,
    /// State to return to after a pause: running or waiting.
    base_state: RunState,
    pause: Option<PauseKind>,
    /// Whether anything was sent or slept since the flag was last cleared.
    activity: bool,
    blocked_checked: Option<Instant>,
    blocked_found: Option<String>,
}

impl<'a> Runner<'a> {
    fn new(engine: &'a Engine, plan: Plan, options: RunOptions) -> Self {
        let status = RunStatus { target: Some(plan.info.clone()), ..RunStatus::default() };
        Self {
            engine,
            plan,
            options,
            status,
            input: Input::new(),
            current: 0,
            hwnd: None,
            templates: HashMap::new(),
            path: Vec::new(),
            base_state: RunState::Running,
            pause: None,
            activity: false,
            blocked_checked: None,
            blocked_found: None,
        }
    }

    fn run(mut self) -> (Outcome, Option<AppError>) {
        let name = self.plan.info.name.clone();
        self.engine
            .journal
            .add("runStart", &[("name", name), ("source", self.options.source.as_str().to_string())]);
        let result = self.countdown().and_then(|()| {
            if self.plan.queue.is_some() {
                self.run_queue()
            } else {
                self.run_item(0)
            }
        });
        self.input.release_all();
        match result {
            Ok(()) => (Outcome::Completed, None),
            Err(Halt::Stop(reason)) | Err(Halt::StopStep(reason)) => (Outcome::Stopped, Some(reason)),
            Err(Halt::Fail(reason)) => (Outcome::Failed, Some(reason)),
        }
    }

    // Status

    fn push(&self) {
        self.engine.hub.set(&self.status);
    }

    fn set_state(&mut self, state: RunState) {
        if self.status.state != state {
            self.status.state = state;
            self.push();
        }
    }

    fn label(&self) -> String {
        self.path.iter().map(|index| index.to_string()).collect::<Vec<_>>().join(".")
    }

    fn fail(&self, code: &str) -> Halt {
        Halt::Fail(AppError::new(code).with("step", self.label()))
    }

    fn scenario(&self) -> &Scenario {
        &self.plan.items[self.current].scenario
    }

    // Time

    fn check_stop(&self) -> Exec<()> {
        if let Some(reason) = self.engine.stop_requested() {
            return Err(Halt::Stop(reason));
        }
        if self.options.corner_stop && cursor_in_corner() {
            return Err(Halt::Stop(AppError::new("cornerStop")));
        }
        Ok(())
    }

    /// Sleeps in short slices so that a stop is noticed at once.
    fn idle(&self, ms: u64) -> Exec<()> {
        let deadline = Instant::now() + Duration::from_millis(ms);
        loop {
            self.check_stop()?;
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            thread::sleep(left.min(SLICE));
        }
    }

    /// A pause that is part of the scenario.
    fn sleep(&mut self, ms: u64) -> Exec<()> {
        if ms == 0 {
            return self.check_stop();
        }
        self.activity = true;
        self.engine.hub.flush();
        self.idle(ms)
    }

    fn pause_for(&mut self, pause: Pause) -> Exec<()> {
        self.sleep(pick_ms(pause))
    }

    fn countdown(&mut self) -> Exec<()> {
        if !self.options.source.has_countdown() {
            return Ok(());
        }
        for left in (1..=self.options.countdown_sec).rev() {
            self.status.state = RunState::Countdown;
            self.status.countdown_left = Some(left);
            self.push();
            self.engine.hub.flush();
            self.idle(1000)?;
        }
        self.status.countdown_left = None;
        Ok(())
    }

    // Guards

    fn blocked_process(&mut self) -> Option<String> {
        if self.options.blocked.is_empty() {
            return None;
        }
        let fresh = self.blocked_checked.is_some_and(|at| at.elapsed() < BLOCKED_CHECK_EVERY);
        if !fresh {
            self.blocked_found = target::first_running(&self.options.blocked);
            self.blocked_checked = Some(Instant::now());
        }
        self.blocked_found.clone()
    }

    fn enter_pause(&mut self, kind: PauseKind, process: Option<String>) {
        if self.pause == Some(kind) {
            return;
        }
        self.input.release_all();
        self.pause = Some(kind);
        let name = ("name", self.scenario().name.clone());
        match kind {
            PauseKind::Window => {
                self.engine.journal.add("pauseWindow", &[name]);
                self.status.state = RunState::PausedWindow;
                self.status.blocked_process = None;
            }
            PauseKind::Process => {
                let process = process.unwrap_or_default();
                self.engine.journal.add("pauseProcess", &[name, ("process", process.clone())]);
                self.status.state = RunState::PausedProcess;
                self.status.blocked_process = Some(process);
            }
        }
        self.push();
        self.engine.hub.flush();
    }

    /// Runs before every input action and every condition check. Returns
    /// whether the run had to pause, in which case a started step starts over.
    fn guard(&mut self) -> Exec<bool> {
        let mut paused = false;
        loop {
            self.check_stop()?;
            if let Some(process) = self.blocked_process() {
                self.enter_pause(PauseKind::Process, Some(process));
            } else if let Some(hwnd) = self.hwnd {
                if !target::is_window(hwnd) {
                    return Err(self.fail("targetClosed"));
                }
                if target::foreground() == hwnd {
                    break;
                }
                self.enter_pause(PauseKind::Window, None);
            } else {
                break;
            }
            paused = true;
            self.idle(PAUSE_POLL_MS)?;
        }
        if let Some(kind) = self.pause.take() {
            let name = ("name", self.scenario().name.clone());
            let code = match kind {
                PauseKind::Window => "resumeWindow",
                PauseKind::Process => "resumeProcess",
            };
            self.engine.journal.add(code, &[name]);
            self.status.state = self.base_state;
            self.status.blocked_process = None;
            self.push();
        }
        Ok(paused)
    }

    // Scenario and queue

    fn rounds_total(rounds: RepeatMode) -> Option<i64> {
        match rounds {
            RepeatMode::Once => Some(1),
            RepeatMode::Count { times } => Some(times),
            RepeatMode::Forever => None,
        }
    }

    /// Finds the target window and brings it to the foreground.
    fn focus_target(&mut self) -> Exec<()> {
        let window_target = self.scenario().target.clone();
        let described = target::describe_target(&window_target);
        let Some(hwnd) = target::find_target(&window_target) else {
            return Err(Halt::Fail(AppError::new("targetNotFound").with("target", described)));
        };
        target::request_focus(hwnd);
        let deadline = Instant::now() + FOCUS_WAIT;
        while target::foreground() != hwnd {
            if Instant::now() >= deadline {
                return Err(Halt::Fail(AppError::new("targetNotFocused").with("target", described)));
            }
            self.idle(20)?;
        }
        self.hwnd = Some(hwnd);
        Ok(())
    }

    fn run_item(&mut self, index: usize) -> Exec<()> {
        self.current = index;
        self.hwnd = None;
        self.templates.clear();
        self.path.clear();
        let rounds = Self::rounds_total(self.plan.items[index].rounds);
        self.status.scenario_id = Some(self.scenario().id.clone());
        self.status.scenario_name = Some(self.scenario().name.clone());
        self.status.round = 0;
        self.status.rounds_total = rounds;
        self.status.step_id = None;
        self.status.step_label = None;
        self.base_state = RunState::Running;
        self.status.state = RunState::Running;
        self.push();

        if self.plan.items[index].invalid {
            return Err(Halt::Fail(AppError::new("invalid")));
        }
        if self.scenario().mode == ScenarioMode::Window {
            self.focus_target()?;
        }

        let steps = self.scenario().steps.clone();
        let pause_between_rounds = self.scenario().pause_between_rounds;
        let mut round = 0i64;
        loop {
            self.check_stop()?;
            round += 1;
            self.status.round = round;
            self.activity = false;
            self.run_steps(&steps)?;
            if rounds.is_some_and(|total| round >= total) {
                return Ok(());
            }
            if !self.activity {
                self.idle(EMPTY_PASS_DELAY_MS)?;
            }
            self.pause_for(pause_between_rounds)?;
        }
    }

    fn run_queue(&mut self) -> Exec<()> {
        let Some(queue) = self.plan.queue.take() else {
            return Ok(());
        };
        let rounds = Self::rounds_total(queue.repeat);
        let count = self.plan.items.len();
        let queue_name = self.plan.info.name.clone();
        let mut round = 0i64;
        loop {
            round += 1;
            let mut worked = false;
            for index in 0..count {
                self.check_stop()?;
                if index > 0 || round > 1 {
                    self.pause_for(queue.pause_between_items)?;
                }
                self.status.queue =
                    Some(QueueProgress { item_index: index + 1, item_count: count, round, rounds_total: rounds });
                let item_name = self.plan.items[index].scenario.name.clone();
                self.engine.journal.add(
                    "queueItemStart",
                    &[("queue", queue_name.clone()), ("index", (index + 1).to_string()), ("name", item_name.clone())],
                );
                let result = self.run_item(index);
                self.input.release_all();
                match result {
                    Ok(()) => worked = true,
                    Err(Halt::Stop(reason)) => return Err(Halt::Stop(reason)),
                    Err(Halt::StopStep(reason)) | Err(Halt::Fail(reason)) => {
                        self.engine.journal.add_with_reason(
                            "queueItemFailed",
                            &[("queue", queue_name.clone()), ("name", item_name.clone())],
                            &reason,
                        );
                        match queue.on_item_failure {
                            OnItemFailure::Stop => return Err(Halt::Fail(reason)),
                            OnItemFailure::Next => {}
                        }
                    }
                }
            }
            if rounds.is_some_and(|total| round >= total) {
                return Ok(());
            }
            if !worked {
                // Every item failed at once: do not spin.
                self.idle(PAUSE_POLL_MS)?;
            }
        }
    }

    // Steps

    fn run_steps(&mut self, steps: &[Step]) -> Exec<Flow> {
        for (index, step) in steps.iter().enumerate() {
            if !step.enabled {
                continue;
            }
            self.path.push(index + 1);
            self.status.step_id = Some(step.id.clone());
            self.status.step_label = Some(self.label());
            self.push();
            let flow = self.run_step(step);
            self.path.pop();
            if flow? == Flow::Break {
                return Ok(Flow::Break);
            }
        }
        Ok(Flow::Next)
    }

    fn run_step(&mut self, step: &Step) -> Exec<Flow> {
        self.check_stop()?;
        match &step.kind {
            StepKind::Wait { condition, poll_ms, timeout_ms, on_timeout, on_found } => {
                self.run_wait(condition, *poll_ms, *timeout_ms, *on_timeout, *on_found)?;
            }
            StepKind::If { condition, poll_ms, timeout_ms, then_steps, else_steps } => {
                let hit = self.wait_condition(condition, *poll_ms, *timeout_ms, true)?;
                let branch = if hit.is_some() { then_steps } else { else_steps };
                if self.run_steps(branch)? == Flow::Break {
                    return Ok(Flow::Break);
                }
            }
            StepKind::Loop { mode, pause_between, steps } => self.run_loop(mode, *pause_between, steps)?,
            StepKind::BreakLoop => return Ok(Flow::Break),
            StepKind::Stop => return Err(Halt::StopStep(AppError::new("stopStep").with("step", self.label()))),
            StepKind::Click { .. }
            | StepKind::Move { .. }
            | StepKind::Drag { .. }
            | StepKind::Scroll { .. }
            | StepKind::Key { .. }
            | StepKind::Text { .. }
            | StepKind::Pause { .. } => self.run_repeatable(step)?,
        }
        self.pause_for(step.pause_after)?;
        Ok(Flow::Next)
    }

    fn run_repeatable(&mut self, step: &Step) -> Exec<()> {
        let times = step.repeat.map_or(1, |repeat| repeat.times);
        for pass in 1..=times {
            if times > 1 {
                self.status.step_repeat = Some(pass);
                self.status.step_repeat_total = Some(times);
                self.push();
            }
            let result = self.run_action(step).and_then(|()| match step.repeat {
                Some(repeat) if pass < times => self.pause_for(repeat.pause_between),
                _ => Ok(()),
            });
            if result.is_err() {
                self.status.step_repeat = None;
                self.status.step_repeat_total = None;
                return result;
            }
        }
        self.status.step_repeat = None;
        self.status.step_repeat_total = None;
        Ok(())
    }

    fn run_action(&mut self, step: &Step) -> Exec<()> {
        if let StepKind::Pause { duration } = &step.kind {
            return self.pause_for(*duration);
        }
        loop {
            self.guard()?;
            match self.try_action(&step.kind)? {
                Attempt::Done => {
                    self.activity = true;
                    return Ok(());
                }
                Attempt::Retry => self.input.release_all(),
            }
        }
    }

    fn sent(&self, result: AppResult<()>) -> Exec<()> {
        result.map_err(|error| Halt::Fail(error.with("step", self.label())))
    }

    /// Where a step acts, in virtual-screen pixels; `None` leaves the cursor
    /// where it is. In the "Window" mode the point has to be inside the client
    /// area of the target, otherwise nothing is sent and the scenario fails.
    fn point(&self, position: &Position) -> Exec<Option<(i32, i32)>> {
        let outside = |x: i32, y: i32| {
            Halt::Fail(AppError::new("pointOutsideTarget").with("step", self.label()).with("x", x).with("y", y))
        };
        let Some(hwnd) = self.hwnd else {
            return match position {
                Position::Cursor => Ok(None),
                Position::Absolute { x, y } => Ok(Some((*x, *y))),
                Position::Client { x, y } => Err(outside(*x, *y)),
            };
        };
        let client = target::client_rect(hwnd).ok_or_else(|| self.fail("targetClosed"))?;
        match position {
            Position::Cursor => {
                let (x, y) = input::cursor_pos();
                if client.contains(x, y) {
                    Ok(None)
                } else {
                    Err(outside(x - client.x, y - client.y))
                }
            }
            Position::Client { x, y } => {
                let point = (client.x + *x, client.y + *y);
                if client.contains(point.0, point.1) {
                    Ok(Some(point))
                } else {
                    Err(outside(*x, *y))
                }
            }
            Position::Absolute { x, y } => Err(outside(*x, *y)),
        }
    }

    fn move_to(&mut self, point: Option<(i32, i32)>) -> Exec<()> {
        match point {
            Some((x, y)) => {
                let result = self.input.move_to(x, y);
                self.sent(result)
            }
            None => Ok(()),
        }
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Exec<()> {
        let result = self.input.button(button, down);
        self.sent(result)
    }

    fn click_once(&mut self, button: MouseButton) -> Exec<()> {
        self.button(button, true)?;
        self.idle(HOLD_MS)?;
        self.button(button, false)
    }

    fn try_action(&mut self, kind: &StepKind) -> Exec<Attempt> {
        match kind {
            StepKind::Click { button, action, position } => {
                let point = self.point(position)?;
                self.move_to(point)?;
                match action {
                    ClickAction::Click => self.click_once(*button)?,
                    ClickAction::Double => {
                        self.click_once(*button)?;
                        self.idle(DOUBLE_CLICK_GAP_MS)?;
                        if self.guard()? {
                            return Ok(Attempt::Retry);
                        }
                        self.click_once(*button)?;
                    }
                    ClickAction::Down => self.button(*button, true)?,
                    ClickAction::Up => self.button(*button, false)?,
                }
            }
            StepKind::Move { position } => {
                let point = self.point(position)?;
                self.move_to(point)?;
            }
            StepKind::Drag { button, from, to } => {
                let start = self.point(from)?.unwrap_or_else(input::cursor_pos);
                // Validation requires a concrete end point; a scenario that got here without one is broken.
                let Some(end) = self.point(to)? else {
                    return Err(self.fail("invalid"));
                };
                self.move_to(Some(start))?;
                self.button(*button, true)?;
                self.idle(HOLD_MS)?;
                // Programs start a drag only after they see the cursor travel.
                for part in 1..=DRAG_STEPS {
                    if self.guard()? {
                        return Ok(Attempt::Retry);
                    }
                    let x = start.0 + (end.0 - start.0) * part / DRAG_STEPS;
                    let y = start.1 + (end.1 - start.1) * part / DRAG_STEPS;
                    self.move_to(Some((x, y)))?;
                    self.idle(DRAG_STEP_MS)?;
                }
                self.button(*button, false)?;
            }
            StepKind::Scroll { direction, notches, position } => {
                let point = self.point(position)?;
                self.move_to(point)?;
                for notch in 0..*notches {
                    if notch > 0 {
                        self.idle(WHEEL_GAP_MS)?;
                        if self.guard()? {
                            return Ok(Attempt::Retry);
                        }
                    }
                    let result = self.input.wheel(*direction == ScrollDirection::Up);
                    self.sent(result)?;
                }
            }
            StepKind::Key { key, action, modifiers } => self.key(key, *action, *modifiers)?,
            StepKind::Text { text } => {
                let chars: Vec<char> = text.chars().collect();
                for (index, chunk) in chars.chunks(TEXT_CHUNK_CHARS).enumerate() {
                    if index > 0 {
                        self.idle(1)?;
                        if self.guard()? {
                            return Ok(Attempt::Retry);
                        }
                    }
                    let result = self.input.text(&chunk.iter().collect::<String>());
                    self.sent(result)?;
                }
            }
            // Not input actions; `run_step` and `run_action` never send them here.
            StepKind::Pause { .. }
            | StepKind::Wait { .. }
            | StepKind::If { .. }
            | StepKind::Loop { .. }
            | StepKind::BreakLoop
            | StepKind::Stop => {}
        }
        Ok(Attempt::Done)
    }

    fn key(&mut self, key: &str, action: KeyAction, modifiers: Modifiers) -> Exec<()> {
        let Some(virtual_key) = vk_from_code(key) else {
            return Err(self.fail("inputFailed"));
        };
        let result = match action {
            KeyAction::Press => self
                .input
                .modifiers(modifiers, true)
                .and_then(|()| self.input.key(virtual_key, true))
                .and_then(|()| {
                    thread::sleep(Duration::from_millis(HOLD_MS));
                    self.input.key(virtual_key, false)
                })
                .and_then(|()| self.input.modifiers(modifiers, false)),
            // A held combination keeps its modifiers down until the matching release step.
            KeyAction::Down => {
                self.input.modifiers(modifiers, true).and_then(|()| self.input.key(virtual_key, true))
            }
            KeyAction::Up => {
                self.input.key(virtual_key, false).and_then(|()| self.input.modifiers(modifiers, false))
            }
        };
        self.sent(result)
    }

    // Conditions

    fn template(&mut self, file: &str) -> Exec<Arc<Template>> {
        if let Some(template) = self.templates.get(file) {
            return Ok(Arc::clone(template));
        }
        let path = self.plan.items[self.current].dir.join(file);
        let template = Arc::new(Template::load(&path).map_err(|error| Halt::Fail(error.with("step", self.label())))?);
        self.templates.insert(file.to_string(), Arc::clone(&template));
        Ok(template)
    }

    fn client_rect(&self) -> Exec<Option<Rect>> {
        match self.hwnd {
            Some(hwnd) => target::client_rect(hwnd).map(Some).ok_or_else(|| self.fail("targetClosed")),
            None => Ok(None),
        }
    }

    /// One look at a condition, without waiting. A screen that could not be
    /// captured counts as "does not hold" for every kind, including "image gone".
    fn check(&mut self, condition: &Condition) -> Exec<Option<Hit>> {
        let plain = |holds: bool| holds.then_some(Hit { center: None });
        match condition {
            Condition::Pixel { point, color, tolerance } => {
                let (x, y) = self.point(point)?.unwrap_or_else(input::cursor_pos);
                let wanted = parse_color(color).unwrap_or_default();
                let tolerance = (*tolerance).clamp(0, 255) as u8;
                let holds = screen::pixel(x, y).is_ok_and(|actual| screen::color_within(actual, wanted, tolerance));
                Ok(plain(holds))
            }
            Condition::ImageFound { template, area, threshold } | Condition::ImageGone { template, area, threshold } => {
                let wants_found = matches!(condition, Condition::ImageFound { .. });
                let template = self.template(template)?;
                let client = self.client_rect()?;
                let Some(rect) = area_rect(area, screen::virtual_screen(), client) else {
                    return Ok(None);
                };
                let Ok(frame) = screen::capture(rect) else {
                    return Ok(None);
                };
                let found = screen::find_template(&frame, &template, *threshold).map(|(left, top)| {
                    (frame.rect.x + left + template.width / 2, frame.rect.y + top + template.height / 2)
                });
                Ok(match (wants_found, found) {
                    (true, Some(center)) => Some(Hit { center: Some(center) }),
                    (false, None) => Some(Hit { center: None }),
                    (true, None) | (false, Some(_)) => None,
                })
            }
            Condition::Window { title, title_match } => Ok(plain(target::window_exists(title, *title_match))),
            Condition::Process { name } => Ok(plain(target::process_running(name))),
        }
    }

    /// Polls a condition until it holds or the timeout runs out. A timeout of 0
    /// means one check when `once_if_zero` is set and "until stopped" otherwise.
    fn wait_condition(
        &mut self,
        condition: &Condition,
        poll_ms: i64,
        timeout_ms: i64,
        once_if_zero: bool,
    ) -> Exec<Option<Hit>> {
        self.base_state = RunState::Waiting;
        self.set_state(RunState::Waiting);
        let result = self.poll_condition(condition, poll_ms, timeout_ms, once_if_zero);
        self.base_state = RunState::Running;
        if result.is_ok() {
            self.set_state(RunState::Running);
        }
        result
    }

    fn poll_condition(
        &mut self,
        condition: &Condition,
        poll_ms: i64,
        timeout_ms: i64,
        once_if_zero: bool,
    ) -> Exec<Option<Hit>> {
        let timeout = Duration::from_millis(timeout_ms.max(0) as u64);
        // Only time spent checking and waiting counts, not time spent paused.
        let mut active = Duration::ZERO;
        loop {
            self.guard()?;
            let started = Instant::now();
            if let Some(hit) = self.check(condition)? {
                return Ok(Some(hit));
            }
            if timeout.is_zero() {
                if once_if_zero {
                    return Ok(None);
                }
            } else if active + started.elapsed() >= timeout {
                return Ok(None);
            }
            self.sleep(poll_ms.max(1) as u64)?;
            active += started.elapsed();
        }
    }

    fn run_wait(
        &mut self,
        condition: &Condition,
        poll_ms: i64,
        timeout_ms: i64,
        on_timeout: OnTimeout,
        on_found: OnFound,
    ) -> Exec<()> {
        let Some(hit) = self.wait_condition(condition, poll_ms, timeout_ms, false)? else {
            return match on_timeout {
                OnTimeout::Stop => Err(self.fail("conditionTimeout")),
                OnTimeout::Continue => Ok(()),
            };
        };
        let (OnFound::Click { button, offset_x, offset_y }, Some((x, y))) = (on_found, hit.center) else {
            return Ok(());
        };
        let point = (x + offset_x, y + offset_y);
        // In the "Window" mode this click obeys the pause and never leaves the target.
        self.guard()?;
        if let Some(client) = self.client_rect()? {
            if !client.contains(point.0, point.1) {
                return Err(Halt::Fail(
                    AppError::new("pointOutsideTarget")
                        .with("step", self.label())
                        .with("x", point.0 - client.x)
                        .with("y", point.1 - client.y),
                ));
            }
        }
        self.move_to(Some(point))?;
        self.click_once(button)?;
        self.activity = true;
        Ok(())
    }

    // Loop block

    fn run_loop(&mut self, mode: &LoopMode, pause_between: Pause, body: &[Step]) -> Exec<()> {
        let outer = (self.status.loop_pass, self.status.loop_total, self.activity);
        let total = match mode {
            LoopMode::Count { times } => Some(*times),
            LoopMode::While { .. } | LoopMode::Forever => None,
        };
        let mut pass = 0i64;
        let result = loop {
            if let Err(halt) = self.check_stop() {
                break Err(halt);
            }
            // No pause after the last pass of a counted loop.
            if total.is_some_and(|times| pass >= times) {
                break Ok(());
            }
            if pass > 0 {
                if let Err(halt) = self.pause_for(pause_between) {
                    break Err(halt);
                }
            }
            // Checked once, right before the pass, without waiting.
            if let LoopMode::While { condition } = mode {
                match self.guard().and_then(|_| self.check(condition)) {
                    Ok(Some(_)) => {}
                    Ok(None) => break Ok(()),
                    Err(halt) => break Err(halt),
                }
            }
            pass += 1;
            self.status.loop_pass = Some(pass);
            self.status.loop_total = total;
            self.activity = false;
            match self.run_steps(body) {
                Ok(Flow::Break) => break Ok(()),
                Ok(Flow::Next) => {}
                Err(halt) => break Err(halt),
            }
            if !self.activity {
                if let Err(halt) = self.idle(EMPTY_PASS_DELAY_MS) {
                    break Err(halt);
                }
            }
        };
        self.status.loop_pass = outer.0;
        self.status.loop_total = outer.1;
        // For the block around this one, a loop that ran at all counts as work.
        self.activity = outer.2 || pass > 0;
        result
    }
}
