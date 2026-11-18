//! The application core: the one place that knows whether ClickIt is idle,
//! running or recording, and that starts and stops everything. Commands, the
//! tray, the hotkeys and the scheduler all come here.
//!
//! Locks of this module are never held across a call that waits for the main
//! thread (windows, hotkeys, tray): the main thread may be waiting for the
//! same lock in a hotkey or tray handler.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, ThreadId};
use std::time::Duration;

use chrono::Local;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Window, WindowEvent};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::Shortcut;
use windows::Win32::Globalization::GetUserDefaultUILanguage;

use crate::agent::Session;
use crate::external::Log;
use crate::engine::conditions::capture_point;
use crate::harness::{self, ConfirmWait};
use crate::engine::{build_plan, Engine, RunOptions, Source};
use crate::error::{AppError, AppResult};
use crate::input;
use crate::journal::Journal;
use crate::model::scenario::{new_id, Scenario, ScenarioMode, WindowTarget};
use crate::model::schedule::{RunTarget, TargetKind};
use crate::model::settings::{Settings, MAX_COUNTDOWN_SEC};
use crate::recorder::{RecordOptions, Recorder};
use crate::runtime::{
    AppInfo, CaptureResult, DataKind, Outcome, RecordFinished, RecordStopReason, Rect, RegionPurpose, RegionResult,
    RunFinished, RunState, RunStatus, StatusHub, TemplateInfo,
};
use crate::scheduler::{Host, Scheduler};
use crate::screen;
use crate::shell::hotkeys::{self, Hotkey};
use crate::shell::text::{error_text, status_text, text};
use crate::shell::tray::{self, Activity, TrayModel};
use crate::shell::{notify, window};
use crate::storage::Store;
use crate::target;
use crate::transfer;
use crate::update::{self, Tracker};

/// Passed by the autostart entry: start in the tray, without the window.
pub const AUTOSTART_FLAG: &str = "--autostart";
/// Smallest side of a selected region, in pixels.
const MIN_REGION_SIDE: i32 = 4;
/// Time for the overlay to leave the screen before the screen is captured.
const OVERLAY_HIDE_DELAY: Duration = Duration::from_millis(200);
const SCHEDULER_TICK: Duration = Duration::from_secs(1);

struct CaptureSession {
    hotkey: Hotkey,
    target: Option<WindowTarget>,
}

struct RegionSession {
    scenario_id: String,
    purpose: RegionPurpose,
    target: Option<WindowTarget>,
    /// Where the mouse button went down.
    first: Option<(i32, i32)>,
}

/// Payload of the `data:changed` event.
#[derive(Clone, Serialize)]
struct DataChanged {
    kind: DataKind,
}

/// What the tray shows now, to change it only when it really changes.
#[derive(Default)]
struct TrayShown {
    activity: Activity,
    tooltip: String,
}

pub struct Core {
    app: AppHandle,
    pub store: Arc<Store>,
    pub journal: Arc<Journal>,
    pub hub: Arc<StatusHub>,
    pub engine: Arc<Engine>,
    pub recorder: Arc<Recorder>,
    pub scheduler: Scheduler,
    pub updates: Tracker,
    settings: Mutex<Settings>,
    /// The start/stop hotkey as registered with Windows.
    start_stop: Mutex<Option<Hotkey>>,
    capture: Mutex<Option<CaptureSession>>,
    region: Mutex<Option<RegionSession>>,
    /// Starts go one at a time, so that two of them cannot both see "idle".
    start_lock: Mutex<()>,
    quitting: AtomicBool,
    /// An update is being installed: nothing new starts, the process is about to end.
    updating: AtomicBool,
    tray: Mutex<TrayShown>,
    /// The thread that owns the window loop. Creating or destroying a window on it deadlocks or panics.
    ui_thread: ThreadId,
    /// Last minimized state of the main window, so a drag does not rebuild the plaque.
    main_minimized: AtomicBool,
    /// The confirm dialog the harness and the assistant share. One at a time.
    pub(crate) confirm: Mutex<Option<ConfirmWait>>,
    pub(crate) confirm_seq: AtomicU64,
    pub agent: Session,
    /// Tool calls from clickit-mcp, shown on the Assistant page.
    pub external: Log,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn with_core(app: &AppHandle, action: impl FnOnce(&Arc<Core>)) {
    if let Some(core) = app.try_state::<Arc<Core>>() {
        action(core.inner());
    }
}

/// "ru" for a Russian Windows interface, "en" for any other.
pub fn system_language() -> &'static str {
    const PRIMARY_LANGUAGE_MASK: u16 = 0x3FF;
    const LANG_RUSSIAN: u16 = 0x19;
    // SAFETY: no preconditions.
    let language = unsafe { GetUserDefaultUILanguage() };
    if language & PRIMARY_LANGUAGE_MASK == LANG_RUSSIAN {
        "ru"
    } else {
        "en"
    }
}

