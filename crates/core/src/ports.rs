//! Traits at I/O boundaries only. Implemented by `media` and `transcribe`;
//! each also gets a test mock, which is what justifies the abstraction.

use std::future::Future;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::{Asset, Edl, TimeRange};
use crate::error::CoreError;

/// Metadata extracted from a media file.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub duration: f64,
    pub has_audio: bool,
    pub has_video: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
}

/// Output format of a render. All cuts are conformed to it so rushes with
/// different resolutions or frame rates can be joined.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Integrated loudness the audio is normalized to, in LUFS (EBU R128). `None` keeps the
    /// levels as recorded.
    pub loudness: Option<f64>,
    /// SubRip file burnt into the picture.
    pub subtitles: Option<PathBuf>,
}

/// Receives render progress as a fraction in `[0, 1]`.
pub type ProgressFn<'a> = &'a (dyn Fn(f64) + Send + Sync);

pub trait MediaProbe: Send + Sync {
    fn probe(&self, path: &Path) -> impl Future<Output = Result<MediaInfo, CoreError>> + Send;
}

/// Detects silent ranges in an asset's audio track.
pub trait Analyzer: Send + Sync {
    fn detect_silences(
        &self,
        path: &Path,
    ) -> impl Future<Output = Result<Vec<TimeRange>, CoreError>> + Send;
}

/// Renders an EDL from its source assets. Dropping the returned future cancels the render.
pub trait Renderer: Send + Sync {
    fn render(
        &self,
        assets: &[Asset],
        edl: &Edl,
        options: &RenderOptions,
        output: &Path,
        progress: ProgressFn<'_>,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}

/// One transcribed phrase with its position on the source timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub range: TimeRange,
    pub text: String,
}

pub trait Transcriber: Send + Sync {
    fn transcribe(
        &self,
        path: &Path,
    ) -> impl Future<Output = Result<Vec<TranscriptSegment>, CoreError>> + Send;
}
