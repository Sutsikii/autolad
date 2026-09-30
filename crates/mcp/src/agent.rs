//! What an AI agent is doing, reported to whoever hosts the server (the desktop UI) so it can
//! show it live. The engine itself knows nothing about this.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum AgentPhase {
    /// The agent just asked for the action; the UI has time to move a cursor there.
    Started,
    Finished,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentEvent {
    pub tool: String,
    pub phase: AgentPhase,
    /// Short sentence for a speech bubble, e.g. "Importing take1.mp4".
    pub label: String,
    /// Cut the action is about, when there is one.
    pub index: Option<usize>,
    /// Timeline position the action is about, in seconds, when there is one.
    pub time: Option<f64>,
    /// The project changed: the UI must reload it once the action is finished.
    pub changes_project: bool,
}

pub type AgentListener = Arc<dyn Fn(AgentEvent) + Send + Sync>;

/// Where agent activity goes, and how long to wait after announcing an action so a human
/// watching the UI can follow it.
#[derive(Clone)]
pub struct AgentLink {
    pub listener: AgentListener,
    pub pace: Duration,
}

impl AgentLink {
    pub fn new(listener: AgentListener, pace: Duration) -> Self {
        Self { listener, pace }
    }
}