/// The plaque stays for the whole run: countdown, execution, waiting and both pauses.
/// Recording keeps its own badge. Idle has nothing to show.
fn hud_while_hidden(state: RunState) -> bool {
    match state {
        RunState::Countdown
        | RunState::Running
        | RunState::Waiting
        | RunState::PausedWindow
        | RunState::PausedProcess => true,
        RunState::Idle | RunState::Recording => false,
    }
}

fn activity_of(status: &RunStatus) -> Activity {
    match status.state {
        RunState::Idle => Activity::Idle,
        RunState::Recording => Activity::Recording,
        RunState::Countdown
        | RunState::Running
        | RunState::Waiting
        | RunState::PausedWindow
        | RunState::PausedProcess => Activity::Running,
    }
}

/// Process names from the settings: trimmed, without blanks and repeats.
fn clean_processes(names: &[String]) -> Vec<String> {
    let mut clean: Vec<String> = Vec::new();
    for name in names.iter().map(|name| name.trim()).filter(|name| !name.is_empty()) {
        if !clean.iter().any(|known| known.eq_ignore_ascii_case(name)) {
            clean.push(name.to_string());
        }
    }
    clean
}

/// A scenario name as a file name: without the characters Windows forbids.
fn file_name_for(name: &str) -> String {
    let clean: String =
        name.chars().map(|ch| if ch.is_control() || r#"\/:*?"<>|"#.contains(ch) { '_' } else { ch }).collect();
    let clean = clean.trim().trim_end_matches('.').to_string();
    if clean.is_empty() {
        "scenario".to_string()
    } else {
        clean
    }
}

fn save_template(store: &Store, scenario_id: &str, rect: Rect) -> AppResult<TemplateInfo> {
    let frame = screen::capture(rect)?;
    let file = format!("tpl-{}.png", new_id("t"));
    frame.save_png(&store.template_path(scenario_id, &file)?)?;
    Ok(TemplateInfo { file, width: rect.width, height: rect.height })
}

fn region_result(store: &Store, session: &RegionSession, first: (i32, i32), second: (i32, i32)) -> RegionResult {
    let rect = Rect {
        x: first.0.min(second.0),
        y: first.1.min(second.1),
        width: (first.0 - second.0).abs() + 1,
        height: (first.1 - second.1).abs() + 1,
    };
    let mut result = RegionResult { purpose: session.purpose, rect, client_rect: None, template: None, error: None };
    if rect.width < MIN_REGION_SIDE || rect.height < MIN_REGION_SIDE {
        result.error = Some(AppError::new("regionTooSmall"));
        return result;
    }
    if let Some(window_target) = &session.target {
        match target::find_target(window_target).and_then(target::client_rect) {
            Some(client) => {
                result.client_rect = rect
                    .intersect(&client)
                    .map(|inside| Rect { x: inside.x - client.x, y: inside.y - client.y, ..inside });
                if result.client_rect.is_none() && session.purpose == RegionPurpose::Area {
                    result.error = Some(AppError::new("captureOutsideTarget"));
                    return result;
                }
            }
            None if session.purpose == RegionPurpose::Area => {
                result.error =
                    Some(AppError::new("targetNotFound").with("target", target::describe_target(window_target)));
                return result;
            }
            None => {}
        }
    }
    if session.purpose == RegionPurpose::Template {
        match save_template(store, &session.scenario_id, rect) {
            Ok(template) => result.template = Some(template),
            Err(error) => result.error = Some(error),
        }
    }
    result
}

impl Core {
    /// Builds the core and puts it into the app state. Called once, from `setup`.
    pub fn create(app: &AppHandle) -> AppResult<Arc<Core>> {
        let store = Arc::new(Store::open()?);
        // Template images no step refers to any more are removed only here, at startup.
        store.sweep_templates();

        let journal = Arc::new(Journal::start(store.journal_path(), store.journal_prev_path(), {
            let app = app.clone();
            Box::new(move || {
                let _ = app.emit("journal:changed", ());
            })
        }));
        let hub = Arc::new(StatusHub::new({
            let app = app.clone();
            Box::new(move |status| {
                let _ = app.emit("run:status", status);
                with_core(&app, |core| core.status_changed(status));
            })
        }));
        let engine = Engine::new(Arc::clone(&journal), Arc::clone(&hub), {
            let app = app.clone();
            Box::new(move |finished| {
                let _ = app.emit("run:finished", finished);
                with_core(&app, |core| core.run_finished(finished));
            })
        });
        let recorder = Recorder::new(Arc::clone(&store), Arc::clone(&journal), Arc::clone(&hub), {
            let app = app.clone();
            Box::new(move |finished| {
                let _ = app.emit("record:finished", finished);
                with_core(&app, |core| core.record_finished(finished));
            })
        });
        let scheduler = Scheduler::new(Arc::clone(&store), Arc::clone(&journal), Local::now().naive_local());

        let mut settings = store.load_settings();
        if settings.language.is_none() {
            // First launch: the language of the Windows interface.
            settings.language = Some(system_language().to_string());
            let _ = store.save_settings(&settings);
        }

        let core = Arc::new(Core {
            app: app.clone(),
            store,
            journal,
            hub,
            engine,
            recorder,
            scheduler,
            updates: Tracker::new(),
            settings: Mutex::new(settings),
            start_stop: Mutex::new(None),
            capture: Mutex::new(None),
            region: Mutex::new(None),
            start_lock: Mutex::new(()),
            quitting: AtomicBool::new(false),
            updating: AtomicBool::new(false),
            tray: Mutex::new(TrayShown::default()),
            ui_thread: thread::current().id(),
            main_minimized: AtomicBool::new(false),
            confirm: Mutex::new(None),
            confirm_seq: AtomicU64::new(0),
            agent: Session::new(),
            external: Log::new(),
        });
        app.manage(Arc::clone(&core));
        Ok(core)
    }

    pub(crate) fn app(&self) -> &AppHandle {
        &self.app
    }

    pub fn info(&self) -> AppInfo {
        AppInfo {
            version: self.app.package_info().version.to_string(),
            data_dir: self.store.root().display().to_string(),
            system_language: system_language().to_string(),
        }
    }

    // Settings

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub(crate) fn lang(&self) -> String {
        lock(&self.settings).lang().to_string()
    }

    /// Checks, applies and stores new settings. Returns what was stored.
    pub fn set_settings(&self, mut new: Settings) -> AppResult<Settings> {
        let old = self.settings();
        new.language = match new.language.as_deref() {
            Some(language @ ("ru" | "en")) => Some(language.to_string()),
            _ => old.language.clone(),
        };
        new.countdown_sec = new.countdown_sec.min(MAX_COUNTDOWN_SEC);
        new.blocked_processes = clean_processes(&new.blocked_processes);
        new.hotkey_start_stop = new.hotkey_start_stop.trim().to_string();
        new.hotkey_capture = new.hotkey_capture.trim().to_string();
        new.agent_endpoint = new.agent_endpoint.trim().to_string();
        new.agent_model = new.agent_model.trim().to_string();

        let start_stop = hotkeys::parse(&new.hotkey_start_stop)?;
        let capture = hotkeys::parse(&new.hotkey_capture)?;
        if start_stop.shortcut == capture.shortcut {
            return Err(AppError::new("hotkeySame"));
        }
        let registered = *lock(&self.start_stop);
        if new.hotkey_capture != old.hotkey_capture {
            self.capture_cancel();
            // The capture hotkey is held only while the capture mode is open;
            // registering it for a moment tells whether Windows will give it.
            // The key ClickIt itself holds for start/stop is about to be freed.
            let own = registered.is_some_and(|hotkey| hotkey.shortcut == capture.shortcut);
            if !own {
                hotkeys::register(&self.app, &capture, &new.hotkey_capture)?;
                hotkeys::unregister(&self.app, &capture);
            }
        }

        let rebind = registered.map(|hotkey| hotkey.shortcut) != Some(start_stop.shortcut);
        if rebind {
            hotkeys::register(&self.app, &start_stop, &new.hotkey_start_stop)?;
        }
        if let Err(error) = self.store.save_settings(&new) {
            if rebind {
                hotkeys::unregister(&self.app, &start_stop);
            }
            return Err(error);
        }
        if rebind {
            if let Some(previous) = registered {
                hotkeys::unregister(&self.app, &previous);
            }
            *lock(&self.start_stop) = Some(start_stop);
        }
        *lock(&self.settings) = new.clone();
        if old.harness && !new.harness {
            harness::kick();
        }
        self.refresh_tray();
        Ok(new)
    }

    pub(crate) fn is_updating(&self) -> bool {
        self.updating.load(Ordering::SeqCst)
    }

    /// Registers the start/stop hotkey from the settings. Called once at startup.
    fn bind_start_stop(&self) {
        let hotkey_text = self.settings().hotkey_start_stop;
        let bound = hotkeys::parse(&hotkey_text)
            .and_then(|hotkey| hotkeys::register(&self.app, &hotkey, &hotkey_text).map(|()| hotkey));
        match bound {
            Ok(hotkey) => *lock(&self.start_stop) = Some(hotkey),
            // Another program holds the key: say so, the user can pick another one in the settings.
            Err(error) => notify(&self.app, "ClickIt", &error_text(&self.lang(), &error)),
        }
    }

    pub fn autostart_enabled(&self) -> AppResult<bool> {
        self.app.autolaunch().is_enabled().map_err(|error| AppError::new("autostartFailed").with("message", error))
    }

    pub fn set_autostart(&self, enabled: bool) -> AppResult<()> {
        let manager = self.app.autolaunch();
        let changed = if enabled { manager.enable() } else { manager.disable() };
        changed.map_err(|error| AppError::new("autostartFailed").with("message", error))
    }

    // Tray

    /// Rebuilds the tray menu: after a change of data, state, language or current target.
    pub fn refresh_tray(&self) {
        let settings = self.settings();
        let status = self.hub.get();
        let model = TrayModel {
            lang: settings.lang().to_string(),
            activity: activity_of(&status),
            current: settings.current.as_ref().and_then(|current| self.target_name(current)),
            scenarios: self.store.list_scenarios().into_iter().map(|scenario| (scenario.id, scenario.name)).collect(),
            queues: self
                .store
                .list_queues()
                .into_iter()
                .filter(|queue| !queue.broken)
                .map(|queue| (queue.id, queue.name))
                .collect(),
        };
        lock(&self.tray).activity = model.activity;
        tray::set_menu(&self.app, model);
        self.refresh_tooltip(&status);
    }

    fn refresh_tooltip(&self, status: &RunStatus) {
        let tooltip = status_text(&self.lang(), status);
        let changed = {
            let mut shown = lock(&self.tray);
            let changed = shown.tooltip != tooltip;
            if changed {
                shown.tooltip = tooltip.clone();
            }
            changed
        };
        if changed {
            tray::set_tooltip(&self.app, tooltip);
        }
    }

    fn status_changed(&self, status: &RunStatus) {
        let activity_changed = lock(&self.tray).activity != activity_of(status);
        if activity_changed {
            self.refresh_tray();
        } else {
            self.refresh_tooltip(status);
        }
        self.sync_hud();
    }

    /// The small window above the tray for the whole run while the main window
    /// is hidden or minimized. The window is left in place when the state
    /// changes inside the run, so a wait does not blink it off.
    pub(crate) fn sync_hud(&self) {
        if thread::current().id() == self.ui_thread {
            let app = self.app.clone();
            thread::spawn(move || with_core(&app, |core| core.apply_hud()));
        } else {
            self.apply_hud();
        }
    }

    fn apply_hud(&self) {
        let hidden = window::main_hidden(&self.app);
        let active = hud_while_hidden(self.hub.get().state);
        if hidden && active {
            if !window::hud_open(&self.app) {
                let _ = window::open_hud(&self.app);
            }
        } else if window::hud_open(&self.app) {
            window::close_hud(&self.app);
        }
    }

    /// Returns whether the minimized flag changed. Called from the window loop.
    fn note_minimized(&self, minimized: bool) -> bool {
        self.main_minimized.swap(minimized, Ordering::Relaxed) != minimized
    }

    fn tray_action(&self, id: &str) {
        let start = |kind: TargetKind, id: &str| {
            if let Err(error) = self.start(&RunTarget { kind, id: id.to_string() }, Source::Tray) {
                self.report_start_failure(&error);
            }
        };
        if id == tray::TOGGLE {
            self.toggle(Source::Tray);
        } else if id == tray::STOP_RECORDING {
            self.recorder.stop(RecordStopReason::User);
        } else if let Some(scenario_id) = id.strip_prefix(tray::SCENARIO_PREFIX) {
            start(TargetKind::Scenario, scenario_id);
        } else if let Some(queue_id) = id.strip_prefix(tray::QUEUE_PREFIX) {
            start(TargetKind::Queue, queue_id);
        }
    }

    // Start and stop

    pub fn target_name(&self, run_target: &RunTarget) -> Option<String> {
        match run_target.kind {
            TargetKind::Scenario => self.store.load_scenario(&run_target.id).ok().map(|scenario| scenario.name),
            TargetKind::Queue => self.store.load_queue(&run_target.id).ok().map(|queue| queue.name),
        }
    }

    fn ensure_idle(&self) -> AppResult<()> {
        if self.updating.load(Ordering::SeqCst) {
            Err(AppError::new("updating"))
        } else if self.engine.is_running() || self.recorder.is_active() || lock(&self.region).is_some() {
            Err(AppError::new("busy"))
        } else {
            Ok(())
        }
    }

    // Updates

    pub(crate) fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }

    /// Claims the application for the installer of an update. Succeeds only
    /// when nothing is running, being recorded or being selected on the
    /// screen; from then on nothing new starts.
    pub(crate) fn begin_install(&self) -> bool {
        let _starting = lock(&self.start_lock);
        if self.ensure_idle().is_err() || lock(&self.capture).is_some() {
            return false;
        }
        self.updating.store(true, Ordering::SeqCst);
        true
    }

    /// The installer could not be started: ClickIt goes on as before.
    pub(crate) fn cancel_install(&self) {
        self.updating.store(false, Ordering::SeqCst);
    }

    /// Starts a scenario or a queue. A start that did not happen is written to
    /// the journal; the scheduler writes its own entry.
    pub fn start(&self, run_target: &RunTarget, source: Source) -> AppResult<()> {
        let started = self.try_start(run_target, source);
        if let Err(error) = &started {
            if source != Source::Schedule {
                let name = self.target_name(run_target).unwrap_or_else(|| run_target.id.clone());
                self.journal.add_with_reason("startRejected", &[("name", name)], error);
            }
        }
        started
    }

    fn try_start(&self, run_target: &RunTarget, source: Source) -> AppResult<()> {
        let _starting = lock(&self.start_lock);
        self.ensure_idle()?;
        let plan = build_plan(&self.store, run_target)?;
        let settings = self.settings();
        if let Some(process) = target::first_running(&settings.blocked_processes) {
            return Err(AppError::new("blockedProcess").with("process", process));
        }
        // A scenario of the "Window" mode does not start without its window.
        // Queue items look for their windows when their turn comes.
        if plan.queue.is_none() {
            if let Some(scenario) = plan.items.first().map(|item| &item.scenario) {
                if scenario.mode == ScenarioMode::Window && target::find_target(&scenario.target).is_none() {
                    return Err(
                        AppError::new("targetNotFound").with("target", target::describe_target(&scenario.target))
                    );
                }
            }
        }
        let options = RunOptions {
            source,
            countdown_sec: settings.countdown_sec.min(MAX_COUNTDOWN_SEC),
            corner_stop: settings.corner_stop,
            blocked: settings.blocked_processes,
        };
        self.engine.start(plan, options)
    }

    /// Stops a run, a countdown or a recording.
    pub fn stop(&self) {
        self.recorder.stop(RecordStopReason::User);
        self.engine.stop(AppError::new("userStop"));
    }

    /// The start/stop hotkey and the tray item: stops what is going on, or starts the current target.
    fn toggle(&self, source: Source) {
        if self.recorder.is_active() || self.engine.is_running() {
            self.stop();
            return;
        }
        let started = match self.settings().current {
            Some(current) => self.start(&current, source),
            None => Err(AppError::new("nothingToRun")),
        };
        if let Err(error) = started {
            self.report_start_failure(&error);
        }
    }

    /// A start from the hotkey or the tray has no window to show its error in.
    fn report_start_failure(&self, error: &AppError) {
        let lang = self.lang();
        notify(&self.app, &text(&lang, "native.notify.startFailed", &[]), &error_text(&lang, error));
    }

    fn run_finished(&self, finished: &RunFinished) {
        self.refresh_tray();
        if finished.outcome == Outcome::Failed && !window::main_visible(&self.app) {
            if let Some(reason) = &finished.reason {
                let lang = self.lang();
                let body = text(
                    &lang,
                    "native.notify.named",
                    &[("name", &finished.target.name), ("text", &error_text(&lang, reason))],
                );
                notify(&self.app, &text(&lang, "native.notify.runFailed", &[]), &body);
            }
        }
    }

    /// Whether a save or a delete of the scenario has to wait: it is being
    /// recorded into, or a run that may use it is going on.
    pub fn scenario_in_use(&self, id: &str) -> bool {
        if self.recorder.scenario_id().as_deref() == Some(id) {
            return true;
        }
        if !self.engine.is_running() {
            return false;
        }
        match self.hub.get().target {
            Some(running) => running.kind == TargetKind::Queue || running.id == id,
            None => true,
        }
    }

    pub fn queue_in_use(&self, id: &str) -> bool {
        self.engine.is_running()
            && self.hub.get().target.is_none_or(|running| running.kind == TargetKind::Queue && running.id == id)
    }

    // Recording

    /// Shows the badge, hides the main window and records into the scenario.
    /// There is no recording without the badge on the screen.
    pub fn record_start(&self, scenario_id: &str) -> AppResult<()> {
        {
            let _starting = lock(&self.start_lock);
            self.ensure_idle()?;
            self.recorder.check(scenario_id)?;
        }
        self.capture_cancel();
        window::open_badge(&self.app).map_err(|_| AppError::new("recordFailed"))?;
        window::hide_main(&self.app);
        let settings = self.settings();
        let stop_hotkey = lock(&self.start_stop).map(|hotkey| (hotkey.modifiers, hotkey.vk));
        let options = RecordOptions { corner_stop: settings.corner_stop, stop_hotkey };
        match self.recorder.start(scenario_id, options) {
            Ok(target_window) => {
                if let Some(hwnd) = target_window {
                    target::request_focus(hwnd);
                }
                Ok(())
            }
            Err(error) => {
                window::close_badge(&self.app);
                window::show_main(&self.app);
                Err(error)
            }
        }
    }

    fn record_finished(&self, finished: &RecordFinished) {
        window::close_badge(&self.app);
        if finished.added > 0 {
            self.data_changed(DataKind::Scenarios);
        }
        self.refresh_tray();
        let lang = self.lang();
        match finished.reason {
            RecordStopReason::Exit => return,
            RecordStopReason::User => {}
            // The user is in another program and has not asked for the stop: tell why it happened.
            RecordStopReason::TimeLimit | RecordStopReason::StepLimit => {
                let reason = text(&lang, &format!("journal.recordReasons.{}", finished.reason.as_str()), &[]);
                notify(&self.app, &text(&lang, "native.notify.recordStopped", &[]), &reason);
            }
        }
        window::show_main(&self.app);
    }

    fn data_changed(&self, kind: DataKind) {
        let _ = self.app.emit("data:changed", DataChanged { kind });
    }

    // Capture of a point

    /// Arms the capture hotkey: the next press reports the point under the cursor.
    pub fn capture_start(&self, window_target: Option<WindowTarget>) -> AppResult<()> {
        if self.engine.is_running() || self.recorder.is_active() {
            return Err(AppError::new("busy"));
        }
        self.capture_cancel();
        let hotkey_text = self.settings().hotkey_capture;
        let hotkey = hotkeys::parse(&hotkey_text)?;
        hotkeys::register(&self.app, &hotkey, &hotkey_text)?;
        *lock(&self.capture) = Some(CaptureSession { hotkey, target: window_target });
        Ok(())
    }

    pub fn capture_cancel(&self) {
        let session = lock(&self.capture).take();
        if let Some(session) = session {
            hotkeys::unregister(&self.app, &session.hotkey);
        }
    }

    fn capture_shot(&self) {
        let session = lock(&self.capture).take();
        let Some(session) = session else {
            return;
        };
        hotkeys::unregister(&self.app, &session.hotkey);
        let result: CaptureResult = capture_point(session.target.as_ref());
        let _ = self.app.emit("capture:point", &result);
    }

    // Selection of a region

    /// Hides the main window and opens the overlay for selecting a rectangle.
    pub fn region_start(
        &self,
        scenario_id: &str,
        purpose: RegionPurpose,
        window_target: Option<WindowTarget>,
    ) -> AppResult<()> {
        {
            let _starting = lock(&self.start_lock);
            self.ensure_idle()?;
            if !self.store.scenario_exists(scenario_id) {
                return Err(AppError::new("notFound"));
            }
            *lock(&self.region) =
                Some(RegionSession { scenario_id: scenario_id.to_string(), purpose, target: window_target, first: None });
        }
        self.capture_cancel();
        window::hide_main(&self.app);
        if window::open_region(&self.app, screen::virtual_screen()).is_err() {
            *lock(&self.region) = None;
            window::close_region(&self.app);
            window::show_main(&self.app);
            return Err(AppError::new("captureFailed"));
        }
        Ok(())
    }

    /// The mouse button went down: the cursor is at the first corner.
    pub fn region_mark(&self) {
        let position = input::cursor_pos();
        if let Some(session) = lock(&self.region).as_mut() {
            session.first = Some(position);
        }
    }

    /// The mouse button went up: the cursor is at the opposite corner.
    pub fn region_finish(&self) {
        let second = input::cursor_pos();
        let session = lock(&self.region).take();
        let Some(session) = session else {
            return;
        };
        window::close_region(&self.app);
        let Some(first) = session.first else {
            window::show_main(&self.app);
            let _ = self.app.emit("region:cancelled", ());
            return;
        };
        thread::sleep(OVERLAY_HIDE_DELAY);
        let result = region_result(&self.store, &session, first, second);
        window::show_main(&self.app);
        let _ = self.app.emit("region:done", &result);
    }

    pub fn region_cancel(&self) {
        let session = lock(&self.region).take();
        if session.is_some() {
            window::close_region(&self.app);
            window::show_main(&self.app);
            let _ = self.app.emit("region:cancelled", ());
        }
    }

    // Export and import

    /// Asks where to save and writes the `.clickit` file. `None` when the dialog was cancelled.
    pub fn export_scenario(&self, id: &str) -> AppResult<Option<String>> {
        let scenario = self.store.load_scenario(id)?;
        let lang = self.lang();
        let mut dialog = self
            .app
            .dialog()
            .file()
            .set_title(text(&lang, "native.dialog.exportTitle", &[]))
            .add_filter(text(&lang, "native.dialog.fileType", &[]), &[transfer::EXTENSION])
            .set_file_name(format!("{}.{}", file_name_for(&scenario.name), transfer::EXTENSION));
        if let Some(main) = self.app.get_webview_window(window::MAIN) {
            dialog = dialog.set_parent(&main);
        }
        let Some(mut path) = dialog.blocking_save_file().and_then(|picked| picked.into_path().ok()) else {
            return Ok(None);
        };
        if path.extension().is_none() {
            path.set_extension(transfer::EXTENSION);
        }
        transfer::export(&self.store, id, &path)?;
        Ok(Some(path.display().to_string()))
    }

    /// Asks for a `.clickit` file and adds its scenario. `None` when the dialog was cancelled.
    pub fn import_scenario(&self) -> AppResult<Option<Scenario>> {
        let lang = self.lang();
        let mut dialog = self
            .app
            .dialog()
            .file()
            .set_title(text(&lang, "native.dialog.importTitle", &[]))
            .add_filter(text(&lang, "native.dialog.fileType", &[]), &[transfer::EXTENSION]);
        if let Some(main) = self.app.get_webview_window(window::MAIN) {
            dialog = dialog.set_parent(&main);
        }
        let Some(path) = dialog.blocking_pick_file().and_then(|picked| picked.into_path().ok()) else {
            return Ok(None);
        };
        let scenario = transfer::import(&self.store, &path)?;
        self.refresh_tray();
        Ok(Some(scenario))
    }

    // Exit

    /// Stops a run or a recording, lets them release what they hold and save
    /// what they recorded, then ends the process.
    pub fn quit(self: &Arc<Self>) {
        if self.quitting.swap(true, Ordering::SeqCst) {
            return;
        }
        harness::kick();
        let core = Arc::clone(self);
        thread::spawn(move || {
            core.recorder.stop(RecordStopReason::Exit);
            core.engine.stop(AppError::new("appExit"));
            core.recorder.join();
            core.engine.join();
            core.app.exit(0);
        });
    }
}

