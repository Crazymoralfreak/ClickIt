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
//!
//! A list of steps is walked as a tree, a graph block by block along its
//! links. Both use the same actions, waits, pauses and guards.

pub mod command;
pub mod conditions;
pub mod timing;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};
use crate::input::keys::vk_from_code;
use crate::input::{self, Input};
use crate::journal::Journal;
use crate::model::graph::{counter_port, Follow, Graph, Node, NodeKind, Port, Wiring, MAX_CALL_DEPTH};
use crate::model::queue::OnItemFailure;
use crate::model::scenario::{
    ClickAction, CommandSpec, Condition, KeyAction, LoopMode, Modifiers, MouseButton, OnFailure, OnFound, OnTimeout,
    Pause, Position, RepeatMode, Scenario, ScenarioMode, ScrollDirection, Step, StepKind,
};
use crate::model::schedule::{RunTarget, TargetKind};
use crate::model::validate::{has_errors, parse_color, validate_queue};
use crate::runtime::{
    Outcome, Phase, QueueProgress, Rect, RunFinished, RunState, RunStatus, RunTrace, StatusHub, TargetInfo, TraceMove,
    TraceStop,
};
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
/// The trace of a graph run goes to the window at most this often.
const TRACE_INTERVAL: Duration = Duration::from_millis(100);
/// Transitions one trace event carries; faster ones are not played one by one anyway.
const TRACE_MOVES: usize = 64;
/// Transitions the stored trace of a finished run keeps.
const TRACE_KEPT_MOVES: usize = 200;

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

/// A scenario that a "Run scenario" block runs.
pub struct Callee {
    pub scenario: Scenario,
    /// Folder with its template images.
    pub dir: PathBuf,
    graph: Option<(Arc<Graph>, Arc<Wiring>)>,
}

impl Callee {
    pub fn new(scenario: Scenario, dir: PathBuf) -> Self {
        let graph = scenario.graph.clone().map(|graph| {
            let wiring = Arc::new(graph.wiring());
            (Arc::new(graph), wiring)
        });
        Self { scenario, dir, graph }
    }
}

/// Everything a run needs, read from the disk before it starts.
pub struct Plan {
    pub info: TargetInfo,
    pub items: Vec<PlanItem>,
    pub queue: Option<QueuePlan>,
    /// Every scenario the items run through their blocks, by id.
    pub callees: HashMap<String, Arc<Callee>>,
}

impl Plan {
    /// Commands the run may start: the enabled "Command" steps and blocks of
    /// every item and of every scenario the items run.
    pub fn commands(&self) -> Vec<&CommandSpec> {
        let items = self.items.iter().map(|item| &item.scenario);
        let callees = self.callees.values().map(|callee| &callee.scenario);
        items.chain(callees).flat_map(|scenario| scenario.commands(true)).collect()
    }
}

/// Reads the scenarios `scenario` runs, directly and through others. The
/// checks have already made sure they exist and do not lead back.
fn load_callees(store: &Store, scenario: &Scenario, into: &mut HashMap<String, Arc<Callee>>) {
    let mut pending = scenario.graph.as_ref().map(|graph| graph.callees()).unwrap_or_default();
    while let Some(id) = pending.pop() {
        if into.contains_key(&id) {
            continue;
        }
        let Ok(callee) = store.load_scenario(&id) else {
            continue;
        };
        pending.extend(callee.graph.as_ref().map(|graph| graph.callees()).unwrap_or_default());
        let dir = store.scenario_dir(&id);
        into.insert(id, Arc::new(Callee::new(callee, dir)));
    }
}

/// Loads and checks what is to be run. A scenario with errors, a queue with
/// errors or a queue that refers to a deleted scenario does not produce a plan.
pub fn build_plan(store: &Store, target: &RunTarget) -> AppResult<Plan> {
    let scenario_ok = |scenario: &Scenario| !has_errors(&store.issues(scenario));
    let mut callees = HashMap::new();
    match target.kind {
        TargetKind::Scenario => {
            let scenario = store.load_scenario(&target.id)?;
            if !scenario_ok(&scenario) {
                return Err(AppError::new("invalid"));
            }
            load_callees(store, &scenario, &mut callees);
            let info = TargetInfo { kind: TargetKind::Scenario, id: scenario.id.clone(), name: scenario.name.clone() };
            let item =
                PlanItem { dir: store.scenario_dir(&scenario.id), rounds: scenario.repeat, invalid: false, scenario };
            Ok(Plan { info, items: vec![item], queue: None, callees })
        }
        TargetKind::Queue => {
            let queue = store.load_queue(&target.id)?;
            if has_errors(&validate_queue(&queue, &|id| store.scenario_exists(id))) {
                return Err(AppError::new("queueInvalid"));
            }
            let mut items = Vec::with_capacity(queue.items.len());
            for item in &queue.items {
                let scenario = store.load_scenario(&item.scenario_id).map_err(|_| AppError::new("queueInvalid"))?;
                load_callees(store, &scenario, &mut callees);
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
                callees,
            })
        }
    }
}

/// Breakpoints of a run started from the editor. They live in the open
/// editor only; the queue, the schedule, the hotkey and the harness have none.
#[derive(Debug, Clone, Default)]
pub struct Debug {
    /// Ids of the blocks the run stops on.
    pub breakpoints: HashSet<String>,
    /// Stop on every block, starting with the first one.
    pub step: bool,
}

/// How a run paused on a block goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resume {
    /// Run this block and stop on the next one.
    Step,
    /// Run until the next breakpoint.
    Continue,
}

/// Stands in for the screen in tests: `Some` answers a condition, `None` leaves it to the real check.
#[cfg(test)]
pub type Probe = Box<dyn FnMut(&Condition) -> Option<bool> + Send>;

pub struct RunOptions {
    pub source: Source,
    pub countdown_sec: u32,
    pub corner_stop: bool,
    pub blocked: Vec<String>,
    /// A trial run of a graph scenario: one pass from this block.
    pub start_node: Option<String>,
    pub debug: Option<Debug>,
    /// The "Command" steps may start programs and scripts. Without it a run
    /// that reaches such a step fails.
    pub allow_commands: bool,
    #[cfg(test)]
    pub probe: Option<Probe>,
}

impl RunOptions {
    /// No countdown, no guards and nothing to debug; the caller sets what it needs.
    pub fn new(source: Source) -> Self {
        Self {
            source,
            countdown_sec: 0,
            corner_stop: false,
            blocked: Vec::new(),
            start_node: None,
            debug: None,
            allow_commands: false,
            #[cfg(test)]
            probe: None,
        }
    }
}

type FinishedSink = Box<dyn Fn(&RunFinished) + Send + Sync>;
type TraceSink = Box<dyn Fn(&RunTrace) + Send + Sync>;

