use std::path::Path;

use autolad_core::ports::Analyzer;
use autolad_core::{CoreError, TimeRange};

use crate::binaries::Binaries;
use crate::error::MediaError;
use crate::parse::parse_silencedetect;
use crate::probe::FfprobeProbe;
use crate::process::{capture, command};

/// Silence detection through ffmpeg's `silencedetect` filter.
#[derive(Debug, Clone)]
pub struct FfmpegAnalyzer {
    binaries: Binaries,
    /// Audio below this level (dBFS, negative) counts as silence.
    pub noise_db: f64,
    /// Quieter stretches shorter than this (seconds) are ignored.
    pub min_silence: f64,
}

impl FfmpegAnalyzer {
    pub fn new(binaries: Binaries) -> Self {
        Self {
            binaries,
            noise_db: -30.0,
            min_silence: 0.3,
        }
    }

    pub async fn detect(&self, path: &Path) -> Result<Vec<TimeRange>, MediaError> {
        // The duration is needed to close a silence that runs to the end of the file.
        let info = FfprobeProbe::new(self.binaries.clone())
            .probe_file(path)
            .await?;
        if !info.has_audio {
            return Err(MediaError::NoAudio);
        }

        let filter = format!(
            "silencedetect=noise={}dB:d={}",
            self.noise_db, self.min_silence
        );
        let mut cmd = command(&self.binaries.ffmpeg);
        cmd.args(["-hide_banner", "-nostats", "-i"])
            .arg(path)
            .args(["-vn", "-af", &filter, "-f", "null", "-"]);
        let out = capture("ffmpeg", cmd).await?;
        parse_silencedetect(&String::from_utf8_lossy(&out.stderr), info.duration)
    }
}

impl Analyzer for FfmpegAnalyzer {
    async fn detect_silences(&self, path: &Path) -> Result<Vec<TimeRange>, CoreError> {
        Ok(self.detect(path).await?)
    }
}