impl Host for Core {
    fn target_name(&self, run_target: &RunTarget) -> Option<String> {
        Core::target_name(self, run_target)
    }

    fn start_scheduled(&self, run_target: &RunTarget) -> AppResult<()> {
        self.start(run_target, Source::Schedule)
    }

    fn notify_skipped(&self, name: &str, reason: &AppError) {
        let lang = self.lang();
        let body = text(&lang, "native.notify.named", &[("name", name), ("text", &error_text(&lang, reason))]);
        notify(&self.app, &text(&lang, "native.notify.scheduleSkipped", &[]), &body);
    }
}

/// Creates the core, the tray icon and the scheduler thread, registers the
/// hotkey and shows the main window unless started by the autostart entry.
pub fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    // Before the window opens, so the Assistant page can already see clickit-mcp.exe.
    crate::external::place_bundled_mcp();
    let core = Core::create(app)?;
    core.journal.add("appStart", &[("version", app.package_info().version.to_string())]);
    tray::create(app, tray_menu)?;
    core.refresh_tray();
    core.bind_start_stop();

    let scheduler_core = Arc::clone(&core);
    thread::Builder::new().name("clickit-schedule".to_string()).spawn(move || loop {
        thread::sleep(SCHEDULER_TICK);
        if scheduler_core.quitting.load(Ordering::SeqCst) {
            break;
        }
        let catch_up = scheduler_core.settings().catch_up;
        scheduler_core.scheduler.tick(Local::now().naive_local(), catch_up, scheduler_core.as_ref());
    })?;

    harness::serve(Arc::clone(&core));
    if !std::env::args().any(|arg| arg == AUTOSTART_FLAG) {
        window::show_main(app);
    }
    update::check_at_startup(&core);
    Ok(())
}

