//! Derived files the editor shows instead of the heavy source: a low-definition proxy to play,
//! a strip of thumbnails and the audio peaks to draw on the timeline.

use std::path::{Path, PathBuf};

use crate::binaries::Binaries;
use crate::error::MediaError;
use crate::process::{capture, command};

/// Height of the playback proxy (see "Performance" in CLAUDE.md).
pub const PROXY_HEIGHT: u32 = 540;
/// Short GOP: seeking inside the proxy decodes at most half a second of frames.
const PROXY_GOP: u32 = 15;

pub const TILE_WIDTH: u32 = 78;
pub const TILE_HEIGHT: u32 = 44;
const MAX_TILES: u32 = 240;
const MIN_STEP: f64 = 0.5;

pub const PEAKS_PER_SECOND: u32 = 100;
const PCM_RATE: u32 = 8_000;

/// Where thumbnails sit inside the strip image: tile `i` shows the frame at `i * step`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripLayout {
    pub step: f64,
    pub tiles: u32,
}

pub fn strip_layout(duration: f64) -> StripLayout {
    let duration = if duration.is_finite() {
        duration.max(0.0)
    } else {
        0.0
    };
    let step = (duration / f64::from(MAX_TILES)).max(MIN_STEP);
    // Bounded by MAX_TILES + 1 because step >= duration / MAX_TILES.
    let tiles = ((duration / step).ceil() as u32).clamp(1, MAX_TILES + 1);
    StripLayout { step, tiles }
}

/// Builds the playback proxy. H.264/AAC in MP4 is what the webview decodes natively.
pub async fn build_proxy(
    binaries: &Binaries,
    source: &Path,
    output: &Path,
) -> Result<(), MediaError> {
    let scale = format!("scale=-2:'min({PROXY_HEIGHT},ih)'");
    let gop = PROXY_GOP.to_string();
    publish(output, |part| {
        let mut cmd = command(&binaries.ffmpeg);
        cmd.args(["-hide_banner", "-nostdin", "-y", "-i"])
            .arg(source)
            .args(["-map", "0:v:0", "-map", "0:a:0?", "-sn", "-dn"])
            .args(["-vf", &scale])
            .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "27"])
            .args(["-g", &gop, "-keyint_min", &gop, "-sc_threshold", "0"])
            .args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "128k"])
            .args(["-movflags", "+faststart"])
            .arg(part);
        cmd
    })
    .await
}

/// One JPEG holding every thumbnail side by side. Reads the proxy's keyframes only, which is
/// why it is built from the proxy and not from the source.
pub async fn build_thumbnail_strip(
    binaries: &Binaries,
    proxy: &Path,
    output: &Path,
    layout: StripLayout,
) -> Result<(), MediaError> {
    let filter = format!(
        "tpad=stop_mode=clone:stop_duration={pad},fps=1/{step},scale={TILE_WIDTH}:{TILE_HEIGHT}:force_original_aspect_ratio=increase,\
         crop={TILE_WIDTH}:{TILE_HEIGHT},tile={tiles}x1",
        step = layout.step,
        // The last keyframe sits before the end of the file: without padding the final tiles
        // would stay black.
        pad = layout.step * 2.0,
        tiles = layout.tiles,
    );
    publish(output, |part| {
        let mut cmd = command(&binaries.ffmpeg);
        cmd.args([
            "-hide_banner",
            "-nostdin",
            "-y",
            "-skip_frame",
            "nokey",
            "-i",
        ])
        .arg(proxy)
        .args(["-an", "-vf", &filter])
        .args(["-frames:v", "1", "-update", "1", "-q:v", "5"])
        .arg(part);
        cmd
    })
    .await
}

