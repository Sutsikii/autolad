//! Integration tests against the real sidecar binaries.
//! Run `scripts/fetch-ffmpeg.ps1 (or .sh on macOS)` first (or set AUTOLAD_FFMPEG_DIR).

// clippy's allow-unwrap-in-tests doesn't cover helper fns in integration-test files.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use autolad_core::edl::{build_silence_cut_edl, SilenceSettings};
use autolad_core::ports::RenderOptions;
use autolad_core::{Asset, AssetId, Cut, Edl, TimeRange};
use autolad_media::encoder::max_bitrate_kbps;
use autolad_media::frame::extract_frame;
use autolad_media::preview::{
    build_proxy, build_thumbnail_strip, build_waveform, strip_layout, PEAKS_PER_SECOND,
};
use autolad_media::{Binaries, Encoder, FfmpegAnalyzer, FfmpegRenderer, FfprobeProbe};

fn binaries() -> Binaries {
    if let Ok(found) = Binaries::discover_in(&[std::env::var_os("AUTOLAD_FFMPEG_DIR")
        .map(PathBuf::from)
        .unwrap_or_default()])
    {
        return found;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/binaries");
    let b = Binaries::sidecars_in(&dir);
    assert!(
        b.ffmpeg.is_file() && b.ffprobe.is_file(),
        "ffmpeg sidecar missing: run `scripts/fetch-ffmpeg.ps1 (or .sh on macOS)`"
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
        has_audio: true,
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
    loudness: Some(-14.0),
    subtitles: None,
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

/// Pure noise with sound: the worst case for an encoder's bitrate.
fn make_noise_clip(b: &Binaries, path: &Path) {
    let status = Command::new(&b.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg("nullsrc=s=1280x720:r=30:d=10,geq=lum='random(1)*255':cb=128:cr=128")
        .args(["-f", "lavfi", "-i", "sine=duration=10"])
        .args(["-c:v", "libx264", "-qp", "0", "-preset", "ultrafast"])
        .args(["-c:a", "aac", "-shortest"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn renders_stay_under_the_bitrate_cap_even_on_noise() {
    let b = binaries();
    let dir = scratch("bitrate");
    let clip = dir.join("noise.mp4");
    make_noise_clip(&b, &clip);
    let edl = Edl {
        cuts: vec![cut("a", 0.0, 10.0)],
    };
    let options = RenderOptions {
        width: 1280,
        height: 720,
        fps: 30.0,
        ..OPTS
    };
    let mut encoders = vec![Encoder::X264];
    let detected = Encoder::detect(&b).await;
    if detected.is_hardware() {
        encoders.push(detected);
    }
    for encoder in encoders {
        let out = dir.join(format!("{encoder:?}.mp4"));
        FfmpegRenderer::new(b.clone(), encoder)
            .render_edl(&[asset("a", &clip)], &edl, &options, &out, &|_| {})
            .await
            .unwrap();
        let megabits = std::fs::metadata(&out).unwrap().len() as f64 * 8.0 / 1e6 / 10.0;
        let cap = f64::from(max_bitrate_kbps(1280, 720, 30.0)) / 1000.0;
        // The cap plus its two-second buffer spread over 10 s, the audio and the container.
        assert!(
            megabits < cap * 1.2 + 0.5,
            "{encoder:?}: {megabits:.1} Mb/s"
        );
    }
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

/// Integrated loudness of a file in LUFS, as ffmpeg's EBU R128 meter reports it.
fn integrated_loudness(b: &Binaries, path: &Path) -> f64 {
    let output = Command::new(&b.ffmpeg)
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(path)
        .args(["-af", "ebur128", "-f", "null", "-"])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stderr);
    // The summary comes last: "Integrated loudness:\n    I:         -23.0 LUFS".
    let summary = log.rsplit("Integrated loudness:").next().unwrap();
    let value = summary
        .split("I:")
        .nth(1)
        .unwrap()
        .split("LUFS")
        .next()
        .unwrap();
    value.trim().parse().unwrap()
}

#[tokio::test]
async fn renders_are_normalized_to_the_loudness_target() {
    let b = binaries();
    let dir = scratch("loudness");
    let clip = dir.join("clip.mp4");
    make_clip(&b, &clip, "320x240");
    // The two parts with sound: a jump cut in the middle, faded on both sides.
    let edl = Edl {
        cuts: vec![cut("a", 0.0, 1.0), cut("a", 2.0, 3.0)],
    };
    let render = |loudness: Option<f64>, name: &str| {
        let out = dir.join(name);
        let options = RenderOptions { loudness, ..OPTS };
        let renderer = FfmpegRenderer::new(b.clone(), Encoder::X264);
        let assets = [asset("a", &clip)];
        let edl = edl.clone();
        async move {
            renderer
                .render_edl(&assets, &edl, &options, &out, &|_| {})
                .await
                .unwrap();
            out
        }
    };

    let quiet = render(Some(-30.0), "quiet.mp4").await;
    let loud = render(Some(-16.0), "loud.mp4").await;
    let (quiet, loud) = (
        integrated_loudness(&b, &quiet),
        integrated_loudness(&b, &loud),
    );
    assert!(
        (quiet + 30.0).abs() < 2.0,
        "quiet render measured {quiet} LUFS"
    );
    assert!(
        (loud + 16.0).abs() < 2.0,
        "loud render measured {loud} LUFS"
    );
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

/// Gray pixels of the bottom quarter of the frame at `time`: where subtitles are drawn.
fn bottom_quarter(b: &Binaries, video: &Path, time: f64) -> Vec<u8> {
    let output = Command::new(&b.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &time.to_string(),
            "-i",
        ])
        .arg(video)
        .args(["-frames:v", "1", "-vf", "crop=iw:ih/4:0:ih*3/4,format=gray"])
        .args(["-f", "rawvideo", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}

#[tokio::test]
async fn subtitles_are_burnt_into_the_picture() {
    let b = binaries();
    let dir = scratch("subtitles");
    let clip = dir.join("clip.mp4");
    make_clip(&b, &clip, "320x240");
    let srt = dir.join("subs-1.srt");
    std::fs::write(&srt, "1\n00:00:00,000 --> 00:00:01,000\nHELLO WORLD\n\n").unwrap();

    let edl = Edl {
        cuts: vec![cut("a", 0.0, 2.0)],
    };
    let render = |subtitles: Option<PathBuf>, name: &str| {
        let out = dir.join(name);
        let options = RenderOptions { subtitles, ..OPTS };
        let renderer = FfmpegRenderer::new(b.clone(), Encoder::X264);
        let assets = [asset("a", &clip)];
        let edl = edl.clone();
        async move {
            renderer
                .render_edl(&assets, &edl, &options, &out, &|_| {})
                .await
                .unwrap();
            out
        }
    };
    let plain = render(None, "plain.mp4").await;
    let captioned = render(Some(srt), "captioned.mp4").await;

    let changed = |time: f64| {
        let (a, b) = (
            bottom_quarter(&b, &plain, time),
            bottom_quarter(&b, &captioned, time),
        );
        a.iter()
            .zip(&b)
            .filter(|(x, y)| x.abs_diff(**y) > 64)
            .count()
    };
    // The caption shows during its second only, as white text.
    assert!(
        changed(0.5) > 200,
        "no caption drawn: {} pixels changed",
        changed(0.5)
    );
    assert!(
        changed(1.5) < 20,
        "caption still shown: {} pixels changed",
        changed(1.5)
    );
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

#[tokio::test]
async fn editor_media_is_built_from_a_real_clip() {
    let b = binaries();
    let dir = scratch("preview");
    let source = dir.join("clip.mp4");
    make_clip(&b, &source, "1280x720");

    let proxy = dir.join("proxy.mp4");
    build_proxy(&b, &source, &proxy).await.unwrap();
    let info = FfprobeProbe::new(b.clone())
        .probe_file(&proxy)
        .await
        .unwrap();
    assert_eq!(info.height, Some(540));
    assert!(info.has_audio && info.has_video);

    let strip = dir.join("strip.jpg");
    let layout = strip_layout(info.duration);
    build_thumbnail_strip(&b, &proxy, &strip, layout, 78)
        .await
        .unwrap();
    assert!(std::fs::metadata(&strip).unwrap().len() > 0);

    let wave = dir.join("wave.bin");
    build_waveform(&b, &source, &wave).await.unwrap();
    let peaks = std::fs::read(&wave).unwrap();
    // 3 s at PEAKS_PER_SECOND, give or take the encoder's padding.
    assert!(peaks.len().abs_diff(3 * PEAKS_PER_SECOND as usize) <= 10);
    // The tone is muted between 1 s and 2 s: loud outside, silent inside.
    // ffmpeg's sine has amplitude 0.125, about 90 once compressed.
    assert!(peaks[50] > 60, "tone should be loud at 0.5 s");
    assert!(peaks[150] < 10, "gap should be silent at 1.5 s");

    // No half-written leftovers next to the published files.
    let leftovers = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".part."))
        .count();
    assert_eq!(leftovers, 0);
}
