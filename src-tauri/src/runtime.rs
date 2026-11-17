//! Payloads of commands and events, mirrored by `src/types/runtime.ts`, and
//! the hub that holds the live status of a run or a recording.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::model::schedule::TargetKind;

/// Positions inside a run are sent at most this often; state changes go out at once.
const STATUS_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub system_language: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    #[default]
    Idle,
    Countdown,
    Running,
    Waiting,
    PausedWindow,
    PausedProcess,
    Recording,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetInfo {
    pub kind: TargetKind,
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueProgress {
    pub item_index: usize,
    pub item_count: usize,
    pub round: i64,
    pub rounds_total: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingProgress {
    pub scenario_id: String,
    pub steps: usize,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    pub state: RunState,
    pub target: Option<TargetInfo>,
    pub countdown_left: Option<u32>,
    pub scenario_id: Option<String>,
    pub scenario_name: Option<String>,
    pub round: i64,
    pub rounds_total: Option<i64>,
    pub step_id: Option<String>,
    pub step_label: Option<String>,
    pub loop_pass: Option<i64>,
    pub loop_total: Option<i64>,
    pub step_repeat: Option<i64>,
    pub step_repeat_total: Option<i64>,
    pub queue: Option<QueueProgress>,
    pub blocked_process: Option<String>,
    pub recording: Option<RecordingProgress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Completed,
    Stopped,
    Failed,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Completed => "completed",
            Outcome::Stopped => "stopped",
            Outcome::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFinished {
    pub target: TargetInfo,
    pub outcome: Outcome,
    pub reason: Option<AppError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordStopReason {
    User,
    TimeLimit,
    StepLimit,
    Exit,
}

impl RecordStopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            RecordStopReason::User => "user",
            RecordStopReason::TimeLimit => "timeLimit",
            RecordStopReason::StepLimit => "stepLimit",
            RecordStopReason::Exit => "exit",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordFinished {
    pub scenario_id: String,
    pub added: usize,
    pub reason: RecordStopReason,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    pub x: i32,
    pub y: i32,
    pub client_x: Option<i32>,
    pub client_y: Option<i32>,
    pub color: String,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    /// Common part of two rectangles; `None` when they do not overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then(|| Rect { x, y, width: right - x, height: bottom - y })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegionPurpose {
    Template,
    Area,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInfo {
    pub file: String,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionResult {
    pub purpose: RegionPurpose,
    pub rect: Rect,
    pub client_rect: Option<Rect>,
    pub template: Option<TemplateInfo>,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub time: String,
    pub code: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowInfo {
    pub title: String,
    pub process: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateState {
    /// Nothing was checked in this session yet.
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer version exists, but this copy cannot install it by itself.
    Available,
    Downloading,
    /// Downloaded; the installation waits for a run or a recording to end.
    Ready,
    Installing,
    Failed,
}

/// Payload of the `update:status` event and of the `update_status` command.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub state: UpdateState,
    /// The newer version, once one is found.
    pub version: Option<String>,
    /// True for a copy that was not put in place by the installer.
    pub portable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DataKind {
    Scenarios,
    Queues,
    Schedule,
    Settings,
}

struct HubState {
    current: RunStatus,
    sent: RunStatus,
    sent_at: Instant,
}

/// Holds the live status and forwards it to the UI. Changes of `state` are
/// sent immediately, everything else is throttled; `flush` sends what is pending.
pub struct StatusHub {
    state: Mutex<HubState>,
    sink: Box<dyn Fn(&RunStatus) + Send + Sync>,
}

impl StatusHub {
    pub fn new(sink: Box<dyn Fn(&RunStatus) + Send + Sync>) -> Self {
        let state = HubState { current: RunStatus::default(), sent: RunStatus::default(), sent_at: Instant::now() };
        Self { state: Mutex::new(state), sink }
    }

    pub fn get(&self) -> RunStatus {
        self.lock().current.clone()
    }

    pub fn set(&self, status: &RunStatus) {
        let to_send = {
            let mut state = self.lock();
            if state.current != *status {
                state.current = status.clone();
            }
            let urgent = state.current.state != state.sent.state;
            if state.current != state.sent && (urgent || state.sent_at.elapsed() >= STATUS_INTERVAL) {
                state.sent = state.current.clone();
                state.sent_at = Instant::now();
                Some(state.sent.clone())
            } else {
                None
            }
        };
        if let Some(status) = to_send {
            (self.sink)(&status);
        }
    }

    /// Sends the pending status, if any. Called before the run goes to sleep.
    pub fn flush(&self) {
        let to_send = {
            let mut state = self.lock();
            if state.current != state.sent {
                state.sent = state.current.clone();
                state.sent_at = Instant::now();
                Some(state.sent.clone())
            } else {
                None
            }
        };
        if let Some(status) = to_send {
            (self.sink)(&status);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HubState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
