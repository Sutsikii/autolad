use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum TranscribeError {
    #[error("whisper model not installed: {0}")]
    ModelMissing(PathBuf),
    #[error("model download failed: {0}")]
    Download(String),
    #[error("model file is corrupt (sha256 expected {expected}, got {actual})")]
    HashMismatch { expected: String, actual: String },
    #[error("invalid language {0:?}: use \"auto\" or an ISO 639-1 code such as \"fr\"")]
    InvalidLanguage(String),
    #[error("audio extraction failed: {0}")]
    Audio(String),
    #[error("whisper failed: {0}")]
    Whisper(String),
    #[error("transcription task crashed: {0}")]
    Task(String),
}

impl From<TranscribeError> for autolad_core::CoreError {
    fn from(e: TranscribeError) -> Self {
        autolad_core::CoreError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_converts_to_core_error() {
        let e: autolad_core::CoreError = TranscribeError::Whisper("boom".into()).into();
        assert_eq!(
            e,
            autolad_core::CoreError::Io("whisper failed: boom".into())
        );
    }
}
