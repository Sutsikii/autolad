use autolad_core::CoreError;
use autolad_media::MediaError;
use autolad_transcribe::TranscribeError;
use thiserror::Error;

/// Every failure the engine can report. Messages are written for an agent reading
/// them: they say what went wrong and, where possible, what to do next.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("{0}")]
    Core(#[from] CoreError),
    #[error("{0}")]
    Media(#[from] MediaError),
    #[error("{0}")]
    Transcribe(#[from] TranscribeError),
    #[error("unknown asset id {0:?}: call list_assets to see imported assets")]
    UnknownAsset(String),
    #[error("unknown render job {0:?}")]
    UnknownJob(String),
    #[error("{0}")]
    Invalid(String),
    #[error("file error: {0}")]
    Io(String),
}
