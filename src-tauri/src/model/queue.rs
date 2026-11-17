use serde::{Deserialize, Serialize};

use super::scenario::{new_id, Pause, RepeatMode};

pub const QUEUE_VERSION: u32 = 1;

fn rounds_default() -> i64 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub id: String,
    pub scenario_id: String,
    /// How many rounds of the scenario this item runs. The scenario's own
    /// repeat setting is ignored inside a queue.
    #[serde(default = "rounds_default")]
    pub rounds: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnItemFailure {
    #[default]
    Stop,
    Next,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Queue {
    #[serde(default)]
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub items: Vec<QueueItem>,
    #[serde(default)]
    pub pause_between_items: Pause,
    #[serde(default)]
    pub repeat: RepeatMode,
    #[serde(default)]
    pub on_item_failure: OnItemFailure,
}

impl Queue {
    pub fn new(name: &str) -> Self {
        Self {
            version: QUEUE_VERSION,
            id: new_id("q"),
            name: name.to_string(),
            items: Vec::new(),
            pause_between_items: Pause::default(),
            repeat: RepeatMode::Once,
            on_item_failure: OnItemFailure::Stop,
        }
    }
}