pub struct Engine {
    journal: Arc<Journal>,
    hub: Arc<StatusHub>,
    running: AtomicBool,
    stop: AtomicBool,
    stop_reason: Mutex<Option<AppError>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    on_finished: FinishedSink,
    on_trace: TraceSink,
    /// The answer to a run that is paused on a block.
    resume: Mutex<Option<Resume>>,
    /// The trace of the last run of every graph scenario, by scenario id.
    traces: Mutex<HashMap<String, RunTrace>>,
    runs: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Engine {
    pub fn new(
        journal: Arc<Journal>,
        hub: Arc<StatusHub>,
        on_finished: FinishedSink,
        on_trace: TraceSink,
    ) -> Arc<Self> {
        Arc::new(Self {
            journal,
            hub,
            running: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            stop_reason: Mutex::new(None),
            thread: Mutex::new(None),
            on_finished,
            on_trace,
            resume: Mutex::new(None),
            traces: Mutex::new(HashMap::new()),
            runs: AtomicU64::new(0),
        })
    }

    /// Lets a run that is paused on a block go on. Does nothing otherwise.
    pub fn resume(&self, how: Resume) {
        if self.is_running() && self.hub.get().state == RunState::PausedBreak {
            *lock(&self.resume) = Some(how);
        }
    }

    fn take_resume(&self) -> Option<Resume> {
        lock(&self.resume).take()
    }

    /// The trace of the last run of a graph scenario, finished or still going.
    pub fn trace(&self, scenario_id: &str) -> Option<RunTrace> {
        lock(&self.traces).get(scenario_id).cloned()
    }

    pub fn clear_trace(&self, scenario_id: &str) {
        lock(&self.traces).remove(scenario_id);
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
        *lock(&self.resume) = None;
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
    /// An intercept ran its branch in the middle of a block: the block starts
    /// over. Caught by the walk that runs the block, never leaves the run.
    Restart,
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

/// An intercept block of the scenario that was started.
struct Watch {
    /// Index of the block in the graph.
    node: usize,
    condition: Condition,
    interval: Duration,
    /// When it was last looked at, or when its branch last ended.
    checked: Option<Instant>,
}

/// The trace of the graph scenario being run.
struct Trace {
    payload: RunTrace,
    /// The last transitions of the run, for the trace that stays after it.
    recent: VecDeque<TraceMove>,
    sent_at: Option<Instant>,
    dirty: bool,
}

/// What the walk was in the middle of when an intercept took over.
struct Scope {
    dir: PathBuf,
    path: Vec<usize>,
    block: Option<String>,
    scopes: Vec<String>,
    step_id: Option<String>,
    step_label: Option<String>,
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
    /// Loaded template images by their full path.
    templates: HashMap<String, Arc<Template>>,
    /// Folder the template images are read from: of the scenario of the plan,
    /// or of the scenario a block is running.
    dir: PathBuf,
    /// 1-based positions of the current step and of the blocks around it, in a list.
    path: Vec<usize>,
    /// Number and name of the current block, in a graph.
    block: Option<String>,
    /// Labels of the "Run scenario" blocks around what is being executed.
    scopes: Vec<String>,
    /// Graph of the scenario that was started, for the branches of its intercepts.
    top: Option<(Arc<Graph>, Arc<Wiring>)>,
    intercepts: Vec<Watch>,
    in_intercept: bool,
    trace: Option<Trace>,
    /// The run stops on every block, not only on breakpoints.
    stepping: bool,
    /// State to return to after a pause: running or waiting.
    base_state: RunState,
    pause: Option<PauseKind>,
    /// Whether anything was sent or slept since the flag was last cleared.
    activity: bool,
    /// Grows whenever something is sent or slept; a graph tells an idle cycle by it.
    work: u64,
    blocked_checked: Option<Instant>,
    blocked_found: Option<String>,
    /// Why the command of the block that was just run failed; the pass is
    /// about to leave that block through `error`.
    command_failure: Option<AppError>,
}

impl<'a> Runner<'a> {
    fn new(engine: &'a Engine, plan: Plan, options: RunOptions) -> Self {
        let status = RunStatus { target: Some(plan.info.clone()), ..RunStatus::default() };
        let stepping = options.debug.as_ref().is_some_and(|debug| debug.step);
        Self {
            engine,
            plan,
            options,
            status,
            input: Input::new(),
            current: 0,
            hwnd: None,
            templates: HashMap::new(),
            dir: PathBuf::new(),
            path: Vec::new(),
            block: None,
            scopes: Vec::new(),
            top: None,
            intercepts: Vec::new(),
            in_intercept: false,
            trace: None,
            stepping,
            base_state: RunState::Running,
            pause: None,
            activity: false,
            work: 0,
            blocked_checked: None,
            blocked_found: None,
            command_failure: None,
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
            // Every walk catches it; one that got here has nothing to start over.
            Err(Halt::Restart) => (Outcome::Failed, Some(AppError::new("invalid"))),
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

    /// Where the run is, for the status, the journal and messages: `2.1` in a
    /// list, `3 «Wait for Start»` in a graph, with the blocks that run this
    /// scenario in front.
    fn label(&self) -> String {
        let own = match &self.block {
            Some(block) => block.clone(),
            None => self.path.iter().map(|index| index.to_string()).collect::<Vec<_>>().join("."),
        };
        let mut parts: Vec<&str> = self.scopes.iter().map(String::as_str).collect();
        if !own.is_empty() {
            parts.push(&own);
        }
        parts.join(" › ")
    }

    fn worked(&mut self) {
        self.activity = true;
        self.work += 1;
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
        self.worked();
        self.engine.hub.flush();
        self.trace_flush();
        self.idle(ms)
    }

    fn pause_for(&mut self, pause: Pause) -> Exec<()> {
        let ms = pick_ms(pause);
        if ms > 0 {
            self.trace_phase(Phase::Pause, Some(ms));
        }
        self.sleep(ms)
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
        let result = self.run_rounds(index);
        self.close_trace(&result);
        self.intercepts.clear();
        self.top = None;
        result
    }

    fn run_rounds(&mut self, index: usize) -> Exec<()> {
        self.current = index;
        self.hwnd = None;
        self.templates.clear();
        self.dir = self.plan.items[index].dir.clone();
        self.path.clear();
        self.block = None;
        self.scopes.clear();
        self.in_intercept = false;
        let mut rounds = Self::rounds_total(self.plan.items[index].rounds);
        self.status.scenario_id = Some(self.scenario().id.clone());
        self.status.scenario_name = Some(self.scenario().name.clone());
        self.status.round = 0;
        self.status.step_id = None;
        self.status.step_label = None;
        self.status.loop_pass = None;
        self.status.loop_total = None;
        self.base_state = RunState::Running;
        self.status.state = RunState::Running;

        let graph = self.scenario().graph.clone().map(|graph| {
            let wiring = Arc::new(graph.wiring());
            (Arc::new(graph), wiring)
        });
        let mut from = None;
        let mut unknown_start = false;
        if let Some((graph, wiring)) = &graph {
            self.open_graph(graph, wiring);
            if let Some(id) = self.options.start_node.clone() {
                // A trial run from a chosen block: one pass, whatever the scenario repeats.
                from = wiring.index_of(&id);
                unknown_start = from.is_none();
                rounds = Some(1);
            }
        }
        self.status.rounds_total = rounds;
        self.push();

        if self.plan.items[index].invalid {
            return Err(Halt::Fail(AppError::new("invalid")));
        }
        if unknown_start {
            return Err(Halt::Fail(AppError::new("notFound")));
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
            match &graph {
                // Counters start from zero on every round.
                Some((graph, wiring)) => self.run_graph(graph, wiring, from)?,
                None => {
                    self.run_steps(&steps)?;
                }
            }
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
                    Err(Halt::Restart) => return Err(Halt::Restart),
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
            // Intercepts of the scenario that runs this list are looked at between its steps.
            self.watch()?;
            self.path.push(index + 1);
            let flow = loop {
                // Inside a "Run scenario" block the status keeps the id of that block.
                if self.scopes.is_empty() {
                    self.status.step_id = Some(step.id.clone());
                }
                self.status.step_label = Some(self.label());
                self.push();
                match self.run_step(step) {
                    Err(Halt::Restart) => continue,
                    other => break other,
                }
            };
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
            StepKind::Command { command, on_failure } => {
                if let Some(failure) = self.run_command(command)? {
                    match on_failure {
                        OnFailure::Stop => return Err(Halt::Fail(failure)),
                        OnFailure::Continue => self.pass_failure(&failure),
                    }
                }
            }
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
                    self.worked();
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
            | StepKind::Stop
            | StepKind::Command { .. } => {}
        }
        Ok(Attempt::Done)
    }

    // Commands

    /// Starts the command of a step or of a block and, when the command is to
    /// be waited for, waits until it ends. `Some` is a failure: the command
    /// did not start, ended with a code other than 0 or ran out of time. What
    /// a failure leads to is up to the step or to the links of the block.
    ///
    /// The guards of the input actions do not apply: a command does not need
    /// the target window in front. A stop ends the command at once.
    fn run_command(&mut self, spec: &CommandSpec) -> Exec<Option<AppError>> {
        if !self.options.allow_commands {
            return Err(self.fail("commandsDisabled"));
        }
        self.check_stop()?;
        self.worked();
        self.engine.hub.flush();
        self.trace_flush();
        let step = self.label();
        let place = [("name", self.scenario().name.clone()), ("step", step.clone())];
        let failure = |code: &str| AppError::new(code).with("step", &step);

        let mut launch = match command::start(spec) {
            Ok(launch) => launch,
            Err(message) => return Ok(Some(failure("commandStartFailed").with("message", message))),
        };
        if !spec.wait {
            launch.detach();
            self.engine.journal.add("commandStarted", &place);
            return Ok(None);
        }
        let limit = (spec.timeout_ms > 0).then(|| Duration::from_millis(spec.timeout_ms as u64));
        let started = Instant::now();
        let ended = loop {
            match launch.poll() {
                Ok(Some(code)) => break Ok(code),
                Ok(None) => {}
                Err(message) => break Err(failure("commandStartFailed").with("message", message)),
            }
            if let Err(halt) = self.check_stop() {
                launch.kill();
                return Err(halt);
            }
            if limit.is_some_and(|limit| started.elapsed() >= limit) {
                break Err(failure("commandTimeout"));
            }
            thread::sleep(SLICE);
        };
        if ended.is_err() {
            launch.kill();
        }
        let output = launch.output();
        if !output.is_empty() {
            self.engine.journal.add("commandOutput", &[place[0].clone(), place[1].clone(), ("output", output)]);
        }
        match ended {
            Ok(0) => {
                self.engine.journal.add("commandDone", &place);
                Ok(None)
            }
            Ok(code) => Ok(Some(failure("commandFailed").with("code", code))),
            Err(error) => Ok(Some(error)),
        }
    }

    /// Notes a failure of a command the scenario goes on after.
    fn pass_failure(&self, failure: &AppError) {
        self.engine.journal.add_with_reason("commandPassed", &[("name", self.scenario().name.clone())], failure);
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
        let path = self.dir.join(file);
        let key = path.to_string_lossy().into_owned();
        if let Some(template) = self.templates.get(&key) {
            return Ok(Arc::clone(template));
        }
        let template = Arc::new(Template::load(&path).map_err(|error| Halt::Fail(error.with("step", self.label())))?);
        self.templates.insert(key, Arc::clone(&template));
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
        #[cfg(test)]
        if let Some(holds) = self.options.probe.as_mut().and_then(|probe| probe(condition)) {
            return Ok(plain(holds));
        }
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
            Condition::All { items } => {
                let mut hit = None;
                for item in items {
                    match self.check(item)? {
                        Some(found) => {
                            if found.center.is_some() || hit.is_none() {
                                hit = Some(found);
                            }
                        }
                        None => return Ok(None),
                    }
                }
                Ok(if items.is_empty() { None } else { hit })
            }
            Condition::Any { items } => {
                for item in items {
                    if let Some(hit) = self.check(item)? {
                        return Ok(Some(hit));
                    }
                }
                Ok(None)
            }
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
            // An intercept is looked at on every poll, so it fires even while
            // the main flow is stuck in this wait.
            if self.watch()? {
                return Err(Halt::Restart);
            }
            self.guard()?;
            let started = Instant::now();
            self.trace_check();
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
        self.click_found(on_found, &hit)
    }

    /// What a wait does once its image is found: nothing, or a click on it.
    fn click_found(&mut self, on_found: OnFound, hit: &Hit) -> Exec<()> {
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
        self.worked();
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

    // Graph

    /// Prepares what a graph scenario needs for all of its rounds: its
    /// intercepts and a fresh trace.
    fn open_graph(&mut self, graph: &Arc<Graph>, wiring: &Arc<Wiring>) {
        self.intercepts = graph
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.enabled)
            .filter_map(|(at, node)| match &node.kind {
                NodeKind::Intercept { condition, min_interval_ms } => Some(Watch {
                    node: at,
                    condition: condition.clone(),
                    interval: Duration::from_millis((*min_interval_ms).max(0) as u64),
                    checked: None,
                }),
                _ => None,
            })
            .collect();
        self.top = Some((Arc::clone(graph), Arc::clone(wiring)));
        let scenario_id = self.scenario().id.clone();
        self.engine.clear_trace(&scenario_id);
        let run = self.engine.runs.fetch_add(1, Ordering::SeqCst) + 1;
        self.trace = Some(Trace {
            payload: RunTrace { scenario_id, run, ..RunTrace::default() },
            recent: VecDeque::new(),
            sent_at: None,
            dirty: false,
        });
    }

    /// One pass over a graph: from its start block, or from the block `from`.
    /// Counters live for one pass.
    fn run_graph(&mut self, graph: &Arc<Graph>, wiring: &Wiring, from: Option<usize>) -> Exec<()> {
        let mut at = match from {
            Some(index) => Some(index),
            None => Some(graph.start().ok_or_else(|| self.fail("invalid"))?),
        };
        let mut counters: HashMap<usize, i64> = HashMap::new();
        // Blocks passed since something was last sent or slept.
        let mut seen: HashSet<usize> = HashSet::new();
        let mut mark = self.work;
        while let Some(index) = at {
            self.check_stop()?;
            if self.watch()? {
                continue;
            }
            if self.work != mark {
                mark = self.work;
                seen.clear();
            }
            if !seen.insert(index) {
                // A cycle that sent nothing and slept nowhere gets the delay of an empty pass.
                self.idle(EMPTY_PASS_DELAY_MS)?;
                seen.clear();
                seen.insert(index);
            }
            let node = &graph.nodes[index];
            self.show_block(graph, index);
            if !matches!(node.kind, NodeKind::Start) {
                self.debug_pause(&node.id)?;
            }
            let port = loop {
                match self.run_node(node, index, &mut counters) {
                    Err(Halt::Restart) => self.show_block(graph, index),
                    other => break other?,
                }
            };
            if node.enabled {
                self.pause_for(node.pause_after)?;
            }
            at = self.follow(graph, wiring, index, port)?;
        }
        Ok(())
    }

    /// Makes the block the current one in the status and in the trace.
    fn show_block(&mut self, graph: &Graph, index: usize) {
        let node = &graph.nodes[index];
        self.block = Some(graph.label(index));
        if self.scopes.is_empty() {
            self.status.step_id = Some(node.id.clone());
        }
        self.status.step_label = Some(self.label());
        self.push();
        self.trace_enter(&node.id);
    }

    /// The block the output leads to; `None` ends the pass. A `timeout` or an
    /// `error` output without a link fails the scenario.
    fn follow(&mut self, graph: &Graph, wiring: &Wiring, from: usize, port: Port) -> Exec<Option<usize>> {
        let next = wiring.follow(from, port);
        let to = match next {
            Follow::To(index) => Some(index),
            Follow::End | Follow::Timeout | Follow::Error => None,
        };
        self.trace_move(&graph.nodes[from].id, port, to.map(|index| graph.nodes[index].id.as_str()));
        let failure = self.command_failure.take().filter(|_| port == Port::Error);
        match next {
            Follow::Timeout => Err(self.fail("conditionTimeout")),
            Follow::Error => Err(failure.map(Halt::Fail).unwrap_or_else(|| self.fail("commandFailed"))),
            Follow::To(_) | Follow::End => {
                if let Some(failure) = failure {
                    self.pass_failure(&failure);
                }
                Ok(to)
            }
        }
    }

    /// Runs one block and tells which output the pass leaves it through.
    fn run_node(&mut self, node: &Node, index: usize, counters: &mut HashMap<usize, i64>) -> Exec<Port> {
        if !node.enabled {
            return Ok(node.kind.skip_port());
        }
        if let Some(kind) = node.kind.action() {
            let step =
                Step { id: node.id.clone(), enabled: true, pause_after: Pause::default(), repeat: node.repeat, kind };
            self.trace_phase(Phase::Action, None);
            self.run_repeatable(&step)?;
            return Ok(Port::Next);
        }
        let limit = |timeout_ms: i64| (timeout_ms > 0).then_some(timeout_ms as u64);
        match &node.kind {
            NodeKind::Stop => Err(Halt::StopStep(AppError::new("stopStep").with("step", self.label()))),
            NodeKind::Wait { condition, poll_ms, timeout_ms, on_found } => {
                self.trace_phase(Phase::Check, limit(*timeout_ms));
                match self.wait_condition(condition, *poll_ms, *timeout_ms, false)? {
                    Some(hit) => {
                        self.click_found(*on_found, &hit)?;
                        Ok(Port::Next)
                    }
                    None => Ok(Port::Timeout),
                }
            }
            NodeKind::Branch { condition, poll_ms, timeout_ms } => {
                self.trace_phase(Phase::Check, limit(*timeout_ms));
                let hit = self.wait_condition(condition, *poll_ms, *timeout_ms, true)?;
                Ok(if hit.is_some() { Port::Yes } else { Port::No })
            }
            NodeKind::While { condition, until, pause_between } => {
                self.trace_phase(Phase::Check, None);
                let go = self.check(condition)?.is_some() != *until;
                if go {
                    let passed = counters.get(&index).copied().unwrap_or(0);
                    if passed > 0 {
                        self.pause_for(*pause_between)?;
                    }
                    counters.insert(index, passed + 1);
                    Ok(Port::Body)
                } else {
                    counters.remove(&index);
                    Ok(Port::Done)
                }
            }
            NodeKind::Focus { target } => {
                self.trace_phase(Phase::Action, None);
                let hwnd = target::find_target(target).ok_or_else(|| {
                    Halt::Fail(
                        AppError::new("targetNotFound")
                            .with("target", target::describe_target(target))
                            .with("step", self.label()),
                    )
                })?;
                target::request_focus(hwnd);
                self.worked();
                Ok(Port::Next)
            }
            NodeKind::Counter { times, pause_between } => {
                let passed = counters.get(&index).copied().unwrap_or(0);
                match counter_port(*times, passed) {
                    Port::Body => {
                        // No pause before the first pass and none after the last one.
                        if passed > 0 {
                            self.pause_for(*pause_between)?;
                        }
                        counters.insert(index, passed + 1);
                        self.count(&node.id, Some((passed + 1, *times)));
                        Ok(Port::Body)
                    }
                    _ => {
                        // Entered again later, the counter starts from zero.
                        counters.remove(&index);
                        self.count(&node.id, None);
                        Ok(Port::Done)
                    }
                }
            }
            NodeKind::Call { scenario_id } => {
                self.run_call_block(node, scenario_id)?;
                Ok(Port::Next)
            }
            NodeKind::Command { command } => {
                self.trace_phase(Phase::Action, limit(command.timeout_ms));
                Ok(match self.run_command(command)? {
                    None => Port::Next,
                    Some(failure) => {
                        self.command_failure = Some(failure);
                        Port::Error
                    }
                })
            }
            // A start block only passes on. An intercept is entered through its
            // branch, never along a link; the actions were handled above.
            NodeKind::Start
            | NodeKind::Intercept { .. }
            | NodeKind::Click { .. }
            | NodeKind::Move { .. }
            | NodeKind::Drag { .. }
            | NodeKind::Scroll { .. }
            | NodeKind::Key { .. }
            | NodeKind::Text { .. }
            | NodeKind::Pause { .. } => Ok(Port::Next),
        }
    }

    /// Shows the progress of a counter that is in its body; `None` takes it away.
    fn count(&mut self, id: &str, progress: Option<(i64, i64)>) {
        if !self.scopes.is_empty() {
            return;
        }
        self.status.loop_pass = progress.map(|(passed, _)| passed);
        self.status.loop_total = progress.map(|(_, total)| total);
        self.push();
        if let Some(trace) = self.trace.as_mut() {
            match progress {
                Some(progress) => trace.payload.counters.insert(id.to_string(), progress),
                None => trace.payload.counters.remove(id),
            };
            self.trace_send(false);
        }
    }

    // Run scenario

    /// A "Run scenario" block with its own repeat.
    fn run_call_block(&mut self, node: &Node, callee_id: &str) -> Exec<()> {
        let times = node.repeat.map_or(1, |repeat| repeat.times);
        let mut result = Ok(());
        for pass in 1..=times {
            if times > 1 {
                self.status.step_repeat = Some(pass);
                self.status.step_repeat_total = Some(times);
                self.push();
            }
            result = self.run_call(callee_id).and_then(|()| match node.repeat {
                Some(repeat) if pass < times => self.pause_for(repeat.pause_between),
                _ => Ok(()),
            });
            if result.is_err() {
                break;
            }
        }
        self.status.step_repeat = None;
        self.status.step_repeat_total = None;
        result
    }

    /// One pass of another scenario, in the window and the mode of this run.
    /// Its own repeat and pause between rounds do not apply. A "Stop" block
    /// and a failure inside end the whole run.
    fn run_call(&mut self, callee_id: &str) -> Exec<()> {
        let Some(callee) = self.plan.callees.get(callee_id).cloned() else {
            return Err(self.fail("callMissing"));
        };
        if self.scopes.len() >= MAX_CALL_DEPTH {
            return Err(self.fail("callTooDeep"));
        }
        self.trace_phase(Phase::Action, None);
        let dir = std::mem::replace(&mut self.dir, callee.dir.clone());
        let path = std::mem::take(&mut self.path);
        let block = self.block.take();
        self.scopes.push(block.clone().unwrap_or_default());
        let result = match &callee.graph {
            Some((graph, wiring)) => self.run_graph(graph, wiring, None),
            None => self.run_steps(&callee.scenario.steps).map(|_| ()),
        };
        self.scopes.pop();
        self.dir = dir;
        self.path = path;
        self.block = block;
        self.status.step_label = Some(self.label());
        self.push();
        result
    }

    // Intercepts

    /// Looks at the intercepts that are due. Returns whether one of them fired
    /// and ran its branch; the block that was going on starts over then.
    /// While a branch runs, the intercepts are not looked at.
    fn watch(&mut self) -> Exec<bool> {
        if self.in_intercept || self.intercepts.is_empty() {
            return Ok(false);
        }
        let Some((graph, wiring)) = self.top.clone() else {
            return Ok(false);
        };
        for at in 0..self.intercepts.len() {
            let watch = &self.intercepts[at];
            if watch.checked.is_some_and(|checked| checked.elapsed() < watch.interval) {
                continue;
            }
            let (node, condition) = (watch.node, watch.condition.clone());
            // An intercept belongs to the scenario that was started, whatever
            // that scenario is running now.
            let scope = self.enter_top();
            let fired = self.guard().and_then(|_| self.check(&condition));
            self.intercepts[at].checked = Some(Instant::now());
            let result = match fired {
                Ok(Some(_)) => self.run_intercept(&graph, &wiring, node).map(|()| true),
                Ok(None) => Ok(false),
                Err(halt) => Err(halt),
            };
            // The interval counts from the end of the branch.
            self.intercepts[at].checked = Some(Instant::now());
            self.leave_top(scope);
            if !matches!(result, Ok(false)) {
                return result;
            }
        }
        Ok(false)
    }

    fn enter_top(&mut self) -> Scope {
        let dir = self.plan.items[self.current].dir.clone();
        Scope {
            dir: std::mem::replace(&mut self.dir, dir),
            path: std::mem::take(&mut self.path),
            block: self.block.take(),
            scopes: std::mem::take(&mut self.scopes),
            step_id: self.status.step_id.clone(),
            step_label: self.status.step_label.clone(),
        }
    }

    fn leave_top(&mut self, scope: Scope) {
        self.dir = scope.dir;
        self.path = scope.path;
        self.block = scope.block;
        self.scopes = scope.scopes;
        self.status.step_id = scope.step_id;
        self.status.step_label = scope.step_label;
        self.push();
    }

    /// Runs the branch of an intercept that fired, up to an output without a link.
    fn run_intercept(&mut self, graph: &Arc<Graph>, wiring: &Wiring, node: usize) -> Exec<()> {
        let name = ("name", self.scenario().name.clone());
        self.engine.journal.add("interceptFired", &[name, ("step", graph.label(node))]);
        let outer = (self.base_state, self.status.state);
        self.in_intercept = true;
        self.base_state = RunState::Running;
        self.set_state(RunState::Running);
        self.show_block(graph, node);
        let result = self.follow(graph, wiring, node, Port::Next).and_then(|first| match first {
            Some(first) => self.run_graph(graph, wiring, Some(first)),
            None => Ok(()),
        });
        self.in_intercept = false;
        self.base_state = outer.0;
        if result.is_ok() {
            self.set_state(outer.1);
        }
        result
    }

    // Breakpoints

    /// Stops on a breakpoint, or on every block of a step-by-step run, until
    /// the user asks for one step or to go on. Blocks of the scenarios run
    /// through "Run scenario" are not stopped on.
    fn debug_pause(&mut self, id: &str) -> Exec<()> {
        if !self.scopes.is_empty() {
            return Ok(());
        }
        let Some(debug) = &self.options.debug else {
            return Ok(());
        };
        if !self.stepping && !debug.breakpoints.contains(id) {
            return Ok(());
        }
        self.input.release_all();
        // An answer left from an earlier pause does not count.
        self.engine.take_resume();
        self.status.state = RunState::PausedBreak;
        self.push();
        self.engine.hub.flush();
        self.trace_flush();
        let how = loop {
            if let Some(how) = self.engine.take_resume() {
                break how;
            }
            self.idle(PAUSE_POLL_MS / 2)?;
        };
        self.stepping = how == Resume::Step;
        self.status.state = self.base_state;
        self.push();
        // "Step" pressed in the ClickIt window took the focus from the target.
        if let Some(hwnd) = self.hwnd {
            target::request_focus(hwnd);
        }
        Ok(())
    }

    // Trace

    /// The trace follows the blocks of the scenario that was started, not of
    /// the ones it runs.
    fn tracing(&mut self) -> Option<&mut Trace> {
        if self.scopes.is_empty() {
            self.trace.as_mut()
        } else {
            None
        }
    }

    fn trace_enter(&mut self, id: &str) {
        let Some(trace) = self.tracing() else {
            return;
        };
        trace.payload.node = Some(id.to_string());
        trace.payload.phase = None;
        trace.payload.phase_ms = None;
        trace.payload.checks = 0;
        *trace.payload.node_passes.entry(id.to_string()).or_insert(0) += 1;
        self.trace_send(false);
    }

    fn trace_phase(&mut self, phase: Phase, ms: Option<u64>) {
        let Some(trace) = self.tracing() else {
            return;
        };
        trace.payload.phase = Some(phase);
        trace.payload.phase_ms = ms;
        self.trace_send(false);
    }

    fn trace_check(&mut self) {
        let Some(trace) = self.tracing() else {
            return;
        };
        trace.payload.checks += 1;
        self.trace_send(false);
    }

    fn trace_move(&mut self, from: &str, port: Port, to: Option<&str>) {
        let Some(trace) = self.tracing() else {
            return;
        };
        let step = TraceMove { from: from.to_string(), port, to: to.map(str::to_string) };
        *trace.payload.edge_passes.entry(format!("{from}:{}", port.as_str())).or_insert(0) += 1;
        trace.payload.outcomes.insert(from.to_string(), port);
        if trace.payload.moves.len() >= TRACE_MOVES {
            trace.payload.moves.remove(0);
        }
        trace.payload.moves.push(step.clone());
        if trace.recent.len() >= TRACE_KEPT_MOVES {
            trace.recent.pop_front();
        }
        trace.recent.push_back(step);
        self.trace_send(false);
    }

    /// Sends the trace when enough time has passed since the last one; `force` sends at once.
    fn trace_send(&mut self, force: bool) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        if !force && trace.sent_at.is_some_and(|at| at.elapsed() < TRACE_INTERVAL) {
            trace.dirty = true;
            return;
        }
        (self.engine.on_trace)(&trace.payload);
        // A window opened in the middle of the run and a tool ask for this copy.
        let mut kept = trace.payload.clone();
        kept.moves = trace.recent.iter().cloned().collect();
        lock(&self.engine.traces).insert(kept.scenario_id.clone(), kept);
        trace.payload.moves.clear();
        trace.sent_at = Some(Instant::now());
        trace.dirty = false;
    }

    /// Sends what is pending. Called before the run goes to sleep.
    fn trace_flush(&mut self) {
        if self.trace.as_ref().is_some_and(|trace| trace.dirty) {
            self.trace_send(true);
        }
    }

    /// Sends the last event of the run and leaves the trace in memory.
    fn close_trace(&mut self, result: &Exec<()>) {
        let Some(mut trace) = self.trace.take() else {
            return;
        };
        let (outcome, reason) = match result {
            Ok(()) => (Outcome::Completed, None),
            Err(Halt::Stop(reason)) | Err(Halt::StopStep(reason)) => (Outcome::Stopped, Some(reason.clone())),
            Err(Halt::Fail(reason)) => (Outcome::Failed, Some(reason.clone())),
            Err(Halt::Restart) => (Outcome::Failed, None),
        };
        trace.payload.phase = None;
        trace.payload.phase_ms = None;
        trace.payload.counters.clear();
        trace.payload.stop = Some(TraceStop { node: trace.payload.node.clone(), outcome, reason });
        (self.engine.on_trace)(&trace.payload);
        trace.payload.moves = trace.recent.into_iter().collect();
        lock(&self.engine.traces).insert(trace.payload.scenario_id.clone(), trace.payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::graph::Edge;
    use crate::model::scenario::{CommandRun, ShellKind};
    use crate::runtime::JournalEntry;
    use std::sync::mpsc::{self, Receiver};

    const WAIT: Duration = Duration::from_secs(10);

    /// An engine that writes into a temporary folder and hands out what it reports.
    struct Bench {
        engine: Arc<Engine>,
        hub: Arc<StatusHub>,
        journal: Arc<Journal>,
        finished: Receiver<RunFinished>,
        traces: Arc<Mutex<Vec<RunTrace>>>,
        dir: PathBuf,
    }

    impl Bench {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("clickit-engine-{}", crate::model::scenario::new_id("t")));
            std::fs::create_dir_all(&dir).unwrap();
            let journal =
                Arc::new(Journal::start(dir.join("journal.jsonl"), dir.join("journal.prev.jsonl"), Box::new(|| {})));
            let hub = Arc::new(StatusHub::new(Box::new(|_| {})));
            let (tx, finished) = mpsc::channel();
            let tx = Mutex::new(tx);
            let traces = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::clone(&traces);
            let engine = Engine::new(
                Arc::clone(&journal),
                Arc::clone(&hub),
                Box::new(move |finished| {
                    let _ = lock(&tx).send(finished.clone());
                }),
                Box::new(move |trace| lock(&seen).push(trace.clone())),
            );
            Self { engine, hub, journal, finished, traces, dir }
        }

        fn plan(&self, scenario: Scenario, callees: Vec<Scenario>) -> Plan {
            let info = TargetInfo { kind: TargetKind::Scenario, id: scenario.id.clone(), name: scenario.name.clone() };
            let item = PlanItem { dir: self.dir.clone(), rounds: scenario.repeat, invalid: false, scenario };
            let callees = callees
                .into_iter()
                .map(|callee| (callee.id.clone(), Arc::new(Callee::new(callee, self.dir.clone()))))
                .collect();
            Plan { info, items: vec![item], queue: None, callees }
        }

        fn start(&self, scenario: Scenario, callees: Vec<Scenario>, options: RunOptions) {
            self.engine.start(self.plan(scenario, callees), options).unwrap();
        }

        fn end(&self) -> RunFinished {
            let finished = self.finished.recv_timeout(WAIT).expect("the run did not end");
            self.engine.join();
            finished
        }

        fn run(&self, scenario: Scenario, callees: Vec<Scenario>, options: RunOptions) -> RunFinished {
            self.start(scenario, callees, options);
            self.end()
        }

        /// Waits until the run stops on a block and tells which one.
        fn paused_on(&self) -> String {
            let deadline = Instant::now() + WAIT;
            loop {
                let status = self.hub.get();
                if status.state == RunState::PausedBreak {
                    return status.step_id.unwrap_or_default();
                }
                assert!(Instant::now() < deadline, "the run did not pause");
                thread::sleep(Duration::from_millis(5));
            }
        }

        fn journal(&self, code: &str) -> Vec<JournalEntry> {
            self.journal.read().into_iter().filter(|entry| entry.code == code).collect()
        }

        fn trace(&self, scenario: &str) -> RunTrace {
            self.engine.trace(scenario).expect("no trace")
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn node(id: &str, kind: NodeKind) -> Node {
        let mut node = Node::new(kind);
        node.id = id.to_string();
        node
    }

    fn edge(from: &str, port: Port, to: &str) -> Edge {
        Edge { from: from.to_string(), port, to: to.to_string(), points: Vec::new() }
    }

    /// A block that does something without touching the mouse or the keyboard.
    fn tick(id: &str) -> Node {
        node(id, NodeKind::Pause { duration: Pause::exact(1) })
    }

    fn named(name: &str) -> Condition {
        Condition::Process { name: name.to_string() }
    }

    fn branch(id: &str, name: &str) -> Node {
        node(id, NodeKind::Branch { condition: named(name), poll_ms: 5, timeout_ms: 0 })
    }

    fn wait(id: &str, name: &str, timeout_ms: i64) -> Node {
        node(id, NodeKind::Wait { condition: named(name), poll_ms: 5, timeout_ms, on_found: OnFound::None })
    }

    fn canvas(id: &str, nodes: Vec<Node>, edges: Vec<Edge>) -> Scenario {
        let mut scenario = Scenario::new_canvas(id);
        scenario.id = id.to_string();
        scenario.graph = Some(Graph { nodes, edges, notes: Vec::new() });
        scenario
    }

    fn options() -> RunOptions {
        RunOptions::new(Source::Schedule)
    }

    /// Conditions are answered by the name of the process they look for.
    fn probed(answer: impl FnMut(&str) -> bool + Send + 'static) -> RunOptions {
        let mut answer = answer;
        let mut options = options();
        options.probe = Some(Box::new(move |condition| match condition {
            Condition::Process { name } => Some(answer(name)),
            _ => Some(false),
        }));
        options
    }

    fn passes(trace: &RunTrace, id: &str) -> u64 {
        trace.node_passes.get(id).copied().unwrap_or(0)
    }

    #[test]
    fn a_while_block_runs_its_body_until_the_condition_fails() {
        let bench = Bench::new();
        let scenario = canvas(
            "until-stop",
            vec![
                node("s", NodeKind::Start),
                node(
                    "w",
                    NodeKind::While {
                        condition: named("go"),
                        until: false,
                        pause_between: Pause::default(),
                    },
                ),
                tick("body"),
                tick("after"),
            ],
            vec![
                edge("s", Port::Next, "w"),
                edge("w", Port::Body, "body"),
                edge("body", Port::Next, "w"),
                edge("w", Port::Done, "after"),
            ],
        );
        let mut left = 2;
        let mut options = options();
        options.probe = Some(Box::new(move |condition| match condition {
            Condition::Process { name } if name == "go" => {
                let holds = left > 0;
                left -= 1;
                Some(holds)
            }
            _ => Some(false),
        }));
        let finished = bench.run(scenario, vec![], options);
        assert_eq!(finished.outcome, Outcome::Completed);
        let trace = bench.trace("until-stop");
        assert_eq!(passes(&trace, "body"), 2);
        assert_eq!(passes(&trace, "w"), 3);
        assert_eq!(trace.outcomes.get("w"), Some(&Port::Done));
        assert_eq!(passes(&trace, "after"), 1);
    }

    #[test]
    fn an_until_block_leaves_when_the_condition_starts_holding() {
        let bench = Bench::new();
        let scenario = canvas(
            "until-hold",
            vec![
                node("s", NodeKind::Start),
                node(
                    "w",
                    NodeKind::While { condition: named("go"), until: true, pause_between: Pause::default() },
                ),
                tick("body"),
                tick("after"),
            ],
            vec![
                edge("s", Port::Next, "w"),
                edge("w", Port::Body, "body"),
                edge("body", Port::Next, "w"),
                edge("w", Port::Done, "after"),
            ],
        );
        let finished = bench.run(scenario, vec![], probed(|_| true));
        assert_eq!(finished.outcome, Outcome::Completed);
        let trace = bench.trace("until-hold");
        assert_eq!(passes(&trace, "body"), 0);
        assert_eq!(passes(&trace, "after"), 1);
        assert_eq!(trace.outcomes.get("w"), Some(&Port::Done));
    }

    #[test]
    fn an_and_does_not_look_at_the_second_check_when_the_first_fails() {
        let bench = Bench::new();
        let scenario = canvas(
            "both",
            vec![
                node("s", NodeKind::Start),
                node(
                    "b",
                    NodeKind::Branch {
                        condition: Condition::All { items: vec![named("first"), named("second")] },
                        poll_ms: 5,
                        timeout_ms: 0,
                    },
                ),
                tick("yes"),
            ],
            vec![edge("s", Port::Next, "b"), edge("b", Port::Yes, "yes"), edge("b", Port::No, "yes")],
        );
        let looked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = std::sync::Arc::clone(&looked);
        let mut options = options();
        options.probe = Some(Box::new(move |condition| match condition {
            Condition::Process { name } => {
                seen.lock().unwrap().push(name.clone());
                Some(false)
            }
            Condition::All { .. } | Condition::Any { .. } => None,
            _ => Some(false),
        }));
        bench.run(scenario, vec![], options);
        assert_eq!(looked.lock().unwrap().as_slice(), ["first"]);
    }

    #[test]
    fn a_counter_runs_its_body_the_set_number_of_times_and_then_leaves() {
        let bench = Bench::new();
        let scenario = canvas(
            "count",
            vec![
                node("s", NodeKind::Start),
                node("c", NodeKind::Counter { times: 3, pause_between: Pause::default() }),
                tick("body"),
                tick("after"),
            ],
            vec![
                edge("s", Port::Next, "c"),
                edge("c", Port::Body, "body"),
                edge("body", Port::Next, "c"),
                edge("c", Port::Done, "after"),
            ],
        );
        let finished = bench.run(scenario, vec![], options());
        assert_eq!(finished.outcome, Outcome::Completed);
        let trace = bench.trace("count");
        assert_eq!(passes(&trace, "body"), 3);
        assert_eq!(passes(&trace, "c"), 4);
        assert_eq!(passes(&trace, "after"), 1);
        assert_eq!(trace.edge_passes.get("c:body"), Some(&3));
        assert_eq!(trace.edge_passes.get("c:done"), Some(&1));
        assert_eq!(trace.outcomes.get("c"), Some(&Port::Done));
        assert!(trace.counters.is_empty());
        let stop = trace.stop.expect("the trace of a finished run says how it ended");
        assert_eq!(stop.outcome, Outcome::Completed);
        assert_eq!(stop.node.as_deref(), Some("after"));
        // The window got the trace while the run was going, and the last event is the end.
        let sent = lock(&bench.traces);
        assert!(sent.len() >= 2);
        assert!(sent.last().unwrap().stop.is_some());
        assert!(sent.iter().all(|event| event.scenario_id == "count" && event.run == trace.run));
    }

    #[test]
    fn a_condition_sends_the_pass_through_yes_or_no() {
        let graph = || {
            canvas(
                "fork",
                vec![node("s", NodeKind::Start), branch("b", "game.exe"), tick("yes"), tick("no")],
                vec![edge("s", Port::Next, "b"), edge("b", Port::Yes, "yes"), edge("b", Port::No, "no")],
            )
        };
        let bench = Bench::new();
        bench.run(graph(), vec![], probed(|_| true));
        let trace = bench.trace("fork");
        assert_eq!((passes(&trace, "yes"), passes(&trace, "no")), (1, 0));
        assert_eq!(trace.outcomes.get("b"), Some(&Port::Yes));

        bench.run(graph(), vec![], probed(|_| false));
        let second = bench.trace("fork");
        assert_eq!((passes(&second, "yes"), passes(&second, "no")), (0, 1));
        assert!(second.run > trace.run);
    }

    #[test]
    fn a_wait_without_a_timeout_link_fails_and_a_linked_one_goes_on() {
        let bench = Bench::new();
        let bare = canvas(
            "bare",
            vec![node("s", NodeKind::Start), wait("w", "game.exe", 20), tick("found")],
            vec![edge("s", Port::Next, "w"), edge("w", Port::Next, "found")],
        );
        let finished = bench.run(bare, vec![], probed(|_| false));
        assert_eq!(finished.outcome, Outcome::Failed);
        assert_eq!(finished.reason.unwrap().code, "conditionTimeout");
        let trace = bench.trace("bare");
        assert_eq!(passes(&trace, "found"), 0);
        assert_eq!(trace.stop.unwrap().node.as_deref(), Some("w"));

        let linked = canvas(
            "linked",
            vec![node("s", NodeKind::Start), wait("w", "game.exe", 20), tick("found"), tick("late")],
            vec![edge("s", Port::Next, "w"), edge("w", Port::Next, "found"), edge("w", Port::Timeout, "late")],
        );
        let finished = bench.run(linked, vec![], probed(|_| false));
        assert_eq!(finished.outcome, Outcome::Completed);
        let trace = bench.trace("linked");
        assert_eq!((passes(&trace, "found"), passes(&trace, "late")), (0, 1));
    }

    #[test]
    fn a_disabled_block_is_passed_through_and_a_stop_block_ends_the_run() {
        let bench = Bench::new();
        let mut off = branch("b", "game.exe");
        off.enabled = false;
        let scenario = canvas(
            "skip",
            vec![node("s", NodeKind::Start), off, tick("yes"), node("stop", NodeKind::Stop), tick("never")],
            vec![
                edge("s", Port::Next, "b"),
                edge("b", Port::Yes, "yes"),
                edge("b", Port::No, "stop"),
                edge("stop", Port::Next, "never"),
            ],
        );
        // The condition would hold, but a switched-off condition is left through "no".
        let finished = bench.run(scenario, vec![], probed(|_| true));
        assert_eq!(finished.outcome, Outcome::Stopped);
        assert_eq!(finished.reason.unwrap().code, "stopStep");
        let trace = bench.trace("skip");
        assert_eq!((passes(&trace, "yes"), passes(&trace, "never")), (0, 0));
    }

    #[test]
    fn a_start_from_a_block_makes_one_pass_from_it() {
        let bench = Bench::new();
        let mut scenario = canvas(
            "from",
            vec![node("s", NodeKind::Start), tick("a"), tick("b"), tick("c")],
            vec![edge("s", Port::Next, "a"), edge("a", Port::Next, "b"), edge("b", Port::Next, "c")],
        );
        scenario.repeat = RepeatMode::Count { times: 5 };
        let mut from_b = options();
        from_b.start_node = Some("b".to_string());
        let finished = bench.run(scenario.clone(), vec![], from_b);
        assert_eq!(finished.outcome, Outcome::Completed);
        let trace = bench.trace("from");
        assert_eq!((passes(&trace, "a"), passes(&trace, "b"), passes(&trace, "c")), (0, 1, 1));

        let mut unknown = options();
        unknown.start_node = Some("nope".to_string());
        let finished = bench.run(scenario, vec![], unknown);
        assert_eq!(finished.outcome, Outcome::Failed);
        assert_eq!(finished.reason.unwrap().code, "notFound");
    }

    #[test]
    fn a_breakpoint_pauses_the_run_until_a_step_or_continue() {
        let bench = Bench::new();
        let scenario = canvas(
            "debug",
            vec![node("s", NodeKind::Start), tick("a"), tick("b"), tick("c"), tick("d")],
            vec![
                edge("s", Port::Next, "a"),
                edge("a", Port::Next, "b"),
                edge("b", Port::Next, "c"),
                edge("c", Port::Next, "d"),
            ],
        );
        let mut with_break = options();
        with_break.debug = Some(Debug { breakpoints: HashSet::from(["b".to_string()]), step: false });
        bench.start(scenario.clone(), vec![], with_break);
        assert_eq!(bench.paused_on(), "b");
        // The block the run stands on has not been run yet.
        thread::sleep(Duration::from_millis(40));
        assert_eq!(bench.hub.get().state, RunState::PausedBreak);
        // One step runs "b" and stops on the next block, which has no breakpoint.
        bench.engine.resume(Resume::Step);
        let deadline = Instant::now() + WAIT;
        while bench.hub.get().step_id.as_deref() != Some("c") || bench.hub.get().state != RunState::PausedBreak {
            assert!(Instant::now() < deadline, "the step did not stop on the next block");
            thread::sleep(Duration::from_millis(5));
        }
        bench.engine.resume(Resume::Continue);
        assert_eq!(bench.end().outcome, Outcome::Completed);
        assert_eq!(passes(&bench.trace("debug"), "d"), 1);

        // A step-by-step run stops on the first block after the start; a stop ends it there.
        let mut by_step = options();
        by_step.debug = Some(Debug { breakpoints: HashSet::new(), step: true });
        bench.start(scenario, vec![], by_step);
        assert_eq!(bench.paused_on(), "a");
        bench.engine.stop(AppError::new("userStop"));
        let finished = bench.end();
        assert_eq!(finished.outcome, Outcome::Stopped);
        assert_eq!(passes(&bench.trace("debug"), "b"), 0);
    }

    #[test]
    fn a_resume_is_ignored_when_the_run_is_not_paused_on_a_block() {
        let bench = Bench::new();
        bench.engine.resume(Resume::Continue);
        assert!(bench.engine.take_resume().is_none());
    }

    #[test]
    fn a_call_block_runs_a_list_and_a_graph_once_whatever_they_repeat() {
        let bench = Bench::new();
        let mut list = Scenario::new("list");
        list.id = "list".to_string();
        list.repeat = RepeatMode::Forever;
        list.steps = vec![Step::new(StepKind::Pause { duration: Pause::exact(1) })];
        let mut inner = canvas(
            "inner",
            vec![node("s", NodeKind::Start), tick("x"), node("call", NodeKind::Call { scenario_id: "list".into() })],
            vec![edge("s", Port::Next, "x"), edge("x", Port::Next, "call")],
        );
        inner.repeat = RepeatMode::Count { times: 9 };
        let mut twice = node("run", NodeKind::Call { scenario_id: "inner".into() });
        twice.repeat = Some(crate::model::scenario::Repeat { times: 2, pause_between: Pause::default() });
        let outer = canvas(
            "outer",
            vec![node("s", NodeKind::Start), twice, tick("after")],
            vec![edge("s", Port::Next, "run"), edge("run", Port::Next, "after")],
        );
        let finished = bench.run(outer, vec![inner, list], options());
        assert_eq!(finished.outcome, Outcome::Completed);
        let trace = bench.trace("outer");
        // The trace follows the blocks of the scenario that was started only.
        assert_eq!((passes(&trace, "run"), passes(&trace, "after"), passes(&trace, "x")), (1, 1, 0));
        assert!(bench.engine.trace("inner").is_none());
    }

    #[test]
    fn a_missing_scenario_and_a_stop_inside_a_call_end_the_whole_run() {
        let bench = Bench::new();
        let outer = || {
            canvas(
                "outer",
                vec![node("s", NodeKind::Start), node("run", NodeKind::Call { scenario_id: "inner".into() }), tick("after")],
                vec![edge("s", Port::Next, "run"), edge("run", Port::Next, "after")],
            )
        };
        let finished = bench.run(outer(), vec![], options());
        assert_eq!(finished.outcome, Outcome::Failed);
        assert_eq!(finished.reason.unwrap().code, "callMissing");

        let inner = canvas(
            "inner",
            vec![node("s", NodeKind::Start), node("halt", NodeKind::Stop)],
            vec![edge("s", Port::Next, "halt")],
        );
        let finished = bench.run(outer(), vec![inner], options());
        assert_eq!(finished.outcome, Outcome::Stopped);
        let reason = finished.reason.unwrap();
        assert_eq!(reason.code, "stopStep");
        // The message names the block of the started scenario and the block inside.
        assert!(reason.params.get("step").is_some_and(|step| step.contains(" › ")));
        assert_eq!(passes(&bench.trace("outer"), "after"), 0);
    }

    #[test]
    fn an_intercept_runs_its_branch_and_the_interrupted_block_starts_over() {
        let bench = Bench::new();
        let scenario = canvas(
            "watch",
            vec![
                node("s", NodeKind::Start),
                wait("w", "game.exe", 0),
                tick("done"),
                node("i", NodeKind::Intercept { condition: named("popup.exe"), min_interval_ms: 500 }),
                tick("close"),
            ],
            vec![edge("s", Port::Next, "w"), edge("w", Port::Next, "done"), edge("i", Port::Next, "close")],
        );
        // The popup shows up once; the game appears only after the popup was closed.
        let mut popup_seen = false;
        let mut waited = 0;
        let options = probed(move |name| match name {
            "popup.exe" => !std::mem::replace(&mut popup_seen, true),
            _ => {
                waited += 1;
                waited > 3
            }
        });
        let finished = bench.run(scenario, vec![], options);
        assert_eq!(finished.outcome, Outcome::Completed);
        let trace = bench.trace("watch");
        assert_eq!(passes(&trace, "close"), 1);
        assert_eq!(passes(&trace, "done"), 1);
        assert_eq!(trace.edge_passes.get("i:next"), Some(&1));
        let fired = bench.journal("interceptFired");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].params.get("name").map(String::as_str), Some("watch"));
    }

    #[test]
    fn an_intercept_is_not_looked_at_more_often_than_its_interval() {
        let bench = Bench::new();
        let scenario = canvas(
            "interval",
            vec![
                node("s", NodeKind::Start),
                wait("w", "game.exe", 300),
                tick("late"),
                node("i", NodeKind::Intercept { condition: named("popup.exe"), min_interval_ms: 100_000 }),
                tick("close"),
            ],
            vec![edge("s", Port::Next, "w"), edge("w", Port::Timeout, "late"), edge("i", Port::Next, "close")],
        );
        let looks = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&looks);
        let options = probed(move |name| {
            if name == "popup.exe" {
                counted.fetch_add(1, Ordering::SeqCst);
            }
            false
        });
        let finished = bench.run(scenario, vec![], options);
        assert_eq!(finished.outcome, Outcome::Completed);
        // The wait polled dozens of times; the intercept was looked at once.
        assert_eq!(looks.load(Ordering::SeqCst), 1);
        assert_eq!(passes(&bench.trace("interval"), "close"), 0);
    }

    #[test]
    fn a_cycle_that_does_nothing_does_not_spin_and_stops_on_request() {
        let bench = Bench::new();
        let scenario = canvas(
            "spin",
            vec![node("s", NodeKind::Start), branch("b", "game.exe")],
            vec![edge("s", Port::Next, "b"), edge("b", Port::No, "b")],
        );
        bench.start(scenario, vec![], probed(|_| false));
        thread::sleep(Duration::from_millis(200));
        bench.engine.stop(AppError::new("userStop"));
        let finished = bench.end();
        assert_eq!(finished.outcome, Outcome::Stopped);
        // Every lap sleeps at least the delay of an empty pass.
        assert!(passes(&bench.trace("spin"), "b") <= 200 / EMPTY_PASS_DELAY_MS + 5);
    }

    #[test]
    fn a_list_scenario_leaves_no_trace() {
        let bench = Bench::new();
        let mut list = Scenario::new("plain");
        list.id = "plain".to_string();
        list.steps = vec![Step::new(StepKind::Pause { duration: Pause::exact(1) })];
        assert_eq!(bench.run(list, vec![], options()).outcome, Outcome::Completed);
        assert!(bench.engine.trace("plain").is_none());
        assert!(lock(&bench.traces).is_empty());
    }

    // Commands. The scripts are run by cmd without a window.

    fn script(text: &str, timeout_ms: i64) -> CommandSpec {
        CommandSpec {
            run: CommandRun::Script { shell: ShellKind::Cmd, text: text.to_string() },
            work_dir: String::new(),
            wait: true,
            timeout_ms,
            hidden: true,
        }
    }

    fn command_step(command: CommandSpec, on_failure: OnFailure) -> Step {
        Step::new(StepKind::Command { command, on_failure })
    }

    fn list(id: &str, steps: Vec<Step>) -> Scenario {
        let mut scenario = Scenario::new(id);
        scenario.id = id.to_string();
        scenario.steps = steps;
        scenario
    }

    /// A run that may start commands.
    fn allowed() -> RunOptions {
        let mut options = options();
        options.allow_commands = true;
        options
    }

    fn param<'a>(entry: &'a JournalEntry, key: &str) -> Option<&'a str> {
        entry.params.get(key).map(String::as_str)
    }

    #[test]
    fn a_command_is_not_started_while_commands_are_switched_off() {
        let bench = Bench::new();
        let marker = bench.dir.join("ran.txt");
        let mut command = script("echo 1>ran.txt", 0);
        command.work_dir = bench.dir.to_string_lossy().into_owned();
        let steps = vec![command_step(command, OnFailure::Continue)];
        let finished = bench.run(list("off", steps), vec![], options());
        assert_eq!(finished.outcome, Outcome::Failed);
        assert_eq!(finished.reason.unwrap().code, "commandsDisabled");
        assert!(!marker.exists());
    }

    #[test]
    fn a_code_other_than_zero_fails_a_list_unless_the_step_goes_on() {
        let bench = Bench::new();
        let strict = list("strict", vec![command_step(script("exit /b 3", 0), OnFailure::Stop)]);
        let finished = bench.run(strict, vec![], allowed());
        assert_eq!(finished.outcome, Outcome::Failed);
        let reason = finished.reason.unwrap();
        assert_eq!(reason.code, "commandFailed");
        assert_eq!(reason.params.get("code").map(String::as_str), Some("3"));
        assert_eq!(reason.params.get("step").map(String::as_str), Some("1"));

        let lenient = list(
            "lenient",
            vec![
                command_step(script("exit /b 3", 0), OnFailure::Continue),
                command_step(script("exit /b 0", 0), OnFailure::Stop),
            ],
        );
        let finished = bench.run(lenient, vec![], allowed());
        assert_eq!(finished.outcome, Outcome::Completed);
        let passed = bench.journal("commandPassed");
        assert_eq!(passed.len(), 1);
        assert_eq!(param(&passed[0], "reason"), Some("commandFailed"));
        assert_eq!(param(&passed[0], "code"), Some("3"));
        assert_eq!(param(&passed[0], "step"), Some("1"));
        let done = bench.journal("commandDone");
        assert_eq!(done.len(), 1);
        assert_eq!(param(&done[0], "step"), Some("2"));
    }

    #[test]
    fn what_a_hidden_command_prints_goes_to_the_journal() {
        let bench = Bench::new();
        let steps = vec![command_step(script("echo Привет, мир", 0), OnFailure::Stop)];
        let finished = bench.run(list("talk", steps), vec![], allowed());
        assert_eq!(finished.outcome, Outcome::Completed);
        let printed = bench.journal("commandOutput");
        assert_eq!(printed.len(), 1);
        assert_eq!(param(&printed[0], "output"), Some("Привет, мир"));
        assert_eq!(param(&printed[0], "name"), Some("talk"));
    }

    #[test]
    fn a_program_gets_its_arguments_and_one_that_is_not_there_fails_the_step() {
        let bench = Bench::new();
        let program = |path: &str, args: &str| CommandSpec {
            run: CommandRun::Program { path: path.to_string(), args: args.to_string() },
            ..script("", 0)
        };
        let steps = vec![command_step(program("cmd.exe", "/d /c exit 5"), OnFailure::Stop)];
        let finished = bench.run(list("program", steps), vec![], allowed());
        let reason = finished.reason.unwrap();
        assert_eq!(reason.code, "commandFailed");
        assert_eq!(reason.params.get("code").map(String::as_str), Some("5"));

        let missing = bench.dir.join("nothing-here.exe").to_string_lossy().into_owned();
        let steps = vec![command_step(program(&missing, ""), OnFailure::Stop)];
        let finished = bench.run(list("missing", steps), vec![], allowed());
        assert_eq!(finished.outcome, Outcome::Failed);
        let reason = finished.reason.unwrap();
        assert_eq!(reason.code, "commandStartFailed");
        assert!(reason.params.get("message").is_some_and(|message| !message.is_empty()));
    }

    #[test]
    fn a_failed_command_block_leaves_through_error_and_fails_when_nothing_is_linked_there() {
        let graph = |id: &str, with_error: bool| {
            let mut edges = vec![edge("s", Port::Next, "c"), edge("c", Port::Next, "fine")];
            if with_error {
                edges.push(edge("c", Port::Error, "broken"));
            }
            canvas(
                id,
                vec![
                    node("s", NodeKind::Start),
                    node("c", NodeKind::Command { command: script("exit /b 2", 0) }),
                    tick("fine"),
                    tick("broken"),
                ],
                edges,
            )
        };
        let bench = Bench::new();
        let finished = bench.run(graph("linked", true), vec![], allowed());
        assert_eq!(finished.outcome, Outcome::Completed);
        let trace = bench.trace("linked");
        assert_eq!(passes(&trace, "broken"), 1);
        assert_eq!(passes(&trace, "fine"), 0);
        assert_eq!(trace.outcomes.get("c"), Some(&Port::Error));
        assert_eq!(bench.journal("commandPassed").len(), 1);

        let finished = bench.run(graph("unlinked", false), vec![], allowed());
        assert_eq!(finished.outcome, Outcome::Failed);
        let reason = finished.reason.unwrap();
        assert_eq!(reason.code, "commandFailed");
        assert_eq!(reason.params.get("code").map(String::as_str), Some("2"));
        assert_eq!(passes(&bench.trace("unlinked"), "fine"), 0);
        // Only the failure that was passed over is noted as such.
        assert_eq!(bench.journal("commandPassed").len(), 1);
    }

    /// A script whose child writes `late.txt` about a second after the start.
    fn slow(bench: &Bench, timeout_ms: i64) -> (CommandSpec, PathBuf) {
        let mut command = script("cmd /d /c \"ping -n 2 127.0.0.1 >nul & echo 1>late.txt\"", timeout_ms);
        command.work_dir = bench.dir.to_string_lossy().into_owned();
        (command, bench.dir.join("late.txt"))
    }

    #[test]
    fn a_timeout_ends_the_command_with_everything_it_started() {
        let bench = Bench::new();
        // Left alone, the child gets to its file.
        let (command, late) = slow(&bench, 0);
        let finished = bench.run(list("patient", vec![command_step(command, OnFailure::Stop)]), vec![], allowed());
        assert_eq!(finished.outcome, Outcome::Completed);
        assert!(late.exists());
        std::fs::remove_file(&late).unwrap();

        let (command, late) = slow(&bench, 200);
        let finished = bench.run(list("hasty", vec![command_step(command, OnFailure::Stop)]), vec![], allowed());
        assert_eq!(finished.outcome, Outcome::Failed);
        assert_eq!(finished.reason.unwrap().code, "commandTimeout");
        thread::sleep(Duration::from_millis(1500));
        assert!(!late.exists());
    }

    #[test]
    fn a_stop_ends_a_command_that_is_waited_for() {
        let bench = Bench::new();
        let steps = vec![command_step(script("ping -n 60 127.0.0.1 >nul", 0), OnFailure::Continue)];
        bench.start(list("long", steps), vec![], allowed());
        thread::sleep(Duration::from_millis(300));
        let asked = Instant::now();
        bench.engine.stop(AppError::new("userStop"));
        let finished = bench.end();
        assert_eq!(finished.outcome, Outcome::Stopped);
        assert_eq!(finished.reason.unwrap().code, "userStop");
        assert!(asked.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn a_command_that_is_not_waited_for_outlives_the_step() {
        let bench = Bench::new();
        let (mut command, late) = slow(&bench, 0);
        command.wait = false;
        let finished = bench.run(list("apart", vec![command_step(command, OnFailure::Stop)]), vec![], allowed());
        assert_eq!(finished.outcome, Outcome::Completed);
        assert_eq!(bench.journal("commandStarted").len(), 1);
        assert!(bench.journal("commandDone").is_empty());
        let deadline = Instant::now() + WAIT;
        while !late.exists() {
            assert!(Instant::now() < deadline, "the command did not go on after the step");
            thread::sleep(Duration::from_millis(50));
        }
    }
}
