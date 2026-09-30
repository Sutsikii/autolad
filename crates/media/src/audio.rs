//! Audio extraction for speech recognition.

use std::path::Path;

use crate::binaries::Binaries;
use crate::error::MediaError;
use crate::process::{capture, command};

/// Sample rate whisper models are trained on.
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Decodes the first audio track to mono 16 kHz `f32` samples in memory.
///
/// One hour of audio is about 230 MB, which is acceptable for a desktop app and
/// avoids temp files; revisit with chunked decoding if longer inputs matter.
pub async fn decode_pcm_16k_mono(binaries: &Binaries, path: &Path) -> Result<Vec<f32>, MediaError> {
    let mut cmd = command(&binaries.ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-v", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar"])
        .arg(WHISPER_SAMPLE_RATE.to_string())
        .args(["-f", "f32le", "-"]);
    let out = capture("ffmpeg", cmd).await?;
    if out.stdout.is_empty() {
        return Err(MediaError::NoAudio);
    }
    Ok(bytes_to_f32(&out.stdout))
}

fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::bytes_to_f32;

    #[test]
    fn converts_little_endian_floats() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0.5f32.to_le_bytes());
        bytes.extend_from_slice(&(-1.0f32).to_le_bytes());
        assert_eq!(bytes_to_f32(&bytes), vec![0.5, -1.0]);
    }

    #[test]
    fn trailing_partial_sample_is_dropped() {
        assert_eq!(bytes_to_f32(&[0, 0, 0, 0, 1, 2]), vec![0.0]);
    }
}
