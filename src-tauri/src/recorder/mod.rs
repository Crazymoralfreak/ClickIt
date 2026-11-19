//! Recording of the user's own mouse and keyboard into a scenario.
//!
//! Two low-level hooks live on their own thread for the duration of a recording
//! and only pass events on; a second thread turns them into steps and applies
//! the rules: the badge stays on screen, mouse moves are not recorded, in the
//! "Window" mode only what happens in the target window counts, the recording
//! ends by the Stop button, the hotkey, the screen corner, after 10 minutes or
//! at 500 steps. The steps are appended to the end of the scenario.
//!
//! A recording can be held: nothing is recorded and its clock stands still,
//! so the time of a hold does not become a pause between two steps. The user
//! holds it with a hotkey; the core holds it while an image is being selected
//! and then hands over the step that waits for that image.

pub mod normalize;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetKeyState, GetKeyboardLayout, ToUnicodeEx};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, PeekMessageW,
    PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
    WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
};

use crate::engine::timing::cursor_in_corner;
use crate::error::{AppError, AppResult};
use crate::input::keys::{modifier_of, ModifierKey};
use crate::journal::Journal;
use crate::model::scenario::{
    Modifiers, MouseButton, Position, Scenario, ScenarioMode, Step, WindowTarget, MAX_STEPS,
};
use crate::runtime::{RecordFinished, RecordStopReason, RecordingProgress, Rect, RunState, RunStatus, StatusHub};
use crate::storage::Store;
use crate::target::{self, Hwnd};
use normalize::{Normalizer, RawEvent};

const MAX_DURATION: Duration = Duration::from_secs(10 * 60);
const TICK: Duration = Duration::from_millis(100);

const MOUSE_INJECTED: u32 = 0x01;
const KEY_EXTENDED: u32 = 0x01;
const KEY_INJECTED: u32 = 0x10;
const VK_SHIFT: usize = 0x10;
const VK_CONTROL: usize = 0x11;
const VK_ALT: usize = 0x12;
const VK_CAPS_LOCK: u16 = 0x14;
const WHEEL_NOTCH: i32 = 120;
/// ToUnicodeEx: leave the keyboard state of the system untouched.
const TO_UNICODE_KEEP_STATE: u32 = 0x4;

/// What a hook saw, in virtual-screen pixels, before the rules of the mode are applied.
enum Hooked {
    MouseDown { button: MouseButton, x: i32, y: i32 },
    MouseUp { button: MouseButton, x: i32, y: i32 },
    Wheel { up: bool, x: i32, y: i32 },
    KeyDown { vk: u16, extended: bool, ch: Option<char>, modifiers: Modifiers },
    KeyUp { vk: u16, extended: bool },
}

/// State of the hook thread. Hook procedures run on the thread that installed them.
struct Sink {
    sender: Sender<(u64, Hooked)>,
    started: Instant,
    modifiers: Modifiers,
    caps_on: bool,
    caps_down: bool,
}

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

fn key_is_down(vk: i32) -> bool {
    // SAFETY: GetAsyncKeyState has no preconditions.
    unsafe { GetAsyncKeyState(vk) < 0 }
}

impl Sink {
    fn new(sender: Sender<(u64, Hooked)>, started: Instant) -> Self {
        let modifiers = Modifiers {
            ctrl: key_is_down(VK_CONTROL as i32),
            alt: key_is_down(VK_ALT as i32),
            shift: key_is_down(VK_SHIFT as i32),
            win: key_is_down(0x5B) || key_is_down(0x5C),
        };
        // SAFETY: GetKeyState has no preconditions.
        let caps_on = unsafe { GetKeyState(VK_CAPS_LOCK as i32) } & 1 != 0;
        Self { sender, started, modifiers, caps_on, caps_down: false }
    }

    fn send(&self, event: Hooked) {
        let _ = self.sender.send((self.started.elapsed().as_millis() as u64, event));
    }

