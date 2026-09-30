use std::path::{Path, PathBuf};

use autolad_core::edl::SilenceSettings;
use autolad_core::edl_edit::EdlOp;
use autolad_mcp::base64;
use autolad_mcp::engine::{
    AssetSummary, BuildEdlRequest, EdlSummary, FrameTarget, ProjectStatus, RenderRequest,
};
use autolad_mcp::jobs::JobStatus;
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

/// Renders next to the first source, so the UI needs no save dialog.
#[tauri::command]
#[specta::specta]
pub async fn render_start(state: State<'_, AppState>, draft: bool) -> Result<String, AppError> {
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
        output: default_output(&source.path, draft),
        draft,
        width: None,
        height: None,
        fps: None,
        overwrite: true,
    };
    Ok(engine.render_start(request).await?)
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
