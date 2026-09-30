use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum MediaError {
    #[error("malformed ffmpeg output: {0}")]
    Parse(String),
    #[error("cannot start {tool}: {reason}")]
    Spawn { tool: &'static str, reason: String },
    #[error("{tool} failed (exit code {code:?}): {stderr}")]
    Failed {
        tool: &'static str,
        code: Option<i32>,
        stderr: String,
    },
    #[error("ffmpeg/ffprobe not found (looked in: {0})")]
    BinariesNotFound(String),
    #[error("cannot read file: {0}")]
    Io(String),
    #[error("the file has no audio track")]
    NoAudio,
    #[error("the EDL has no cuts to render")]
    EmptyEdl,
    #[error("EDL references an unknown asset: {0}")]
    UnknownAsset(String),
    #[error("invalid render options: {0}")]
    InvalidOptions(&'static str),
}

impl From<MediaError> for autolad_core::CoreError {
    fn from(e: MediaError) -> Self {
        match e {
            MediaError::UnknownAsset(id) => autolad_core::CoreError::UnknownAsset(id),
            other => autolad_core::CoreError::Io(other.to_string()),
        }
    }
}