    fn track_modifier(&mut self, vk: u16, down: bool) {
        match modifier_of(vk) {
            Some(ModifierKey::Ctrl) => self.modifiers.ctrl = down,
            Some(ModifierKey::Alt) => self.modifiers.alt = down,
            Some(ModifierKey::Shift) => self.modifiers.shift = down,
            Some(ModifierKey::Win) => self.modifiers.win = down,
            None => {}
        }
        if vk == VK_CAPS_LOCK {
            if down && !self.caps_down {
                self.caps_on = !self.caps_on;
            }
            self.caps_down = down;
        }
    }

    /// The character the key types in the layout of the active window, if it is a plain one.
    fn character(&self, vk: u16, scan: u32) -> Option<char> {
        let mut state = [0u8; 256];
        if self.modifiers.shift {
            state[VK_SHIFT] = 0x80;
        }
        if self.caps_on {
            state[VK_CAPS_LOCK as usize] = 0x01;
        }
        // Ctrl+Alt is AltGr on many layouts and types characters of its own.
        if self.modifiers.ctrl && self.modifiers.alt {
            state[VK_CONTROL] = 0x80;
            state[VK_ALT] = 0x80;
        }
        let mut buffer = [0u16; 8];
        // SAFETY: the buffers are valid for the calls; the flag keeps the system keyboard state as it is.
        let written = unsafe {
            let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let layout = GetKeyboardLayout(thread);
            ToUnicodeEx(u32::from(vk), scan, &state, &mut buffer, TO_UNICODE_KEEP_STATE, Some(layout))
        };
        if written != 1 {
            return None;
        }
        char::from_u32(u32::from(buffer[0])).filter(|ch| !ch.is_control())
    }

    fn on_mouse(&mut self, message: u32, info: &MSLLHOOKSTRUCT) {
        if info.flags & MOUSE_INJECTED != 0 {
            return;
        }
        let (x, y) = (info.pt.x, info.pt.y);
        let event = match message {
            WM_LBUTTONDOWN => Hooked::MouseDown { button: MouseButton::Left, x, y },
            WM_LBUTTONUP => Hooked::MouseUp { button: MouseButton::Left, x, y },
            WM_RBUTTONDOWN => Hooked::MouseDown { button: MouseButton::Right, x, y },
            WM_RBUTTONUP => Hooked::MouseUp { button: MouseButton::Right, x, y },
            WM_MBUTTONDOWN => Hooked::MouseDown { button: MouseButton::Middle, x, y },
            WM_MBUTTONUP => Hooked::MouseUp { button: MouseButton::Middle, x, y },
            WM_MOUSEWHEEL => {
                // The wheel delta is the signed high word.
                let delta = i32::from((info.mouseData >> 16) as u16 as i16);
                for _ in 1..(delta.abs() / WHEEL_NOTCH).max(1) {
                    self.send(Hooked::Wheel { up: delta > 0, x, y });
                }
                Hooked::Wheel { up: delta > 0, x, y }
            }
            // Moves, the horizontal wheel and the extra buttons are not recorded.
            _ => return,
        };
        self.send(event);
    }

    fn on_key(&mut self, message: u32, info: &KBDLLHOOKSTRUCT) {
        if info.flags.0 & KEY_INJECTED != 0 {
            return;
        }
        let Ok(vk) = u16::try_from(info.vkCode) else {
            return;
        };
        let extended = info.flags.0 & KEY_EXTENDED != 0;
        let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        self.track_modifier(vk, down);
        if !down {
            self.send(Hooked::KeyUp { vk, extended });
            return;
        }
        let ch = if modifier_of(vk).is_some() { None } else { self.character(vk, info.scanCode) };
        let mut modifiers = self.modifiers;
        if ch.is_some() && modifiers.ctrl && modifiers.alt {
            // An AltGr character is plain text, not a Ctrl+Alt combination.
            modifiers.ctrl = false;
            modifiers.alt = false;
        }
        self.send(Hooked::KeyDown { vk, extended, ch, modifiers });
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: for WH_MOUSE_LL with a non-negative code `lparam` points to an MSLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        SINK.with(|sink| {
            if let Some(sink) = sink.borrow_mut().as_mut() {
                sink.on_mouse(wparam.0 as u32, info);
            }
        });
    }
    // SAFETY: passes the event on unchanged, as every hook has to.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: for WH_KEYBOARD_LL with a non-negative code `lparam` points to a KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        SINK.with(|sink| {
            if let Some(sink) = sink.borrow_mut().as_mut() {
                sink.on_key(wparam.0 as u32, info);
            }
        });
    }
    // SAFETY: passes the event on unchanged, as every hook has to.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Installs the hooks and pumps messages until WM_QUIT. Reports its thread id
