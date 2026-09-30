//! Engine behaviour against the real ffmpeg sidecar (no MCP transport involved).
//! Run `pwsh scripts/fetch-ffmpeg.ps1` first (or set AUTOLAD_FFMPEG_DIR).

// clippy's allow-unwrap-in-tests doesn't cover helper fns in integration-test files.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use autolad_core::edl::SilenceSettings;
use autolad_core::edl_edit::EdlOp;
use autolad_core::AssetId;
use autolad_mcp::engine::{
    BuildEdlRequest, CutTextRequest, Engine, FrameTarget, Occurrence, RenderRequest,
    TranscribeRequest,
};
use autolad_mcp::jobs::JobState;
use autolad_mcp::EngineError;
use autolad_media::{Binaries, FfprobeProbe};

fn binaries() -> Binaries {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/binaries");
    let triple = "x86_64-pc-windows-msvc";
    let b = Binaries {
        ffmpeg: dir.join(format!("ffmpeg-{triple}.exe")),
        ffprobe: dir.join(format!("ffprobe-{triple}.exe")),
    };
    assert!(
        b.ffmpeg.is_file(),
        "ffmpeg sidecar missing: run `pwsh scripts/fetch-ffmpeg.ps1`"
    );
    b
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("autolad-mcp-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn engine(dir: &Path) -> Engine {
    Engine::new(binaries(), dir.join("data"))
}

/// Test clip: test pattern + tone muted between 1 s and 2 s.
fn make_clip(path: &Path, seconds: u32, size: &str) {
    let b = binaries();
    let status = Command::new(&b.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "lavfi", "-i"])
        .arg(format!("testsrc=size={size}:rate=25:duration={seconds}"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={seconds}"))
        .args(["-af", "volume=enable='between(t,1,2)':volume=0"])
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .args(["-c:a", "aac", "-shortest"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

/// Test clip with no audio track at all (like a screen recording).
fn make_silent_clip(path: &Path, seconds: u32, size: &str) {
    let b = binaries();
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
        .arg(format!("testsrc=size={size}:rate=25:duration={seconds}"))
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

fn silence_request(asset_id: &str) -> BuildEdlRequest {
    BuildEdlRequest {
        asset_id: asset_id.to_owned(),
        settings: SilenceSettings {
            max_gap: 0.0,
            margin: 0.0,
            min_segment: 0.0,
        },
        noise_db: None,
        min_silence: None,
        append: false,
    }
}

async fn wait_for_job(engine: &Engine, id: &str) -> autolad_mcp::jobs::JobStatus {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let status = engine.render_status(id).unwrap();
        if status.state != JobState::Running {
            return status;
        }
        assert!(Instant::now() < deadline, "render job timed out");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn silence_cutting_workflow_end_to_end() {
    let dir = scratch("flow");
    let clip = dir.join("talk.mp4");
    make_clip(&clip, 3, "320x240");
    let engine = engine(&dir);

    // Import, and importing again is idempotent.
    let imported = engine.import_media(&clip).await.unwrap();
    assert!(!imported.already_imported);
    assert_eq!(imported.asset.id.len(), 12);
    let again = engine.import_media(&clip).await.unwrap();
    assert!(again.already_imported);
    assert_eq!(again.asset.id, imported.asset.id);
    let id = imported.asset.id;

    // Detection, then cache hit.
    let first = engine.detect_silences(&id, None, None).await.unwrap();
    assert_eq!(first.silences.len(), 1);
    assert!(!first.cached);
    assert!((first.silent_seconds - 1.0).abs() < 0.2);
    assert!(
        engine
            .detect_silences(&id, None, None)
            .await
            .unwrap()
            .cached
    );

    // Build the EDL: the muted second disappears.
    let built = engine
        .build_silence_edl(silence_request(&id))
        .await
        .unwrap();
    assert_eq!(built.edl.cuts.len(), 2);
    assert!((built.edl.total_duration - 2.0).abs() < 0.25);
    assert!((built.removed_seconds - 1.0).abs() < 0.25);
    assert_eq!(built.edl.cuts[1].timeline_start, built.edl.cuts[0].duration);

    // Edits: split then delete; an invalid batch changes nothing.
    let split_at = built.edl.cuts[0].start + built.edl.cuts[0].duration / 2.0;
    let edited = engine
        .edit_edl(vec![EdlOp::Split {
            index: 0,
            at: split_at,
        }])
        .await
        .unwrap();
    assert_eq!(edited.cuts.len(), 3);

    let before = engine.edl_summary().cuts.len();
    let bad = engine
        .edit_edl(vec![
            EdlOp::Delete { index: 0 },
            EdlOp::Delete { index: 50 },
        ])
        .await;
    assert!(matches!(bad, Err(EngineError::Core(_))));
    assert_eq!(engine.edl_summary().cuts.len(), before);

    let after_delete = engine
        .edit_edl(vec![EdlOp::Delete { index: 2 }])
        .await
        .unwrap();
    assert_eq!(after_delete.cuts.len(), 2);
}

#[tokio::test]
async fn preview_frames_from_timeline_and_source() {
    let dir = scratch("frames");
    let clip = dir.join("talk.mp4");
    make_clip(&clip, 3, "320x240");
    let engine = engine(&dir);
    let id = engine.import_media(&clip).await.unwrap().asset.id;
    engine
        .build_silence_edl(silence_request(&id))
        .await
        .unwrap();

    let frame = engine
        .preview_frame(FrameTarget::Timeline { time: 0.5 }, 160)
        .await
        .unwrap();
    assert_eq!(&frame.png[1..4], b"PNG");
    assert!(
        frame.description.contains("cut #0"),
        "{}",
        frame.description
    );

    let frame = engine
        .preview_frame(
            FrameTarget::Source {
                asset_id: id.clone(),
                time: 2.5,
            },
            160,
        )
        .await
        .unwrap();
    assert_eq!(&frame.png[1..4], b"PNG");

    let past_end = engine
        .preview_frame(FrameTarget::Timeline { time: 99.0 }, 160)
        .await;
    assert!(matches!(past_end, Err(EngineError::Invalid(_))));
    let bad_source = engine
        .preview_frame(
            FrameTarget::Source {
                asset_id: id,
                time: 99.0,
            },
            160,
        )
        .await;
    assert!(matches!(bad_source, Err(EngineError::Invalid(_))));
}

#[tokio::test]
async fn project_survives_save_and_reopen() {
    let dir = scratch("project");
    let clip = dir.join("talk.mp4");
    make_clip(&clip, 3, "320x240");
    let project = dir.join("project.json");

    let engine_a = engine(&dir);
    let id = engine_a.import_media(&clip).await.unwrap().asset.id;
    engine_a
        .build_silence_edl(silence_request(&id))
        .await
        .unwrap();
    engine_a.save_project(&project).await.unwrap();
    // Bound projects autosave: this edit must reach the file without another save.
    engine_a
        .edit_edl(vec![EdlOp::Delete { index: 0 }])
        .await
        .unwrap();

    let engine_b = engine(&dir);
    let report = engine_b.open_project(&project).await.unwrap();
    assert_eq!(report.assets, 1);
    assert_eq!(report.cuts, 1);
    assert!(report.missing_files.is_empty());
    assert_eq!(engine_b.project_status().assets[0].id, id);

    std::fs::remove_file(&clip).unwrap();
    let report = engine_b.open_project(&project).await.unwrap();
    assert_eq!(report.missing_files.len(), 1);
}

#[tokio::test]
async fn a_new_project_is_empty_and_no_longer_writes_to_the_old_file() {
    let dir = scratch("new-project");
    let clip = dir.join("talk.mp4");
    make_clip(&clip, 3, "320x240");
    let project = dir.join("project.json");

    let engine = engine(&dir);
    engine.import_media(&clip).await.unwrap();
    engine.save_project(&project).await.unwrap();
    let saved = std::fs::read_to_string(&project).unwrap();

    engine.new_project();
    let status = engine.project_status();
    assert!(status.assets.is_empty());
    assert!(status.edl.cuts.is_empty());
    assert!(status.project_file.is_none());

    // Work in the new project must not leak into the file of the previous one.
    engine.import_media(&clip).await.unwrap();
    engine.edit_edl(vec![EdlOp::Clear]).await.unwrap();
    assert_eq!(std::fs::read_to_string(&project).unwrap(), saved);
}

#[tokio::test]
async fn draft_render_produces_a_playable_smaller_video() {
    let dir = scratch("render");
    let clip = dir.join("talk.mp4");
    make_clip(&clip, 3, "1280x720");
    let engine = engine(&dir);
    let id = engine.import_media(&clip).await.unwrap().asset.id;
    let built = engine
        .build_silence_edl(silence_request(&id))
        .await
        .unwrap();

    let out = dir.join("draft.mp4");
    let job = engine
        .render_start(RenderRequest {
            output: out.clone(),
            draft: true,
            width: None,
            height: None,
            fps: None,
            overwrite: false,
            loudness: Some(-14.0),
        })
        .await
        .unwrap();
    let status = wait_for_job(&engine, &job).await;
    assert!(
        matches!(status.state, JobState::Done { size_bytes } if size_bytes > 0),
        "{status:?}"
    );
    assert_eq!(status.progress, 1.0);

    let info = FfprobeProbe::new(binaries())
        .probe_file(&out)
        .await
        .unwrap();
    assert_eq!((info.width, info.height), (Some(640), Some(360)));
    assert!((info.duration - built.edl.total_duration).abs() < 0.3);
    assert!(info.has_audio);
}

#[tokio::test]
async fn render_refuses_dangerous_or_pointless_requests() {
    let dir = scratch("guards");
    let clip = dir.join("talk.mp4");
    make_clip(&clip, 3, "320x240");
    let engine = engine(&dir);
    let request = |output: &Path| RenderRequest {
        output: output.to_path_buf(),
        draft: true,
        width: None,
        height: None,
        fps: None,
        overwrite: false,
        loudness: Some(-14.0),
    };

    // Nothing to render yet.
    let id = engine.import_media(&clip).await.unwrap().asset.id;
    let empty = engine.render_start(request(&dir.join("o.mp4"))).await;
    assert!(matches!(empty, Err(EngineError::Invalid(_))));

    engine
        .build_silence_edl(silence_request(&id))
        .await
        .unwrap();

    // Would destroy the source clip.
    let over_source = engine.render_start(request(&clip)).await;
    assert!(matches!(over_source, Err(EngineError::Invalid(m)) if m.contains("source")));

    // Existing output needs explicit overwrite.
    let existing = dir.join("exists.mp4");
    std::fs::write(&existing, b"keep me").unwrap();
    assert!(engine.render_start(request(&existing)).await.is_err());
    assert_eq!(std::fs::read(&existing).unwrap(), b"keep me");

    // Wrong extension, missing folder.
    assert!(engine
        .render_start(request(&dir.join("o.avi")))
        .await
        .is_err());
    assert!(engine
        .render_start(request(&dir.join("nope/o.mp4")))
        .await
        .is_err());
}

#[tokio::test]
async fn cancelling_a_render_stops_ffmpeg_and_removes_the_output() {
    let dir = scratch("cancel");
    let clip = dir.join("long.mp4");
    make_clip(&clip, 240, "640x360");
    let engine = engine(&dir);
    let id = engine.import_media(&clip).await.unwrap().asset.id;
    engine
        .edit_edl(vec![EdlOp::Insert {
            index: 0,
            asset: AssetId(id),
            start: 0.0,
            end: 239.0,
        }])
        .await
        .unwrap();

    let out = dir.join("long-out.mp4");
    let job = engine
        .render_start(RenderRequest {
            output: out.clone(),
            draft: false,
            width: None,
            height: None,
            fps: None,
            overwrite: false,
            loudness: Some(-14.0),
        })
        .await
        .unwrap();
    let status = engine.render_cancel(&job).await.unwrap();

    assert_eq!(status.state, JobState::Cancelled);
    assert!(!out.exists(), "partial output should be deleted");
    assert!(
        !dir.join("long-out.mp4.filtergraph").exists(),
        "temp filtergraph should be deleted"
    );
}

#[tokio::test]
async fn bad_inputs_give_actionable_errors() {
    let dir = scratch("errors");
    let engine = engine(&dir);

    let missing = engine.import_media(&dir.join("nope.mp4")).await;
    assert!(matches!(missing, Err(EngineError::Invalid(m)) if m.contains("no such file")));

    let unknown = engine.detect_silences("ghost", None, None).await;
    assert!(matches!(unknown, Err(EngineError::UnknownAsset(_))));

    let clip = dir.join("talk.mp4");
    make_clip(&clip, 3, "320x240");
    let id = engine.import_media(&clip).await.unwrap().asset.id;
    assert!(engine.detect_silences(&id, Some(5.0), None).await.is_err());
    assert!(engine.detect_silences(&id, None, Some(0.0)).await.is_err());

    let bad_model = engine
        .transcribe(TranscribeRequest {
            asset_id: id,
            language: None,
            model: Some("gigantic".into()),
            word_timestamps: false,
        })
        .await;
    assert!(matches!(bad_model, Err(EngineError::Invalid(m)) if m.contains("large-v3-turbo")));

    assert!(matches!(
        engine.render_status("render-999"),
        Err(EngineError::UnknownJob(_))
    ));
}

#[tokio::test]
async fn a_clip_without_sound_can_be_imported_edited_and_rendered() {
    let dir = scratch("silent");
    let silent = dir.join("screen.mp4");
    let talk = dir.join("talk.mp4");
    make_silent_clip(&silent, 4, "320x240");
    make_clip(&talk, 3, "320x240");
    let engine = engine(&dir);

    let imported = engine.import_media(&silent).await.unwrap().asset;
    assert!(!imported.has_audio);
    let silent_id = imported.id;

    // Audio analysis explains itself and points at the alternative.
    for error in [
        engine
            .detect_silences(&silent_id, None, None)
            .await
            .unwrap_err(),
        engine
            .build_silence_edl(silence_request(&silent_id))
            .await
            .unwrap_err(),
    ] {
        let EngineError::Invalid(message) = error else {
            panic!("expected an Invalid error");
        };
        assert!(message.contains("no audio track"), "{message}");
        assert!(message.contains("edit_edl"), "{message}");
    }
    let transcribe = engine
        .transcribe(TranscribeRequest {
            asset_id: silent_id.clone(),
            language: None,
            model: None,
            word_timestamps: false,
        })
        .await;
    assert!(matches!(transcribe, Err(EngineError::Invalid(_))));
    assert!(matches!(
        engine.ensure_waveform(&silent_id).await,
        Err(EngineError::Invalid(_))
    ));

    // It can still be cut by hand, next to a clip that has sound, and rendered.
    let talk_id = engine.import_media(&talk).await.unwrap().asset.id;
    assert!(engine.project_status().assets.iter().any(|a| a.has_audio));
    engine
        .edit_edl(vec![
            EdlOp::Insert {
                index: 0,
                asset: AssetId(silent_id.clone()),
                start: 0.0,
                end: 2.0,
            },
            EdlOp::Insert {
                index: 1,
                asset: AssetId(talk_id),
                start: 0.0,
                end: 3.0,
            },
        ])
        .await
        .unwrap();

    let out = dir.join("mixed.mp4");
    let job = engine
        .render_start(RenderRequest {
            output: out.clone(),
            draft: true,
            width: None,
            height: None,
            fps: None,
            overwrite: false,
            loudness: Some(-14.0),
        })
        .await
        .unwrap();
    let status = wait_for_job(&engine, &job).await;
    assert!(matches!(status.state, JobState::Done { .. }), "{status:?}");
    let info = FfprobeProbe::new(binaries())
        .probe_file(&out)
        .await
        .unwrap();
    assert!(info.has_audio, "the export keeps a (silent) audio track");
    assert!((info.duration - 5.0).abs() < 0.3, "{}", info.duration);
}

#[tokio::test]
async fn a_file_without_video_is_still_refused() {
    let dir = scratch("audio-only");
    let audio = dir.join("tone.m4a");
    let status = Command::new(binaries().ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg("sine=frequency=440:duration=2")
        .args(["-c:a", "aac"])
        .arg(&audio)
        .status()
        .unwrap();
    assert!(status.success());
    let error = engine(&dir).import_media(&audio).await.unwrap_err();
    assert!(matches!(error, EngineError::Invalid(m) if m.contains("no video track")));
}

/// One test clip of an awkward shape: what to generate and what a viewer should see.
struct Shape {
    name: &'static str,
    /// lavfi size of the coded frame.
    coded: &'static str,
    /// Extra video filter (pixel shape).
    filter: Option<&'static str>,
    pix_fmt: &'static str,
    /// Rotation flag to write without re-encoding (phone videos).
    rotate: Option<u32>,
    /// Size as displayed.
    shown: (u32, u32),
    /// Size of a draft export.
    draft: (u32, u32),
}

fn make_shape(dir: &Path, shape: &Shape) -> PathBuf {
    let b = binaries();
    let coded = dir.join(format!("{}-coded.mp4", shape.name));
    let mut filter = String::from("format=");
    filter.push_str(shape.pix_fmt);
    if let Some(extra) = shape.filter {
        filter = format!("{extra},{filter}");
    }
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
        .arg(format!("testsrc=size={}:rate=25:duration=1", shape.coded))
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
        .args(["-vf", &filter])
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&coded)
        .status()
        .unwrap();
    assert!(status.success(), "could not make {}", shape.name);

    let Some(degrees) = shape.rotate else {
        return coded;
    };
    let rotated = dir.join(format!("{}.mp4", shape.name));
    let status = Command::new(&b.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-display_rotation",
        ])
        .arg(degrees.to_string())
        .arg("-i")
        .arg(&coded)
        .args(["-c", "copy"])
        .arg(&rotated)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "could not flag the rotation of {}",
        shape.name
    );
    rotated
}

#[tokio::test]
async fn every_video_shape_is_imported_previewed_and_exported_at_the_right_size() {
    let shapes = [
        Shape {
            name: "landscape",
            coded: "640x360",
            filter: None,
            pix_fmt: "yuv420p",
            rotate: None,
            shown: (640, 360),
            draft: (640, 360),
        },
        Shape {
            name: "portrait",
            coded: "360x640",
            filter: None,
            pix_fmt: "yuv420p",
            rotate: None,
            shown: (360, 640),
            draft: (360, 640),
        },
        Shape {
            name: "phone",
            coded: "640x360",
            filter: None,
            pix_fmt: "yuv420p",
            rotate: Some(90),
            shown: (360, 640),
            draft: (360, 640),
        },
        Shape {
            name: "anamorphic",
            coded: "480x360",
            filter: Some("setsar=4/3"),
            pix_fmt: "yuv420p",
            rotate: None,
            shown: (640, 360),
            draft: (640, 360),
        },
        Shape {
            name: "tiny",
            coded: "96x54",
            filter: None,
            pix_fmt: "yuv420p",
            rotate: None,
            shown: (96, 54),
            draft: (96, 54),
        },
        Shape {
            name: "odd",
            coded: "321x241",
            filter: None,
            pix_fmt: "yuv444p",
            rotate: None,
            shown: (321, 241),
            draft: (320, 240),
        },
        Shape {
            name: "ultrawide",
            coded: "1280x320",
            filter: None,
            pix_fmt: "yuv420p",
            rotate: None,
            shown: (1280, 320),
            draft: (640, 160),
        },
        Shape {
            name: "uhd",
            coded: "3840x2160",
            filter: None,
            pix_fmt: "yuv420p",
            rotate: None,
            shown: (3840, 2160),
            draft: (640, 360),
        },
    ];
    let dir = scratch("shapes");
    let engine = engine(&dir);
    let probe = FfprobeProbe::new(binaries());

    for shape in &shapes {
        let clip = make_shape(&dir, shape);
        let asset = engine.import_media(&clip).await.unwrap().asset;
        let label = shape.name;
        assert_eq!(
            (asset.width, asset.height),
            (Some(shape.shown.0), Some(shape.shown.1)),
            "{label}: size as displayed"
        );

        // The proxy keeps the shape (within the rounding to even sides) and is never enlarged.
        let proxy = engine.ensure_proxy(&asset.id).await.unwrap();
        let proxy_info = probe.probe_file(&proxy).await.unwrap();
        let (pw, ph) = (proxy_info.width.unwrap(), proxy_info.height.unwrap());
        assert!(
            ph <= 540 && ph <= shape.shown.1 + 1,
            "{label}: proxy {pw}x{ph}"
        );
        assert!(
            pw % 2 == 0 && ph % 2 == 0,
            "{label}: proxy {pw}x{ph} must be even"
        );
        let wanted = f64::from(shape.shown.0) / f64::from(shape.shown.1);
        let got = f64::from(pw) / f64::from(ph);
        assert!(
            (wanted - got).abs() / wanted < 0.03,
            "{label}: proxy shape {got} vs {wanted}"
        );

        // Thumbnails are as wide as the picture is shaped.
        let strip = engine.ensure_thumbnails(&asset.id).await.unwrap();
        assert_eq!(
            strip.tile_width,
            autolad_media::preview::tile_width(asset.width, asset.height),
            "{label}"
        );
        assert!(
            std::fs::metadata(&strip.path).unwrap().len() > 0,
            "{label}: strip"
        );

        // A draft export has the shape of the source, bounded to 640 on its long side.
        engine.new_project();
        let asset = engine.import_media(&clip).await.unwrap().asset;
        engine
            .edit_edl(vec![EdlOp::Insert {
                index: 0,
                asset: AssetId(asset.id),
                start: 0.0,
                end: 1.0,
            }])
            .await
            .unwrap();
        let out = dir.join(format!("{label}-draft.mp4"));
        let job = engine
            .render_start(RenderRequest {
                output: out.clone(),
                draft: true,
                width: None,
                height: None,
                fps: None,
                overwrite: true,
                loudness: Some(-14.0),
            })
            .await
            .unwrap();
        let status = wait_for_job(&engine, &job).await;
        assert!(
            matches!(status.state, JobState::Done { .. }),
            "{label}: {status:?}"
        );
        let rendered = probe.probe_file(&out).await.unwrap();
        assert_eq!(
            (rendered.width, rendered.height),
            (Some(shape.draft.0), Some(shape.draft.1)),
            "{label}: draft export size"
        );
    }
}

/// Speech synthesized with the Windows voices, muxed under a test pattern.
fn make_speech_clip(dir: &Path, text: &str) -> PathBuf {
    let wav = dir.join("speech.wav");
    let script = format!(
        "Add-Type -AssemblyName System.Speech; \
         $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         $s.SelectVoice('Microsoft David Desktop'); \
         $s.SetOutputToWaveFile('{}'); $s.Speak('{text}'); $s.Dispose()",
        wav.display()
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .unwrap();
    assert!(status.success(), "speech synthesis failed");

    let clip = dir.join("speech.mp4");
    let status = Command::new(binaries().ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "lavfi", "-i", "testsrc=size=320x240:rate=25"])
        .arg("-i")
        .arg(&wav)
        .args(["-shortest", "-c:v", "libx264", "-preset", "ultrafast"])
        .args(["-pix_fmt", "yuv420p", "-c:a", "aac"])
        .arg(&clip)
        .status()
        .unwrap();
    assert!(status.success());
    clip
}

/// Real speech through whisper's word mode. Needs the Windows speech voices and the `small`
/// model (downloaded on first run), so it is not part of the default run:
/// `cargo test -p autolad-mcp --test engine -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "needs Windows speech synthesis and a Whisper model"]
async fn a_spoken_edit_is_cleaned_up_by_text() {
    let dir = scratch("speech");
    let clip = make_speech_clip(
        &dir,
        "Hello everyone. Today we are going. Today we are going to talk about video editing. \
         Cutting silences is really useful. This sentence must go away. \
         And that is all for today.",
    );
    let engine = engine(&dir);
    let asset = engine.import_media(&clip).await.unwrap().asset;
    engine
        .edit_edl(vec![EdlOp::Insert {
            index: 0,
            asset: AssetId(asset.id.clone()),
            start: 0.0,
            end: asset.duration,
        }])
        .await
        .unwrap();

    let retakes = engine.remove_retakes(true).await.unwrap();
    println!("retakes: {:#?}", retakes.removed);
    assert_eq!(retakes.removed.len(), 1, "{:?}", retakes.removed);
    assert!(retakes.removed[0].text.contains("going"));

    engine.remove_retakes(false).await.unwrap();
    let cut = engine
        .cut_text(CutTextRequest {
            text: "This sentence must go away.".into(),
            occurrence: Occurrence::Nth(1),
        })
        .await
        .unwrap();
    assert!(cut.removed_seconds > 1.0, "{cut:?}");

    let said = engine.edit_transcript();
    println!("edit says: {}", said.full_text);
    assert!(said.word_level);
    let text = said.full_text.to_lowercase();
    assert!(!text.contains("go away"), "{text}");
    assert_eq!(text.matches("today we are going").count(), 1, "{text}");
    assert!(text.contains("all for today"), "{text}");

    // The phrase view of the source is still there for the transcript tab.
    let phrases = engine.find_transcript(&asset.id).unwrap();
    assert!(phrases.segments.len() < 12, "{:?}", phrases.segments);

    // What the rendered file really says, heard by whisper again.
    let out = dir.join("cleaned.mp4");
    let job = engine
        .render_start(RenderRequest {
            output: out.clone(),
            draft: true,
            width: None,
            height: None,
            fps: None,
            overwrite: true,
            loudness: Some(-14.0),
        })
        .await
        .unwrap();
    let status = wait_for_job(&engine, &job).await;
    assert!(matches!(status.state, JobState::Done { .. }), "{status:?}");
    let rendered = engine.import_media(&out).await.unwrap().asset;
    let heard = engine
        .transcribe(TranscribeRequest {
            asset_id: rendered.id,
            language: Some("en".into()),
            model: None,
            word_timestamps: false,
        })
        .await
        .unwrap()
        .full_text
        .to_lowercase();
    println!("render says: {heard}");
    assert!(!heard.contains("go away"), "{heard}");
    assert_eq!(heard.matches("today we are going").count(), 1, "{heard}");
    assert!(heard.contains("all for today"), "{heard}");
}
