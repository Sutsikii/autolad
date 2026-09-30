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
use autolad_mcp::engine::{BuildEdlRequest, Engine, FrameTarget, RenderRequest, TranscribeRequest};
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
