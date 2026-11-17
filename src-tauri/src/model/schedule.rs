use serde::{Deserialize, Serialize};

pub const MIN_INTERVAL_MINUTES: i64 = 1;
pub const MAX_INTERVAL_MINUTES: i64 = 24 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    Scenario,
    Queue,
}

/// What to run: a scenario or a queue, by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTarget {
    pub kind: TargetKind,
    pub id: String,
}

/// Time-of-day window of an interval rule, local `HH:MM`. `from > to` wraps
/// over midnight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayWindow {
    pub from: String,
    pub to: String,
}

/// All times are local. Date-times are `YYYY-MM-DDTHH:MM`, times are `HH:MM`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RuleWhen {
    Once { at: String },
    Daily { time: String },
    /// `days`: 1 = Monday .. 7 = Sunday.
    Weekly { days: Vec<u8>, time: String },
    /// Slots sit on a fixed grid: `start_at`, `start_at + every_minutes`, ...
    Interval {
        every_minutes: i64,
        start_at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<DayWindow>,
    },
}

fn enabled_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRule {
    pub id: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    pub target: RunTarget,
    pub when: RuleWhen,
}
