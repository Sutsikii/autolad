use autolad_mcp::Engine;
use tokio_util::sync::CancellationToken;

use crate::error::AppError;

/// Shared application state, managed by Tauri.
pub struct AppState {
    /// Cancelled on shutdown; long jobs derive child tokens from it.
    pub shutdown: CancellationToken,
    /// Kept as a message when the ffmpeg sidecar is missing, so the window still opens
    /// and every command can explain what is wrong.
    engine: Result<Engine, String>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            shutdown: CancellationToken::new(),
            engine: Engine::discover().map_err(|e| e.to_string()),
        }
    }

    pub fn engine(&self) -> Result<&Engine, AppError> {
        self.engine
            .as_ref()
            .map_err(|message| AppError::Internal(message.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_token_starts_live_and_propagates_to_children() {
        let state = AppState::new();
        let child = state.shutdown.child_token();
        assert!(!child.is_cancelled());
        state.shutdown.cancel();
        assert!(child.is_cancelled());
    }
}