/// One byte per 10 ms: the loudest sample of that window, compressed so quiet speech is visible.
pub async fn build_waveform(
    binaries: &Binaries,
    source: &Path,
    output: &Path,
) -> Result<(), MediaError> {
    let mut cmd = command(&binaries.ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-v", "error", "-i"])
        .arg(source)
        .args(["-vn", "-ac", "1", "-ar"])
        .arg(PCM_RATE.to_string())
        .args(["-f", "s16le", "-"]);
    let pcm = capture("ffmpeg", cmd).await?.stdout;
    if pcm.is_empty() {
        return Err(MediaError::NoAudio);
    }
    let window = (PCM_RATE / PEAKS_PER_SECOND) as usize;
    let peaks = peaks_from_pcm(&pcm, window);

    let part = part_path(output);
    tokio::fs::write(&part, peaks)
        .await
        .map_err(|e| MediaError::Io(format!("{}: {e}", part.display())))?;
    finish(&part, output).await
}

/// Loudest sample of each `window` samples of little-endian 16-bit PCM, as `0..=255`.
pub fn peaks_from_pcm(pcm: &[u8], window: usize) -> Vec<u8> {
    if window == 0 {
        return Vec::new();
    }
    let samples: Vec<i16> = pcm
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    samples
        .chunks(window)
        .map(|chunk| {
            let loudest = chunk.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
            let level = f32::from(loudest) / 32_768.0;
            (level.sqrt() * 255.0).round().min(255.0) as u8
        })
        .collect()
}

/// Runs `build` against a temporary name, then renames it: a crash or a cancel never leaves a
/// half-written file that later passes for a cached one.
async fn publish(
    output: &Path,
    build: impl FnOnce(&Path) -> tokio::process::Command,
) -> Result<(), MediaError> {
    let part = part_path(output);
    let result = capture("ffmpeg", build(&part)).await;
    match result {
        Ok(_) => finish(&part, output).await,
        Err(e) => {
            // Best effort: the error being reported matters more than the leftover.
            let _ = tokio::fs::remove_file(&part).await;
            Err(e)
        }
    }
}

async fn finish(part: &Path, output: &Path) -> Result<(), MediaError> {
    tokio::fs::rename(part, output)
        .await
        .map_err(|e| MediaError::Io(format!("{}: {e}", output.display())))
}

/// `name.ext` becomes `name.part.ext`, keeping the extension ffmpeg uses to pick the format.
fn part_path(output: &Path) -> PathBuf {
    let stem = output.file_stem().unwrap_or_default().to_string_lossy();
    let name = match output.extension() {
        Some(ext) => format!("{stem}.part.{}", ext.to_string_lossy()),
        None => format!("{stem}.part"),
    };
    output.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_clips_get_one_tile_per_half_second() {
        let layout = strip_layout(20.0);
        assert_eq!(layout.step, 0.5);
        assert_eq!(layout.tiles, 40);
    }

    #[test]
    fn long_clips_are_capped_at_the_tile_budget() {
        let layout = strip_layout(3600.0);
        assert_eq!(layout.tiles, MAX_TILES);
        assert!((layout.step - 15.0).abs() < 1e-9);
    }

    #[test]
    fn degenerate_durations_still_give_one_tile() {
        assert_eq!(strip_layout(0.0).tiles, 1);
        assert_eq!(strip_layout(f64::NAN).tiles, 1);
        assert_eq!(strip_layout(-3.0).tiles, 1);
    }

    #[test]
    fn peaks_take_the_loudest_sample_per_window() {
        let mut pcm = Vec::new();
        for s in [0i16, 100, -32768, 5, 0, 0] {
            pcm.extend_from_slice(&s.to_le_bytes());
        }
        let peaks = peaks_from_pcm(&pcm, 3);
        // 5/32768 is barely audible but the square root keeps it above zero.
        assert_eq!(peaks, vec![255, 3]);
    }

    #[test]
    fn quiet_audio_is_boosted_but_silence_stays_zero() {
        let quiet = 1_000i16.to_le_bytes();
        let peaks = peaks_from_pcm(&quiet, 1);
        assert!(peaks[0] > 30);
        assert_eq!(peaks_from_pcm(&0i16.to_le_bytes(), 1), vec![0]);
        assert!(peaks_from_pcm(&[], 0).is_empty());
    }

    #[test]
    fn part_files_keep_their_extension() {
        assert_eq!(
            part_path(Path::new("c/abc.mp4")),
            PathBuf::from("c/abc.part.mp4")
        );
        assert_eq!(part_path(Path::new("abc")), PathBuf::from("abc.part"));
    }
}
