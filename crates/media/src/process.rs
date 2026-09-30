//! Thin wrapper around `tokio::process` shared by every ffmpeg/ffprobe call.

use std::path::Path;
use std::process::{Output, Stdio};

use tokio::process::Command;

use crate::error::MediaError;

pub(crate) fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    // Dropping the future must stop ffmpeg: this is how renders are cancelled.
    cmd.kill_on_drop(true).stdin(Stdio::null());
    // CREATE_NO_WINDOW: no console flashing when launched from the GUI.
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    cmd
}

pub(crate) async fn capture(tool: &'static str, mut cmd: Command) -> Result<Output, MediaError> {
    let out = cmd.output().await.map_err(|e| spawn_error(tool, &e))?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(MediaError::Failed {
            tool,
            code: out.status.code(),
            stderr: tail(&String::from_utf8_lossy(&out.stderr)),
        })
    }
}

pub(crate) fn spawn_error(tool: &'static str, e: &std::io::Error) -> MediaError {
    MediaError::Spawn {
        tool,
        reason: e.to_string(),
    }
}

/// Keeps error messages readable: ffmpeg's useful line is at the end.
pub(crate) fn tail(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().rev().take(12).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::tail;

    #[test]
    fn tail_keeps_last_lines() {
        let text = (1..=30)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let out = tail(&text);
        assert!(out.ends_with("30"));
        assert!(!out.contains("\n1\n"));
        assert_eq!(out.lines().count(), 12);
    }
}
