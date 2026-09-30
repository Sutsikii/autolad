//! All editing behaviour behind the MCP tools. Methods take and return plain
//! serializable types, so they are testable without any MCP machinery.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use autolad_core::edl::{build_silence_cut_edl, SilenceSettings};
use autolad_core::edl_edit::{apply_ops, EdlOp};
use autolad_core::ports::RenderOptions;
use autolad_core::{Asset, AssetId, Edl, TimeRange};
use autolad_media::frame::extract_frame;
use autolad_media::hash::hash_file;
use autolad_media::{Binaries, Encoder, FfmpegAnalyzer, FfmpegRenderer, FfprobeProbe};
use autolad_transcribe::{ModelStore, TranscribeOptions, WhisperModel, WhisperTranscriber};
use serde::Serialize;
use tokio::sync::OnceCell;

use crate::error::EngineError;
use crate::jobs::{JobStatus, Jobs};
use crate::project_file::{AssetEntry, ProjectFile};

const DEFAULT_NOISE_DB: f64 = -30.0;
const DEFAULT_MIN_SILENCE: f64 = 0.3;
const ASSET_ID_LEN: usize = 12;
const DRAFT_WIDTH: u32 = 640;
const DRAFT_MAX_FPS: f64 = 30.0;

#[derive(Default)]
struct State {
    project: ProjectFile,
    /// When set, every change is written back to this file.
    project_path: Option<PathBuf>,
    /// Silence detection results, so repeated tuning of the EDL doesn't re-run ffmpeg.
    silences: HashMap<String, Vec<TimeRange>>,
}

pub struct Engine {
    binaries: Binaries,
    models: ModelStore,
    data_dir: PathBuf,
    encoder: OnceCell<Encoder>,
    whisper: tokio::sync::Mutex<Option<(WhisperModel, WhisperTranscriber)>>,
    state: Mutex<State>,
    jobs: Jobs,
    tmp_counter: AtomicU64,
}