/// (or the failure) through `ready`.
fn hook_thread(sender: Sender<(u64, Hooked)>, started: Instant, ready: Sender<Option<u32>>) {
    // SAFETY: plain Win32 calls with valid arguments; both hooks are removed before the thread ends.
    unsafe {
        let mut message = MSG::default();
        // Creates the message queue, so that WM_QUIT posted right away is not lost.
        let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
        let module = GetModuleHandleW(None).ok().map(Into::into);
        SINK.with(|sink| *sink.borrow_mut() = Some(Sink::new(sender, started)));
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
        let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
        let installed = mouse.is_ok() && keyboard.is_ok();
        let _ = ready.send(installed.then(|| GetCurrentThreadId()));
        if installed {
            while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                DispatchMessageW(&message);
            }
        }
        if let Ok(hook) = mouse {
            let _ = UnhookWindowsHookEx(hook);
        }
        if let Ok(hook) = keyboard {
            let _ = UnhookWindowsHookEx(hook);
        }
        SINK.with(|sink| *sink.borrow_mut() = None);
    }
}

pub struct RecordOptions {
    pub corner_stop: bool,
    /// The hotkeys ClickIt holds during the recording (stop, pause, the image
    /// ones): their presses are not recorded.
    pub own_hotkeys: Vec<(Modifiers, u16)>,
}

type FinishedSink = Box<dyn Fn(&RecordFinished) + Send + Sync>;

pub struct Recorder {
    store: Arc<Store>,
    journal: Arc<Journal>,
    hub: Arc<StatusHub>,
    active: AtomicBool,
    stop: Mutex<Option<RecordStopReason>>,
    scenario_id: Mutex<Option<String>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    /// The user paused the recording with the hotkey.
    paused_by_user: AtomicBool,
    /// The core is busy with the recording itself: an image is being selected.
    busy: AtomicBool,
    /// Steps made by the core, waiting for the worker to put them in.
    handed: Mutex<Vec<Step>>,
    on_finished: FinishedSink,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One recording, owned by its worker thread.
struct Session {
    scenario_id: String,
    name: String,
    mode: ScenarioMode,
    target: WindowTarget,
    hwnd: Option<Hwnd>,
    /// Steps the scenario had before the recording.
    existing: usize,
    options: RecordOptions,
    normalizer: Normalizer,
    hold: Hold,
}

/// The holds of a recording. All times are milliseconds since its start.
#[derive(Debug, Default)]
struct Hold {
    /// When the current hold began.
    since: Option<u64>,
    /// How long the recording was held before, in total.
    total: u64,
}

impl Hold {
    fn is_on(&self) -> bool {
        self.since.is_some()
    }

    /// The time of the recording without its holds: this is what the pauses
    /// between steps are made of. It stands still while a hold is on.
    fn clock(&self, at: u64) -> u64 {
        self.since.map_or(at, |since| at.min(since)).saturating_sub(self.total)
    }

    /// Turns the hold on or off. Returns whether it has just begun.
    fn set(&mut self, on: bool, now: u64) -> bool {
        match (on, self.since) {
            (true, None) => {
                self.since = Some(now);
                true
            }
            (false, Some(since)) => {
                self.total += now.saturating_sub(since);
                self.since = None;
                false
            }
            _ => false,
        }
    }
}

impl Session {
    fn room(&self) -> usize {
        MAX_STEPS.saturating_sub(self.existing)
    }

