//! Integration tests against the real sidecar binaries.
//! Run `pwsh scripts/fetch-ffmpeg.ps1` first (or set AUTOLAD_FFMPEG_DIR).

// clippy's allow-unwrap-in-tests doesn't cover helper fns in integration-test files.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use autolad_core::edl::{build_silence_cut_edl, SilenceSettings};
use autolad_core::ports::RenderOptions;
use autolad_core::{Asset, AssetId, Cut, Edl, TimeRange};
use autolad_media::frame::extract_frame;
use autolad_media::{Binaries, Encoder, FfmpegAnalyzer, FfmpegRenderer, FfprobeProbe};

fn binaries() -> Binaries {
    if let Ok(found) = Binaries::discover_in(&[std::env::var_os("AUTOLAD_FFMPEG_DIR")
        .map(PathBuf::from)
        .unwrap_or_default()])
    {
        return found;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/binaries");
    let triple = "x86_64-pc-windows-msvc";
    let b = Binaries {
        ffmpeg: dir.join(format!("ffmpeg-{triple}.exe")),
        ffprobe: dir.join(format!("ffprobe-{triple}.exe")),
    };
    assert!(
        b.ffmpeg.is_file() && b.ffprobe.is_file(),
        "ffmpeg sidecar missing: run `pwsh scripts/fetch-ffmpeg.ps1`"
    );
    b
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("autolad-it-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 3 s clip: test pattern + 440 Hz tone that is muted between 1 s and 2 s.
fn make_clip(b: &Binaries, path: &Path, size: &str) {
    let status = Command::new(&b.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "lavfi", "-i"])
        .arg(format!("testsrc=size={size}:rate=25:duration=3"))
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
        .args(["-af", "volume=enable='between(t,1,2)':volume=0"])
        .args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success(), "could not generate test clip");
}

fn asset(id: &str, path: &Path) -> Asset {
    Asset {
        id: AssetId(id.into()),
        path: path.to_path_buf(),
        duration: 3.0,
    }
}

fn cut(id: &str, s: f64, e: f64) -> Cut {
    Cut {
        asset: AssetId(id.into()),
        range: TimeRange::new(s, e).unwrap(),
    }
}

const OPTS: RenderOptions = RenderOptions {
    width: 320,
    height: 240,
    fps: 25.0,
};

#[tokio::test]
async fn probe_reports_stream_info() {
    let b = binaries();
    let clip = scratch("probe").join("clip.mp4");
    make_clip(&b, &clip, "320x240");

    let info = FfprobeProbe::new(b).probe_file(&clip).await.unwrap();
    assert!((info.duration - 3.0).abs() < 0.2);
    assert!(info.has_audio && info.has_video);
    assert_eq!((info.width, info.height), (Some(320), Some(240)));
    assert_eq!(info.fps, Some(25.0));
}

#[tokio::test]
async fn detects_the_muted_second() {
    let b = binaries();
    let clip = scratch("silence").join("clip.mp4");
    make_clip(&b, &clip, "320x240");

    let silences = FfmpegAnalyzer::new(b).detect(&clip).await.unwrap();
    assert_eq!(silences.len(), 1, "got {silences:?}");
    assert!((silences[0].start - 1.0).abs() < 0.15, "got {silences:?}");
    assert!((silences[0].end - 2.0).abs() < 0.15, "got {silences:?}");
}

#[tokio::test]
async fn silence_removal_end_to_end() {
    let b = binaries();
    let dir = scratch("e2e");
    let clip = dir.join("clip.mp4");
    make_clip(&b, &clip, "320x240");
    let asset = asset("a", &clip);

    let silences = FfmpegAnalyzer::new(b.clone()).detect(&clip).await.unwrap();
    let settings = SilenceSettings {
        max_gap: 0.0,
        margin: 0.0,
        min_segment: 0.0,
    };
    let edl = build_silence_cut_edl(&asset, &silences, &settings).unwrap();
    assert_eq!(edl.cuts.len(), 2);

    let out = dir.join("out.mp4");
    let seen = std::sync::Mutex::new(Vec::new());
    FfmpegRenderer::new(b.clone(), Encoder::X264)
        .render_edl(&[asset], &edl, &OPTS, &out, &|p| {
            seen.lock().unwrap().push(p)
        })
        .await
        .unwrap();

    let info = FfprobeProbe::new(b).probe_file(&out).await.unwrap();
    assert!(
        (info.duration - edl.total_duration()).abs() < 0.25,
        "output {}s vs EDL {}s",
        info.duration,
        edl.total_duration()
    );
    assert!(info.has_audio && info.has_video);
    assert_eq!(seen.lock().unwrap().last().copied(), Some(1.0));
    assert!(
        !dir.join("out.mp4.filtergraph").exists(),
        "temp graph left behind"
    );
}

#[tokio::test]
async fn rushes_of_different_sizes_are_conformed() {
    let b = binaries();
    let dir = scratch("mixed");
    let (big, small) = (dir.join("big.mp4"), dir.join("small.mp4"));
    make_clip(&b, &big, "320x240");
    make_clip(&b, &small, "160x120");

    let edl = Edl {
        cuts: vec![
            cut("big", 0.0, 1.0),
            cut("small", 0.0, 1.0),
            cut("big", 2.0, 3.0),
        ],
    };
    let out = dir.join("out.mp4");
    FfmpegRenderer::new(b.clone(), Encoder::X264)
        .render_edl(
            &[asset("big", &big), asset("small", &small)],
            &edl,
            &OPTS,
            &out,
            &|_| {},
        )
        .await
        .unwrap();

    let info = FfprobeProbe::new(b).probe_file(&out).await.unwrap();
    assert_eq!((info.width, info.height), (Some(320), Some(240)));
    assert!((info.duration - 3.0).abs() < 0.3, "got {}s", info.duration);
}

#[tokio::test]
async fn detected_encoder_can_render() {
    let b = binaries();
    let dir = scratch("hw");
    let clip = dir.join("clip.mp4");
    make_clip(&b, &clip, "320x240");

    let encoder = Encoder::detect(&b).await;
    let edl = Edl {
        cuts: vec![cut("a", 0.0, 1.0)],
    };
    let out = dir.join("out.mp4");
    FfmpegRenderer::new(b.clone(), encoder)
        .render_edl(&[asset("a", &clip)], &edl, &OPTS, &out, &|_| {})
        .await
        .unwrap_or_else(|e| panic!("render with {encoder:?} failed: {e}"));
    assert!(std::fs::metadata(&out).unwrap().len() > 0);
}

#[tokio::test]
async fn ffmpeg_failure_carries_its_message() {
    let b = binaries();
    let dir = scratch("fail");
    let edl = Edl {
        cuts: vec![cut("a", 0.0, 1.0)],
    };
    let missing = dir.join("nope.mp4");
    let err = FfmpegRenderer::new(b, Encoder::X264)
        .render_edl(
            &[asset("a", &missing)],
            &edl,
            &OPTS,
            &dir.join("out.mp4"),
            &|_| {},
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("nope.mp4"), "got: {err}");
}

#[tokio::test]
async fn extracts_a_png_frame() {
    let b = binaries();
    let dir = scratch("frame");
    let clip = dir.join("clip.mp4");
    make_clip(&b, &clip, "320x240");

    let png = dir.join("f.png");
    extract_frame(&b, &clip, 1.5, &png, 160).await.unwrap();
    let bytes = std::fs::read(&png).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
}
