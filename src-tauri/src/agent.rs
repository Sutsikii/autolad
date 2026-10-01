//! Shows an AI agent at work: serves MCP to agents from inside the app (same engine, same
//! project as the UI) and forwards what they do to the front as events.

use std::sync::Arc;
use std::time::Duration;

use autolad_mcp::agent::{AgentEvent, AgentLink, AgentPhase};
use autolad_mcp::bridge;
use autolad_mcp::{AutoladServer, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Runtime};
use tauri_specta::Event;

/// Leaves the front time to glide the cursor to the target before the action runs.
const PACE: Duration = Duration::from_millis(900);

/// One step of what an agent is doing, for the virtual cursor.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct AgentActivity {
    pub tool: String,
    pub phase: AgentPhase,
    pub label: String,
    pub index: Option<usize>,
    pub time: Option<f64>,
    pub changes_project: bool,
    /// Render the agent started: the export bar follows it.
    pub job_id: Option<String>,
}

impl From<AgentEvent> for AgentActivity {
    fn from(event: AgentEvent) -> Self {
        Self {
            tool: event.tool,
            phase: event.phase,
            label: event.label,
            index: event.index,
            time: event.time,
            changes_project: event.changes_project,
            job_id: event.job_id,
        }
    }
}

/// Starts accepting agent connections. A failure only costs the live link: the app keeps
/// working and agents fall back to a standalone engine.
pub fn start_bridge<R: Runtime>(app: AppHandle<R>, engine: Arc<Engine>) {
    let listener = Arc::new(move |event: AgentEvent| {
        // Nobody listening (window closing) is not an error worth reporting.
        let _ = AgentActivity::from(event).emit(&app);
    });
    let server = AutoladServer::shared(engine, Some(AgentLink::new(listener, PACE)));

    tauri::async_runtime::spawn(async move {
        let data_dir = autolad_mcp::paths::data_dir();
        match bridge::host(server, &data_dir).await {
            // Kept alive by the runtime until exit; `clear_info` runs on shutdown.
            Ok(running) => std::mem::forget(running),
            Err(e) => eprintln!("agent bridge unavailable: {e}"),
        }
    });
}

/// Stops agents from trying to reach an app that is gone.
pub fn stop_bridge() {
    bridge::clear_info(&autolad_mcp::paths::data_dir());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_keeps_every_field_of_the_agent_event() {
        let activity = AgentActivity::from(AgentEvent {
            tool: "edit_edl".into(),
            phase: AgentPhase::Started,
            label: "Splitting clip 2".into(),
            index: Some(1),
            time: None,
            changes_project: true,
            job_id: None,
        });
        assert_eq!(activity.tool, "edit_edl");
        assert_eq!(activity.index, Some(1));
        assert!(activity.changes_project);
    }
}