/// A second copy was started: the running one comes to the front instead.
pub fn second_instance(app: &AppHandle, args: &[String]) {
    if !args.iter().any(|arg| arg == AUTOSTART_FLAG) {
        window::show_main(app);
    }
}

/// A global hotkey was pressed. Runs on the main thread, so the work goes to another one.
pub fn hotkey_pressed(app: &AppHandle, shortcut: &Shortcut) {
    with_core(app, |core| {
        let is_start_stop = lock(&core.start_stop).is_some_and(|hotkey| hotkey.shortcut == *shortcut);
        let is_capture = lock(&core.capture).as_ref().is_some_and(|session| session.hotkey.shortcut == *shortcut);
        let core = Arc::clone(core);
        if is_start_stop {
            thread::spawn(move || core.toggle(Source::Hotkey));
        } else if is_capture {
            thread::spawn(move || core.capture_shot());
        }
    });
}

/// An item of the tray menu was chosen. Runs on the main thread.
fn tray_menu(app: &AppHandle, id: &str) {
    with_core(app, |core| {
        if id == tray::OPEN {
            window::show_main(app);
        } else if id == tray::QUIT {
            core.quit();
        } else {
            let core = Arc::clone(core);
            let id = id.to_string();
            thread::spawn(move || core.tray_action(&id));
        }
    });
}

