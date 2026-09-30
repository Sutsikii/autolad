//! "Connect Claude": registers this app as an MCP server in Claude Desktop and Claude Code.

use autolad_mcp::claude_setup::{self, ClaudeApp, ClaudeSetup, Locations};

use crate::error::AppError;

/// Whether Claude Desktop and Claude Code are installed, and already connected to AutoLad.
#[tauri::command]
#[specta::specta]
pub async fn claude_setup() -> Result<ClaudeSetup, AppError> {
    // Reads a few config files: kept off the main thread.
    tokio::task::spawn_blocking(|| Ok(claude_setup::status(&Locations::discover()?)))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

#[tauri::command]
#[specta::specta]
pub async fn connect_claude(app: ClaudeApp) -> Result<ClaudeSetup, AppError> {
    let at = Locations::discover()?;
    Ok(claude_setup::connect(app, &at).await?)
}