    /// Client area of the target window while it is the active one.
    fn active_client(&mut self) -> Option<Rect> {
        if self.hwnd.is_none_or(|hwnd| !target::is_window(hwnd)) {
            self.hwnd = target::find_target(&self.target);
        }
        let hwnd = self.hwnd?;
        if target::foreground() == hwnd {
            target::client_rect(hwnd)
        } else {
            None
        }
    }

    /// Nothing is recorded in the "Window" mode while another window is active.
    fn paused(&mut self) -> bool {
        self.mode == ScenarioMode::Window && self.active_client().is_none()
    }

    /// Where a mouse event happened in the coordinates of the mode; `None` when it does not count.
    fn position(&mut self, x: i32, y: i32) -> Option<Position> {
        if target::own_window_at(x, y) {
            return None;
        }
        match self.mode {
            ScenarioMode::Screen => Some(Position::Absolute { x, y }),
            ScenarioMode::Window => {
                let client = self.active_client()?;
                client.contains(x, y).then(|| Position::Client { x: x - client.x, y: y - client.y })
            }
        }
    }

    fn handle(&mut self, at: u64, event: Hooked) {
        let held = self.hold.is_on();
        let at = self.hold.clock(at);
        match event {
            // Releases always go through: a modifier pressed in the target window
            // may be let go after another window became active, or during a hold.
            Hooked::KeyUp { vk, extended } => self.normalizer.push(at, RawEvent::KeyUp { vk, extended }),
            Hooked::KeyDown { vk, modifiers, .. } if self.options.own_hotkeys.contains(&(modifiers, vk)) => {
                self.normalizer.forget_combination();
            }
            _ if held => {}
            Hooked::MouseDown { button, x, y } => {
                if let Some(position) = self.position(x, y) {
                    self.normalizer.push(at, RawEvent::MouseDown { button, position });
                }
            }
            Hooked::MouseUp { button, x, y } => match self.position(x, y) {
                Some(position) => self.normalizer.push(at, RawEvent::MouseUp { button, position }),
                None => self.normalizer.cancel_press(),
            },
            Hooked::Wheel { up, x, y } => {
                if let Some(position) = self.position(x, y) {
                    self.normalizer.push(at, RawEvent::Wheel { up, position });
                }
            }
            Hooked::KeyDown { vk, extended, ch, modifiers } => {
                if !self.paused() {
                    self.normalizer.push(at, RawEvent::KeyDown { vk, extended, ch, modifiers });
                }
            }
        }
    }
}

impl Recorder {
    pub fn new(store: Arc<Store>, journal: Arc<Journal>, hub: Arc<StatusHub>, on_finished: FinishedSink) -> Arc<Self> {
        Arc::new(Self {
            store,
            journal,
            hub,
            active: AtomicBool::new(false),
            stop: Mutex::new(None),
            scenario_id: Mutex::new(None),
            thread: Mutex::new(None),
            paused_by_user: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            handed: Mutex::new(Vec::new()),
            on_finished,
        })
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// The scenario being recorded into.
    pub fn scenario_id(&self) -> Option<String> {
        lock(&self.scenario_id).clone()
    }

    /// Starts recording into the scenario. Returns the target window of the
    /// "Window" mode, for the caller to bring to the front.
    pub fn start(self: &Arc<Self>, scenario_id: &str, options: RecordOptions) -> AppResult<Option<Hwnd>> {
        if self.active.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err(AppError::new("busy"));
        }
        let started = self.begin(scenario_id, options);
        if started.is_err() {
            *lock(&self.scenario_id) = None;
            self.active.store(false, Ordering::SeqCst);
        }
        started
    }

    /// Whether a recording into the scenario could start right now: there is
    /// room for steps and, in the "Window" mode, the target window exists.
    pub fn check(&self, scenario_id: &str) -> AppResult<()> {
        self.prepare(scenario_id).map(|_| ())
    }

