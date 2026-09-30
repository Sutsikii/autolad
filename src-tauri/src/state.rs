use tokio_util::sync::CancellationToken;

/// Shared application state, managed by Tauri.
pub struct AppState {
    /// Cancelled on shutdown; long jobs derive child tokens from it.
    pub shutdown: CancellationToken,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            shutdown: CancellationToken::new(),
        }
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
