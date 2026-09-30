//! whisper-rs implementation of `Transcriber`, plus model management.
//! GPU backend is chosen at compile time (`vulkan` by default, `cuda` optional).

pub mod download;
pub mod error;
pub mod models;
pub mod whisper;

pub use error::TranscribeError;
pub use models::{ModelStore, WhisperModel};
pub use whisper::{TranscribeOptions, WhisperTranscriber};
