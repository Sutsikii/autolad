use std::path::{Path, PathBuf};

use autolad_core::edl::SilenceSettings;
use autolad_core::edl_edit::EdlOp;
use autolad_mcp::base64;
use autolad_mcp::engine::{
    AssetSummary, BuildEdlRequest, CutTextRequest, EditTranscript, EdlSummary, FrameTarget,
    HistoryStep, Occurrence, OpenReport, ProjectStatus, RenderRequest, SubtitleExport,
    TextEditReport, ThumbnailStrip, TranscribeRequest, TranscriptReport,
};
use autolad_mcp::jobs::JobStatus;
use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::error::AppError;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn import_media(
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<AssetSummary, AppError> {
    Ok(state.engine()?.import_media(&path).await?.asset)
}

#[tauri::command]
#[specta::specta]
pub fn project_status(state: State<'_, AppState>) -> Result<ProjectStatus, AppError> {
    Ok(state.engine()?.project_status())
}

/// Replaces the EDL with the speech parts of `asset_id`.
#[tauri::command]
#[specta::specta]
pub async fn auto_cut(
    state: State<'_, AppState>,
    asset_id: String,
    settings: SilenceSettings,
    noise_db: Option<f64>,
) -> Result<EdlSummary, AppError> {
    let request = BuildEdlRequest {
        asset_id,
        settings,
        noise_db,
        min_silence: None,
        append: false,
    };
    Ok(state.engine()?.build_silence_edl(request).await?.edl)
}

#[tauri::command]
#[specta::specta]
pub async fn edit_edl(state: State<'_, AppState>, ops: Vec<EdlOp>) -> Result<EdlSummary, AppError> {
    Ok(state.engine()?.edit_edl(ops).await?)
}

/// Reverts the last change to the timeline, the user's or an agent's.
#[tauri::command]
#[specta::specta]
pub async fn undo(state: State<'_, AppState>) -> Result<HistoryStep, AppError> {
    Ok(state.engine()?.undo().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn redo(state: State<'_, AppState>) -> Result<HistoryStep, AppError> {
    Ok(state.engine()?.redo().await?)
}

/// Frame of the edit at `time`, as a PNG data URL ready for an `<img>`.
#[tauri::command]
#[specta::specta]
pub async fn preview_frame(
    state: State<'_, AppState>,
    time: f64,
    max_width: u32,
) -> Result<String, AppError> {
    let frame = state
        .engine()?
        .preview_frame(FrameTarget::Timeline { time }, max_width)
        .await?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::encode(&frame.png)
    ))
}

/// Playback proxy of an asset. The front plays it through the asset protocol.
#[tauri::command]
#[specta::specta]
pub async fn prepare_proxy(
    state: State<'_, AppState>,
    asset_id: String,
) -> Result<PathBuf, AppError> {
    Ok(state.engine()?.ensure_proxy(&asset_id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn prepare_thumbnails(
    state: State<'_, AppState>,
    asset_id: String,
) -> Result<ThumbnailStrip, AppError> {
    Ok(state.engine()?.ensure_thumbnails(&asset_id).await?)
}

/// One byte per `1 / peaks_per_second` seconds, base64 encoded (a few tens of KB).
#[derive(Debug, Serialize, Type)]
pub struct WaveformPeaks {
    pub peaks_per_second: u32,
    pub base64: String,
}

#[tauri::command]
#[specta::specta]
pub async fn prepare_waveform(
    state: State<'_, AppState>,
    asset_id: String,
) -> Result<WaveformPeaks, AppError> {
    let waveform = state.engine()?.ensure_waveform(&asset_id).await?;
    let peaks = tokio::fs::read(&waveform.path)
        .await
        .map_err(|e| AppError::Internal(format!("{}: {e}", waveform.path.display())))?;
    Ok(WaveformPeaks {
        peaks_per_second: waveform.peaks_per_second,
        base64: base64::encode(&peaks),
    })
}

/// Transcribes an asset (local Whisper). The first use of a model downloads it, so this can
/// take minutes; a transcript already computed comes back at once.
#[tauri::command]
#[specta::specta]
pub async fn transcribe(
    state: State<'_, AppState>,
    asset_id: String,
    language: Option<String>,
    model: Option<String>,
) -> Result<TranscriptReport, AppError> {
    let request = TranscribeRequest {
        asset_id,
        language,
        model,
        word_timestamps: false,
    };
    Ok(state.engine()?.transcribe(request).await?)
}

/// The transcript of an asset if one exists, without starting a transcription.
#[tauri::command]
#[specta::specta]
pub fn cached_transcript(
    state: State<'_, AppState>,
    asset_id: String,
) -> Result<Option<TranscriptReport>, AppError> {
    Ok(state.engine()?.find_transcript(&asset_id))
}

/// What the edit says, sentence by sentence, from the transcripts already computed.
#[tauri::command]
#[specta::specta]
pub fn edit_transcript(state: State<'_, AppState>) -> Result<EditTranscript, AppError> {
    Ok(state.engine()?.edit_transcript())
}

/// Cuts a phrase of `asset_id`'s transcript (said around `start..end` in the source) out of
/// the edit, at word precision.
#[tauri::command]
#[specta::specta]
pub async fn cut_phrase(
    state: State<'_, AppState>,
    asset_id: String,
    start: f64,
    end: f64,
    text: String,
) -> Result<TextEditReport, AppError> {
    let request = CutTextRequest {
        text,
        occurrence: Occurrence::Near {
            asset_id,
            start,
            end,
        },
    };
    Ok(state.engine()?.cut_text(request).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn remove_fillers(state: State<'_, AppState>) -> Result<TextEditReport, AppError> {
    Ok(state.engine()?.remove_fillers(false).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn remove_retakes(state: State<'_, AppState>) -> Result<TextEditReport, AppError> {
    Ok(state.engine()?.remove_retakes(false).await?)
}

/// Writes the project to `path` and keeps that file up to date from now on.
#[tauri::command]
#[specta::specta]
pub async fn save_project(state: State<'_, AppState>, path: PathBuf) -> Result<PathBuf, AppError> {
    Ok(state.engine()?.save_project(&path).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn open_project(
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<OpenReport, AppError> {
    Ok(state.engine()?.open_project(&path).await?)
}

#[tauri::command]
#[specta::specta]
pub fn new_project(state: State<'_, AppState>) -> Result<(), AppError> {
    state.engine()?.new_project();
    Ok(())
}

/// Streaming platforms normalize to about -14 LUFS.
const TARGET_LUFS: f64 = -14.0;

/// Renders to `output`, or next to the first source when none is given. Every cut gets short
/// audio fades; `normalize_audio` also levels the loudness to -14 LUFS, and `burn_subtitles`
/// draws the speech as captions.
#[tauri::command]
#[specta::specta]
pub async fn render_start(
    state: State<'_, AppState>,
    draft: bool,
    output: Option<PathBuf>,
    normalize_audio: bool,
    burn_subtitles: bool,
) -> Result<String, AppError> {
    let engine = state.engine()?;
    let status = engine.project_status();
    let first_cut = status
        .edl
        .cuts
        .first()
        .ok_or_else(|| AppError::InvalidInput("the timeline is empty".into()))?;
    let source = status
        .assets
        .iter()
        .find(|asset| asset.id == first_cut.asset)
        .ok_or_else(|| AppError::InvalidInput("the first clip's source is missing".into()))?;
    let request = RenderRequest {
        output: output.unwrap_or_else(|| default_output(&source.path, draft)),
        draft,
        width: None,
        height: None,
        fps: None,
        overwrite: true,
        loudness: normalize_audio.then_some(TARGET_LUFS),
        subtitles: burn_subtitles,
    };
    Ok(engine.render_start(request).await?)
}

/// Writes the subtitles of the edit as `.srt` or `.vtt`.
#[tauri::command]
#[specta::specta]
pub async fn export_subtitles(
    state: State<'_, AppState>,
    output: PathBuf,
) -> Result<SubtitleExport, AppError> {
    Ok(state.engine()?.export_subtitles(&output).await?)
}

/// Writes the edit as Final Cut Pro XML (Final Cut Pro, DaVinci Resolve).
#[tauri::command]
#[specta::specta]
pub async fn export_fcpxml(
    state: State<'_, AppState>,
    output: PathBuf,
) -> Result<PathBuf, AppError> {
    Ok(state.engine()?.export_fcpxml(&output).await?)
}

#[tauri::command]
#[specta::specta]
pub fn render_status(state: State<'_, AppState>, job_id: String) -> Result<JobStatus, AppError> {
    Ok(state.engine()?.render_status(&job_id)?)
}

#[tauri::command]
#[specta::specta]
pub async fn render_cancel(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<JobStatus, AppError> {
    Ok(state.engine()?.render_cancel(&job_id).await?)
}

fn default_output(source: &Path, draft: bool) -> PathBuf {
    let stem = source
        .file_stem()
        .map_or_else(|| "edit".into(), |s| s.to_string_lossy());
    let suffix = if draft { "_autolad_draft" } else { "_autolad" };
    source.with_file_name(format!("{stem}{suffix}.mp4"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_lands_next_to_the_source() {
        let out = default_output(Path::new("C:/videos/take1.mov"), false);
        assert_eq!(out, PathBuf::from("C:/videos/take1_autolad.mp4"));
    }

    #[test]
    fn draft_output_has_its_own_name() {
        let out = default_output(Path::new("take1.mov"), true);
        assert_eq!(out, PathBuf::from("take1_autolad_draft.mp4"));
    }
}