/// Closing the main window hides it to the tray, or quits when the settings say so.
/// The helper windows are closed only by the core.
pub fn window_event(closing: &Window, event: &WindowEvent) {
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let label = closing.label().to_string();
            with_core(closing.app_handle(), |core| {
                if label == window::MAIN {
                    if core.settings().close_exits {
                        core.quit();
                    } else {
                        let _ = closing.hide();
                        core.sync_hud();
                    }
                } else if label == window::REGION {
                    let core = Arc::clone(core);
                    thread::spawn(move || core.region_cancel());
                }
                // The badge stays for as long as the recording goes on.
            });
        }
        WindowEvent::Resized(_) if closing.label() == window::MAIN => {
            let minimized = closing.is_minimized().unwrap_or(false);
            with_core(closing.app_handle(), |core| {
                if core.note_minimized(minimized) {
                    core.sync_hud();
                }
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::hud_while_hidden;
    use crate::runtime::RunState;

    #[test]
    fn the_tray_plaque_stays_up_for_the_whole_run() {
        assert!(hud_while_hidden(RunState::Countdown));
        assert!(hud_while_hidden(RunState::Running));
        assert!(hud_while_hidden(RunState::Waiting));
        assert!(hud_while_hidden(RunState::PausedWindow));
        assert!(hud_while_hidden(RunState::PausedProcess));
        assert!(!hud_while_hidden(RunState::Idle));
        assert!(!hud_while_hidden(RunState::Recording));
    }
}