    fn prepare(&self, scenario_id: &str) -> AppResult<(Scenario, Option<Hwnd>)> {
        let scenario = self.store.load_scenario(scenario_id)?;
        if scenario.step_count() >= MAX_STEPS {
            return Err(AppError::new("stepLimit").with("max", MAX_STEPS));
        }
        let hwnd = match scenario.mode {
            ScenarioMode::Screen => None,
            ScenarioMode::Window => Some(target::find_target(&scenario.target).ok_or_else(|| {
                AppError::new("targetNotFound").with("target", target::describe_target(&scenario.target))
            })?),
        };
        Ok((scenario, hwnd))
    }

    fn begin(self: &Arc<Self>, scenario_id: &str, options: RecordOptions) -> AppResult<Option<Hwnd>> {
        let (scenario, hwnd) = self.prepare(scenario_id)?;
        let existing = scenario.step_count();

        let started = Instant::now();
        let (sender, events) = mpsc::channel();
        let (ready_sender, ready) = mpsc::channel();
        let hooks = thread::Builder::new()
            .name("clickit-hooks".to_string())
            .spawn(move || hook_thread(sender, started, ready_sender))?;
        let Some(hook_thread_id) = ready.recv().ok().flatten() else {
            let _ = hooks.join();
            return Err(AppError::new("recordFailed"));
        };

        *lock(&self.stop) = None;
        *lock(&self.scenario_id) = Some(scenario.id.clone());
        self.paused_by_user.store(false, Ordering::SeqCst);
        self.busy.store(false, Ordering::SeqCst);
        lock(&self.handed).clear();
        let session = Session {
            scenario_id: scenario.id.clone(),
            name: scenario.name.clone(),
            mode: scenario.mode,
            target: scenario.target.clone(),
            hwnd,
            existing,
            options,
            normalizer: Normalizer::new(),
            hold: Hold::default(),
        };
        self.journal.add("recordStart", &[("name", scenario.name.clone())]);
        let recorder = Arc::clone(self);
        let worker = thread::Builder::new().name("clickit-record".to_string()).spawn(move || {
            recorder.work(session, events, started);
            // SAFETY: posting to a thread id is always allowed; a dead thread only makes the call fail.
            unsafe {
                let _ = PostThreadMessageW(hook_thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            let _ = hooks.join();
        });
        match worker {
            Ok(handle) => {
                *lock(&self.thread) = Some(handle);
                Ok(hwnd)
            }
            Err(error) => {
                // SAFETY: as above.
                unsafe {
                    let _ = PostThreadMessageW(hook_thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
                }
                Err(AppError::from(error))
            }
        }
    }

    /// Asks the recording to end; what was recorded is saved.
    pub fn stop(&self, reason: RecordStopReason) {
        if self.is_active() {
            lock(&self.stop).get_or_insert(reason);
        }
    }

    /// The pause hotkey: holds the recording, or lets it go on.
    pub fn toggle_pause(&self) {
        if self.is_active() {
            self.paused_by_user.fetch_xor(true, Ordering::SeqCst);
        }
    }

    /// Holds the recording while the core does something for it on the screen.
    pub fn set_busy(&self, busy: bool) {
        self.busy.store(busy, Ordering::SeqCst);
    }

    /// Hands over a step made outside: it goes after what is recorded so far.
    pub fn add_step(&self, step: Step) {
        if self.is_active() {
            lock(&self.handed).push(step);
        }
    }

    /// Takes the handed steps and the state of the hold into the session.
    fn sync(&self, session: &mut Session, now: u64) {
        // The hold is read first: a step is handed over before its hold is lifted,
        // so it is seen here no later than the end of the hold and gets the time
        // the hold began at.
        let held = self.paused_by_user.load(Ordering::SeqCst) || self.busy.load(Ordering::SeqCst);
        let handed = std::mem::take(&mut *lock(&self.handed));
        for step in handed {
            if session.normalizer.len() < session.room() {
                session.normalizer.insert(step, session.hold.clock(now));
            }
        }
        if session.hold.set(held, now) {
            // A button that is down will be released where nothing is recorded.
            session.normalizer.cancel_press();
        }
    }

    /// Waits for the recording to be saved. Used on exit, after `stop`.
    pub fn join(&self) {
        let handle = lock(&self.thread).take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    fn publish(&self, session: &Session, paused: bool) {
        let held = session.hold.is_on();
        self.hub.set(&RunStatus {
            state: RunState::Recording,
            scenario_id: Some(session.scenario_id.clone()),
            scenario_name: Some(session.name.clone()),
            recording: Some(RecordingProgress {
                scenario_id: session.scenario_id.clone(),
                steps: session.normalizer.len().min(session.room()),
                // A hold says more than the wait for the window does.
                paused: paused && !held,
                held,
            }),
            ..RunStatus::default()
        });
    }

    fn work(&self, mut session: Session, events: Receiver<(u64, Hooked)>, started: Instant) {
        let now = || started.elapsed().as_millis() as u64;
        let reason = loop {
            if let Some(reason) = *lock(&self.stop) {
                break reason;
            }
            self.sync(&mut session, now());
            // The time of a hold does not count towards the limit.
            if session.hold.clock(now()) >= MAX_DURATION.as_millis() as u64 {
                break RecordStopReason::TimeLimit;
            }
            if session.normalizer.len() >= session.room() {
                break RecordStopReason::StepLimit;
            }
            if session.options.corner_stop && cursor_in_corner() {
                break RecordStopReason::User;
            }
            let paused = session.paused();
            self.publish(&session, paused);
            match events.recv_timeout(TICK) {
                Ok((at, event)) => {
                    self.sync(&mut session, now());
                    session.handle(at, event);
                    while session.normalizer.len() < session.room() {
                        match events.try_recv() {
                            Ok((at, event)) => {
                                self.sync(&mut session, now());
                                session.handle(at, event);
                            }
                            Err(_) => break,
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                // The hook thread is gone; nothing more will arrive.
                Err(RecvTimeoutError::Disconnected) => break RecordStopReason::User,
            }
        };
        self.finish(session, reason);
    }

    fn finish(&self, session: Session, reason: RecordStopReason) {
        let room = session.room();
        let name = session.name.clone();
        let mut steps = session.normalizer.finish();
        steps.truncate(room);
        let mut added = steps.len();
        if added > 0 {
            // The file is read again: this is the only writer while a recording is on.
            let saved = self.store.load_scenario(&session.scenario_id).and_then(|mut scenario| {
                match scenario.graph.as_mut() {
                    // A chain below the blocks; it joins the start only in an empty graph.
                    Some(graph) => graph.append_chain(&steps),
                    None => scenario.steps.extend(steps),
                }
                self.store.save_scenario(&scenario)
            });
            if let Err(error) = saved {
                added = 0;
                self.journal.add_with_reason("recordSaveFailed", &[("name", name.clone())], &error);
            }
        }
        self.hub.set(&RunStatus::default());
        self.hub.flush();
        self.journal.add(
            "recordEnd",
            &[("name", name), ("added", added.to_string()), ("reason", reason.as_str().to_string())],
        );
        *lock(&self.scenario_id) = None;
        self.active.store(false, Ordering::SeqCst);
        (self.on_finished)(&RecordFinished { scenario_id: session.scenario_id, added, reason });
    }
}

#[cfg(test)]
mod tests {
    use super::Hold;

    #[test]
    fn the_clock_of_a_recording_stands_still_during_a_hold() {
        let mut hold = Hold::default();
        assert_eq!(hold.clock(700), 700);

        assert!(hold.set(true, 1_000));
        // Asking again changes nothing: the hold began once.
        assert!(!hold.set(true, 1_500));
        assert!(hold.is_on());
        assert_eq!(hold.clock(900), 900);
        assert_eq!(hold.clock(1_000), 1_000);
        assert_eq!(hold.clock(4_000), 1_000);

        assert!(!hold.set(false, 6_000));
        assert!(!hold.is_on());
        // Five seconds of the hold are gone: 6.3 s of the recording is 1.3 s of its steps.
        assert_eq!(hold.clock(6_300), 1_300);

        assert!(hold.set(true, 7_000));
        assert_eq!(hold.clock(9_000), 2_000);
        hold.set(false, 10_000);
        assert_eq!(hold.clock(10_000), 2_000);
    }
}