// ---- Outputs ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct AssetSummary {
    pub id: String,
    pub path: PathBuf,
    pub duration: f64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportedAsset {
    #[serde(flatten)]
    pub asset: AssetSummary,
    pub already_imported: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CutSummary {
    pub index: usize,
    pub asset: String,
    pub start: f64,
    pub end: f64,
    pub duration: f64,
    /// Where this cut begins on the edited timeline.
    pub timeline_start: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EdlSummary {
    pub cuts: Vec<CutSummary>,
    pub total_duration: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectStatus {
    pub assets: Vec<AssetSummary>,
    pub edl: EdlSummary,
    pub project_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SilenceReport {
    pub asset: String,
    pub noise_db: f64,
    pub min_silence: f64,
    pub silences: Vec<TimeRange>,
    pub silent_seconds: f64,
    pub speech_seconds: f64,
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    pub edl: EdlSummary,
    pub source_duration: f64,
    pub removed_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptEntry {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptReport {
    pub asset: String,
    pub model: String,
    pub language: String,
    pub word_timestamps: bool,
    pub cached: bool,
    pub segments: Vec<TranscriptEntry>,
    pub full_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenReport {
    pub assets: usize,
    pub cuts: usize,
    /// Source files that no longer exist at their recorded path.
    pub missing_files: Vec<PathBuf>,
}

pub struct Frame {
    pub png: Vec<u8>,
    pub description: String,
}

// ---- Inputs ----------------------------------------------------------------

pub struct BuildEdlRequest {
    pub asset_id: String,
    pub settings: SilenceSettings,
    pub noise_db: Option<f64>,
    pub min_silence: Option<f64>,
    /// Keep existing cuts and add the new ones after them (multi-rush edits).
    pub append: bool,
}

pub struct TranscribeRequest {
    pub asset_id: String,
    pub language: Option<String>,
    pub model: Option<String>,
    pub word_timestamps: bool,
}

pub enum FrameTarget {
    Source { asset_id: String, time: f64 },
    Timeline { time: f64 },
}

pub struct RenderRequest {
    pub output: PathBuf,
    pub draft: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub overwrite: bool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn io_err(path: &Path, e: &std::io::Error) -> EngineError {
    EngineError::Io(format!("{}: {e}", path.display()))
}

impl Engine {
    pub fn new(binaries: Binaries, data_dir: PathBuf) -> Self {
        Self {
            binaries,
            models: ModelStore::new(data_dir.join("models")),
            data_dir,
            encoder: OnceCell::new(),
            whisper: tokio::sync::Mutex::new(None),
            state: Mutex::new(State::default()),
            jobs: Jobs::default(),
            tmp_counter: AtomicU64::new(0),
        }
    }

    /// Finds the ffmpeg sidecar and the app data directory the standard way.
    pub fn discover() -> Result<Self, EngineError> {
        Ok(Self::new(Binaries::discover()?, crate::paths::data_dir()))
    }

    // ---- Assets ------------------------------------------------------------

    pub async fn import_media(&self, path: &Path) -> Result<ImportedAsset, EngineError> {
        let path = std::path::absolute(path).map_err(|e| io_err(path, &e))?;
        if !path.is_file() {
            return Err(EngineError::Invalid(format!(
                "no such file: {}",
                path.display()
            )));
        }

        let info = FfprobeProbe::new(self.binaries.clone())
            .probe_file(&path)
            .await?;
        if !info.has_video || !info.has_audio {
            return Err(EngineError::Invalid(
                "only files with both a video and an audio track are supported for now".into(),
            ));
        }

        let hash = hash_file(&path).await?;
        let id = AssetId(hash[..ASSET_ID_LEN].to_owned());

        if let Some(existing) = self.find_entry(&id.0) {
            return Ok(ImportedAsset {
                asset: summarize_asset(&existing),
                already_imported: true,
            });
        }

        let entry = AssetEntry {
            asset: Asset {
                id,
                path,
                duration: info.duration,
            },
            width: info.width,
            height: info.height,
            fps: info.fps,
        };
        lock(&self.state).project.assets.push(entry.clone());
        self.persist().await?;
        Ok(ImportedAsset {
            asset: summarize_asset(&entry),
            already_imported: false,
        })
    }

    pub fn project_status(&self) -> ProjectStatus {
        let state = lock(&self.state);
        ProjectStatus {
            assets: state.project.assets.iter().map(summarize_asset).collect(),
            edl: summarize_edl(&state.project.edl),
            project_file: state.project_path.clone(),
        }
    }

    fn find_entry(&self, id: &str) -> Option<AssetEntry> {
        lock(&self.state)
            .project
            .assets
            .iter()
            .find(|e| e.asset.id.0 == id)
            .cloned()
    }

    fn entry(&self, id: &str) -> Result<AssetEntry, EngineError> {
        self.find_entry(id)
            .ok_or_else(|| EngineError::UnknownAsset(id.to_owned()))
    }

    // ---- Analysis ----------------------------------------------------------

    pub async fn detect_silences(
        &self,
        asset_id: &str,
        noise_db: Option<f64>,
        min_silence: Option<f64>,
    ) -> Result<SilenceReport, EngineError> {
        let entry = self.entry(asset_id)?;
        let noise_db = noise_db.unwrap_or(DEFAULT_NOISE_DB);
        let min_silence = min_silence.unwrap_or(DEFAULT_MIN_SILENCE);
        if !(-90.0..0.0).contains(&noise_db) {
            return Err(EngineError::Invalid(
                "noise_db must be between -90 and 0 (dBFS, e.g. -30)".into(),
            ));
        }
        if !(min_silence > 0.0 && min_silence <= 60.0) {
            return Err(EngineError::Invalid(
                "min_silence must be in (0, 60] seconds".into(),
            ));
        }

        let key = format!("{asset_id}|{noise_db}|{min_silence}");
        let cached = lock(&self.state).silences.get(&key).cloned();
        let (silences, was_cached) = match cached {
            Some(found) => (found, true),
            None => {
                let mut analyzer = FfmpegAnalyzer::new(self.binaries.clone());
                analyzer.noise_db = noise_db;
                analyzer.min_silence = min_silence;
                let found = analyzer.detect(&entry.asset.path).await?;
                lock(&self.state).silences.insert(key, found.clone());
                (found, false)
            }
        };

        let silent_seconds: f64 = silences.iter().map(TimeRange::duration).sum();
        Ok(SilenceReport {
            asset: asset_id.to_owned(),
            noise_db,
            min_silence,
            silent_seconds,
            speech_seconds: (entry.asset.duration - silent_seconds).max(0.0),
            silences,
            cached: was_cached,
        })
    }

    pub async fn transcribe(
        &self,
        req: TranscribeRequest,
    ) -> Result<TranscriptReport, EngineError> {
        let entry = self.entry(&req.asset_id)?;
        let model = match req.model.as_deref() {
            None => WhisperModel::default(),
            Some(id) => WhisperModel::from_id(id).ok_or_else(|| {
                let known: Vec<&str> = WhisperModel::ALL.iter().map(|m| m.id()).collect();
                EngineError::Invalid(format!("unknown model {id:?}; choose one of {known:?}"))
            })?,
        };
        let language = req.language.unwrap_or_else(|| "auto".to_owned());
        let key = format!(
            "{}:{}:{language}:{}",
            req.asset_id,
            model.id(),
            req.word_timestamps
        );

        let cached = lock(&self.state).project.transcripts.get(&key).cloned();
        let (segments, was_cached) = match cached {
            Some(found) => (found, true),
            None => {
                let base = self.transcriber(model).await?;
                let options = TranscribeOptions {
                    language: language.clone(),
                    threads: None,
                    word_timestamps: req.word_timestamps,
                };
                let found = base
                    .with_options(options)?
                    .transcribe_file(&entry.asset.path)
                    .await?;
                lock(&self.state)
                    .project
                    .transcripts
                    .insert(key, found.clone());
                self.persist().await?;
                (found, false)
            }
        };

        let full_text = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        Ok(TranscriptReport {
            asset: req.asset_id,
            model: model.id().to_owned(),
            language,
            word_timestamps: req.word_timestamps,
            cached: was_cached,
            segments: segments
                .into_iter()
                .map(|s| TranscriptEntry {
                    start: s.range.start,
                    end: s.range.end,
                    text: s.text,
                })
                .collect(),
            full_text,
        })
    }

    /// Loads the model once and reuses it: loading takes a while and GPU memory.
    async fn transcriber(&self, model: WhisperModel) -> Result<WhisperTranscriber, EngineError> {
        let mut slot = self.whisper.lock().await;
        if let Some((loaded, transcriber)) = slot.as_ref() {
            if *loaded == model {
                return Ok(transcriber.clone());
            }
        }
        let path = self.models.ensure(model, &|_, _| {}).await?;
        let transcriber =
            WhisperTranscriber::load(self.binaries.clone(), path, TranscribeOptions::default())
                .await?;
        *slot = Some((model, transcriber.clone()));
        Ok(transcriber)
    }

    // ---- EDL ---------------------------------------------------------------

    pub async fn build_silence_edl(
        &self,
        req: BuildEdlRequest,
    ) -> Result<BuildReport, EngineError> {
        let entry = self.entry(&req.asset_id)?;
        let report = self
            .detect_silences(&req.asset_id, req.noise_db, req.min_silence)
            .await?;
        let built = build_silence_cut_edl(&entry.asset, &report.silences, &req.settings)?;

        let summary = {
            let mut state = lock(&self.state);
            if req.append {
                state.project.edl.cuts.extend(built.cuts);
            } else {
                state.project.edl = built;
            }
            summarize_edl(&state.project.edl)
        };
        self.persist().await?;
        Ok(BuildReport {
            edl: summary,
            source_duration: entry.asset.duration,
            removed_seconds: (entry.asset.duration - report.speech_seconds).max(0.0),
        })
    }

    pub fn edl_summary(&self) -> EdlSummary {
        summarize_edl(&lock(&self.state).project.edl)
    }

    pub async fn edit_edl(&self, ops: Vec<EdlOp>) -> Result<EdlSummary, EngineError> {
        let summary = {
            let mut state = lock(&self.state);
            let assets: Vec<Asset> = state
                .project
                .assets
                .iter()
                .map(|e| e.asset.clone())
                .collect();
            let edited = apply_ops(&state.project.edl, &ops, &assets)?;
            state.project.edl = edited;
            summarize_edl(&state.project.edl)
        };
        self.persist().await?;
        Ok(summary)
    }

    // ---- Preview -----------------------------------------------------------

    pub async fn preview_frame(
        &self,
        target: FrameTarget,
        max_width: u32,
    ) -> Result<Frame, EngineError> {
        let (entry, source_time, description) = match target {
            FrameTarget::Source { asset_id, time } => {
                let entry = self.entry(&asset_id)?;
                if !time.is_finite() || time < 0.0 || time > entry.asset.duration {
                    return Err(EngineError::Invalid(format!(
                        "time must be within 0..{:.2} for this asset",
                        entry.asset.duration
                    )));
                }
                let desc = format!("asset {asset_id} at source time {time:.2}s");
                (entry, time, desc)
            }
            FrameTarget::Timeline { time } => {
                let (cut, entry, total) = {
                    let state = lock(&self.state);
                    let edl = &state.project.edl;
                    let located = edl.locate(time).ok_or_else(|| {
                        EngineError::Invalid(format!(
                            "timeline time {time} is outside the edit (0..{:.2}s)",
                            edl.total_duration()
                        ))
                    })?;
                    let cut = edl.cuts[located.0].clone();
                    let entry = state
                        .project
                        .assets
                        .iter()
                        .find(|e| e.asset.id == cut.asset)
                        .cloned()
                        .ok_or_else(|| EngineError::UnknownAsset(cut.asset.0.clone()))?;
                    ((located.0, located.1), entry, edl.total_duration())
                };
                let desc = format!(
                    "timeline {time:.2}s of {total:.2}s = cut #{} (asset {}) at source time {:.2}s",
                    cut.0, entry.asset.id.0, cut.1
                );
                (entry, cut.1, desc)
            }
        };

        let dir = self.data_dir.join("tmp");
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| io_err(&dir, &e))?;
        let n = self.tmp_counter.fetch_add(1, Ordering::Relaxed);
        let png_path = dir.join(format!("frame-{}-{n}.png", std::process::id()));

        let max_width = max_width.clamp(64, 1920);
        let result = extract_frame(
            &self.binaries,
            &entry.asset.path,
            source_time,
            &png_path,
            max_width,
        )
        .await;
        let png = match result {
            Ok(()) => tokio::fs::read(&png_path)
                .await
                .map_err(|e| io_err(&png_path, &e)),
            Err(e) => Err(e.into()),
        };
        // Best effort: scratch file, must not mask the result.
        let _ = tokio::fs::remove_file(&png_path).await;
        Ok(Frame {
            png: png?,
            description,
        })
    }

    // ---- Project files -----------------------------------------------------

    /// Writes the project to `path` and keeps it in sync from now on.
    pub async fn save_project(&self, path: &Path) -> Result<PathBuf, EngineError> {
        let path = std::path::absolute(path).map_err(|e| io_err(path, &e))?;
        lock(&self.state).project_path = Some(path.clone());
        self.persist().await?;
        Ok(path)
    }

    pub async fn open_project(&self, path: &Path) -> Result<OpenReport, EngineError> {
        let path = std::path::absolute(path).map_err(|e| io_err(path, &e))?;
        let json = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| io_err(&path, &e))?;
        let project = ProjectFile::from_json(&json)?;

        let missing_files = project
            .assets
            .iter()
            .filter(|e| !e.asset.path.is_file())
            .map(|e| e.asset.path.clone())
            .collect();
        let report = OpenReport {
            assets: project.assets.len(),
            cuts: project.edl.cuts.len(),
            missing_files,
        };

        let mut state = lock(&self.state);
        *state = State {
            project,
            project_path: Some(path),
            silences: HashMap::new(),
        };
        Ok(report)
    }

    /// Writes the project file atomically (temp file + rename) when one is bound.
    async fn persist(&self) -> Result<(), EngineError> {
        let (json, path) = {
            let state = lock(&self.state);
            match &state.project_path {
                Some(path) => (state.project.to_json()?, path.clone()),
                None => return Ok(()),
            }
        };
        let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
        tmp_name.push(".tmp");
        let tmp = path.with_file_name(tmp_name);
        tokio::fs::write(&tmp, json)
            .await
            .map_err(|e| io_err(&tmp, &e))?;
        tokio::fs::rename(&tmp, &path)
            .await
            .map_err(|e| io_err(&path, &e))
    }

    // ---- Rendering ---------------------------------------------------------

    pub async fn render_start(&self, req: RenderRequest) -> Result<String, EngineError> {
        let (edl, entries) = {
            let state = lock(&self.state);
            (state.project.edl.clone(), state.project.assets.clone())
        };
        if edl.cuts.is_empty() {
            return Err(EngineError::Invalid(
                "the EDL is empty: build or edit it before rendering".into(),
            ));
        }
        let output = std::path::absolute(&req.output).map_err(|e| io_err(&req.output, &e))?;
        validate_output(&output, &entries, req.overwrite)?;

        // Cuts are conformed to the first cut's asset by default.
        let first = entries
            .iter()
            .find(|e| e.asset.id == edl.cuts[0].asset)
            .ok_or_else(|| EngineError::UnknownAsset(edl.cuts[0].asset.0.clone()))?;
        let options = resolve_options(first, &req)?;

        let encoder = *self
            .encoder
            .get_or_init(|| Encoder::detect(&self.binaries))
            .await;
        let renderer = FfmpegRenderer::new(self.binaries.clone(), encoder);
        let assets: Vec<Asset> = entries.into_iter().map(|e| e.asset).collect();
        let out = output.clone();

        Ok(self.jobs.spawn(output, move |progress| async move {
            renderer
                .render_edl(&assets, &edl, &options, &out, &*progress)
                .await
                .map_err(|e| e.to_string())
        }))
    }

    pub fn render_status(&self, job_id: &str) -> Result<JobStatus, EngineError> {
        self.jobs.status(job_id)
    }

    pub async fn render_cancel(&self, job_id: &str) -> Result<JobStatus, EngineError> {
        self.jobs.cancel(job_id).await
    }
}

// ---- Pure helpers ----------------------------------------------------------

fn summarize_asset(e: &AssetEntry) -> AssetSummary {
    AssetSummary {
        id: e.asset.id.0.clone(),
        path: e.asset.path.clone(),
        duration: e.asset.duration,
        width: e.width,
        height: e.height,
        fps: e.fps,
    }
}

pub fn summarize_edl(edl: &Edl) -> EdlSummary {
    let mut timeline_start = 0.0;
    let cuts = edl
        .cuts
        .iter()
        .enumerate()
        .map(|(index, cut)| {
            let duration = cut.range.duration();
            let summary = CutSummary {
                index,
                asset: cut.asset.0.clone(),
                start: cut.range.start,
                end: cut.range.end,
                duration,
                timeline_start,
            };
            timeline_start += duration;
            summary
        })
        .collect();
    EdlSummary {
        cuts,
        total_duration: edl.total_duration(),
    }
}

/// Refuses outputs that would destroy a source file or silently overwrite a file.
fn validate_output(
    output: &Path,
    entries: &[AssetEntry],
    overwrite: bool,
) -> Result<(), EngineError> {
    if !output
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mp4"))
    {
        return Err(EngineError::Invalid("output must be an .mp4 file".into()));
    }
    if entries.iter().any(|e| e.asset.path == output) {
        return Err(EngineError::Invalid(
            "output would overwrite a source file; choose another path".into(),
        ));
    }
    if !output.parent().is_some_and(Path::is_dir) {
        return Err(EngineError::Invalid(format!(
            "output folder does not exist: {}",
            output.parent().unwrap_or(output).display()
        )));
    }
    if output.exists() && !overwrite {
        return Err(EngineError::Invalid(format!(
            "{} already exists; pass overwrite=true to replace it",
            output.display()
        )));
    }
    Ok(())
}

/// Output size and frame rate: explicit request, else the first asset's, else 1080p30.
/// Drafts shrink to 640 px wide and cap the frame rate for fast previews.
fn resolve_options(first: &AssetEntry, req: &RenderRequest) -> Result<RenderOptions, EngineError> {
    let mut width = req.width.or(first.width).unwrap_or(1920);
    let mut height = req.height.or(first.height).unwrap_or(1080);
    let mut fps = req.fps.or(first.fps).unwrap_or(30.0);

    if req.draft {
        if req.width.is_none() && req.height.is_none() && width > DRAFT_WIDTH {
            height = (u64::from(height) * u64::from(DRAFT_WIDTH) / u64::from(width.max(1))) as u32;
            width = DRAFT_WIDTH;
        }
        fps = fps.min(DRAFT_MAX_FPS);
    }

    // yuv420p needs even dimensions.
    let even = |v: u32| (v & !1).max(2);
    let options = RenderOptions {
        width: even(width),
        height: even(height),
        fps,
    };
    if !options.fps.is_finite() || options.fps <= 0.0 || options.fps > 240.0 {
        return Err(EngineError::Invalid("fps must be in (0, 240]".into()));
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use autolad_core::{Cut, TimeRange};

    use super::*;

    fn entry(w: Option<u32>, h: Option<u32>, fps: Option<f64>) -> AssetEntry {
        AssetEntry {
            asset: Asset {
                id: AssetId("a".into()),
                path: PathBuf::from("C:/rushes/a.mp4"),
                duration: 10.0,
            },
            width: w,
            height: h,
            fps,
        }
    }

    fn req(draft: bool) -> RenderRequest {
        RenderRequest {
            output: PathBuf::from("out.mp4"),
            draft,
            width: None,
            height: None,
            fps: None,
            overwrite: false,
        }
    }

    #[test]
    fn summary_computes_timeline_offsets() {
        let cut = |s, e| Cut {
            asset: AssetId("a".into()),
            range: TimeRange::new(s, e).unwrap(),
        };
        let edl = Edl {
            cuts: vec![cut(1.0, 3.0), cut(10.0, 13.0)],
        };
        let summary = summarize_edl(&edl);
        assert_eq!(summary.cuts[0].timeline_start, 0.0);
        assert_eq!(summary.cuts[1].timeline_start, 2.0);
        assert_eq!(summary.cuts[1].index, 1);
        assert_eq!(summary.total_duration, 5.0);
    }

    #[test]
    fn default_options_follow_the_first_asset() {
        let o = resolve_options(&entry(Some(1920), Some(1080), Some(29.97)), &req(false)).unwrap();
        assert_eq!((o.width, o.height), (1920, 1080));
        assert_eq!(o.fps, 29.97);
    }

    #[test]
    fn unknown_source_format_falls_back_to_1080p30() {
        let o = resolve_options(&entry(None, None, None), &req(false)).unwrap();
        assert_eq!((o.width, o.height, o.fps), (1920, 1080, 30.0));
    }

    #[test]
    fn draft_shrinks_to_640_wide_with_even_dimensions_and_capped_fps() {
        let o = resolve_options(&entry(Some(1920), Some(1080), Some(60.0)), &req(true)).unwrap();
        assert_eq!((o.width, o.height), (640, 360));
        assert_eq!(o.fps, 30.0);

        // 1000x667 -> 640x426.9 -> must be made even.
        let o = resolve_options(&entry(Some(1000), Some(667), Some(25.0)), &req(true)).unwrap();
        assert_eq!(o.width % 2, 0);
        assert_eq!(o.height % 2, 0);
    }

    #[test]
    fn explicit_size_beats_draft_defaults_and_odd_sizes_are_evened() {
        let request = RenderRequest {
            width: Some(801),
            height: Some(451),
            ..req(true)
        };
        let o = resolve_options(&entry(Some(1920), Some(1080), None), &request).unwrap();
        assert_eq!((o.width, o.height), (800, 450));
    }

    #[test]
    fn absurd_fps_is_rejected() {
        let request = RenderRequest {
            fps: Some(1000.0),
            ..req(false)
        };
        assert!(resolve_options(&entry(None, None, None), &request).is_err());
    }

    #[test]
    fn output_validation() {
        let dir = std::env::temp_dir();
        let entries = vec![entry(None, None, None)];
        let ok = dir.join("autolad-validate-new.mp4");
        assert!(validate_output(&ok, &entries, false).is_ok());

        assert!(validate_output(&dir.join("x.mov"), &entries, false).is_err());
        assert!(validate_output(&dir.join("missing-folder/x.mp4"), &entries, false).is_err());
        assert!(validate_output(Path::new("C:/rushes/a.mp4"), &entries, true).is_err());

        let existing = dir.join(format!("autolad-validate-{}.mp4", std::process::id()));
        std::fs::write(&existing, b"x").unwrap();
        assert!(validate_output(&existing, &entries, false).is_err());
        assert!(validate_output(&existing, &entries, true).is_ok());
    }
}
