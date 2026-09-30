//! All editing behaviour behind the MCP tools. Methods take and return plain
//! serializable types, so they are testable without any MCP machinery.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use autolad_core::edl::{build_silence_cut_edl, SilenceSettings};
use autolad_core::edl_edit::{apply_ops, describe_ops, EdlOp};
use autolad_core::history::{History, HistoryStatus};
use autolad_core::ports::{RenderOptions, TranscriptSegment};
use autolad_core::{Asset, AssetId, Edl, TimeRange};
use autolad_media::frame::extract_frame;
use autolad_media::hash::hash_file;
use autolad_media::preview::{
    build_proxy, build_thumbnail_strip, build_waveform, strip_layout, tile_width, PEAKS_PER_SECOND,
    TILE_HEIGHT,
};
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
const DRAFT_LONG_SIDE: u32 = 640;
const DRAFT_MAX_FPS: f64 = 30.0;

#[derive(Default)]
struct State {
    project: ProjectFile,
    /// When set, every change is written back to this file.
    project_path: Option<PathBuf>,
    /// Silence detection results, so repeated tuning of the EDL doesn't re-run ffmpeg.
    silences: HashMap<String, Vec<TimeRange>>,
    /// Undo stack of the EDL, shared by the UI and agents. Lives for the session only.
    history: History<Edl>,
}

impl State {
    /// The single way the EDL changes, so every change can be undone. No-op edits are not
    /// recorded: undoing them would seem to do nothing.
    fn commit(&mut self, edl: Edl, label: impl Into<String>) {
        if self.project.edl == edl {
            return;
        }
        let before = std::mem::replace(&mut self.project.edl, edl);
        self.history.record(before, label);
    }

    fn summary(&self) -> EdlSummary {
        summarize_edl(&self.project.edl, self.history.status())
    }
}

pub struct Engine {
    binaries: Binaries,
    models: ModelStore,
    data_dir: PathBuf,
    encoder: OnceCell<Encoder>,
    whisper: tokio::sync::Mutex<Option<(WhisperModel, WhisperTranscriber)>>,
    /// Proxies are built one at a time: concurrent callers for the same asset must not write
    /// the same file, and a single encode already uses every core.
    proxy_gate: tokio::sync::Mutex<()>,
    state: Mutex<State>,
    jobs: Jobs,
    tmp_counter: AtomicU64,
}

// ---- Outputs ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AssetSummary {
    pub id: String,
    pub path: PathBuf,
    pub duration: f64,
    /// `false` for silent clips: nothing to detect, transcribe or draw as a waveform.
    pub has_audio: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
}

/// Thumbnails of one asset laid out side by side in a single image.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThumbnailStrip {
    pub path: PathBuf,
    /// Seconds between two thumbnails: tile `i` shows the frame at `i * step`.
    pub step: f64,
    pub tiles: u32,
    pub tile_width: u32,
    pub tile_height: u32,
}

/// Loudness of one asset, one byte (`0..=255`) per `1 / peaks_per_second` seconds.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Waveform {
    pub path: PathBuf,
    pub peaks_per_second: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportedAsset {
    #[serde(flatten)]
    pub asset: AssetSummary,
    pub already_imported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
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
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EdlSummary {
    pub cuts: Vec<CutSummary>,
    pub total_duration: f64,
    /// Names of the changes `undo` and `redo` would revert or re-apply.
    pub history: HistoryStatus,
}

/// Result of `undo` / `redo`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HistoryStep {
    /// The change that was reverted (undo) or applied again (redo).
    pub change: String,
    pub edl: EdlSummary,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
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
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BuildReport {
    pub edl: EdlSummary,
    pub source_duration: f64,
    pub removed_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TranscriptEntry {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
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
#[cfg_attr(feature = "specta", derive(specta::Type))]
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

async fn is_file(path: &Path) -> bool {
    tokio::fs::metadata(path).await.is_ok_and(|m| m.is_file())
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
            proxy_gate: tokio::sync::Mutex::new(()),
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
        if !info.has_video {
            return Err(EngineError::Invalid(
                "this file has no video track: only videos can be imported".into(),
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
                has_audio: info.has_audio,
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
            edl: state.summary(),
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

    /// Like `entry`, for operations that read the audio track.
    fn entry_with_audio(&self, id: &str, doing: &str) -> Result<AssetEntry, EngineError> {
        let entry = self.entry(id)?;
        if entry.asset.has_audio {
            Ok(entry)
        } else {
            Err(EngineError::Invalid(format!(
                "asset {id} has no audio track, so there is nothing to {doing}; \
                 add it to the timeline with edit_edl (insert) instead"
            )))
        }
    }

    // ---- Editor media (cached by asset id) ----------------------------------

    /// Playback proxy of an asset, built on first use.
    pub async fn ensure_proxy(&self, asset_id: &str) -> Result<PathBuf, EngineError> {
        let entry = self.entry(asset_id)?;
        let output = self.cache_file("proxies", asset_id, "mp4").await?;
        let _turn = self.proxy_gate.lock().await;
        if !is_file(&output).await {
            build_proxy(&self.binaries, &entry.asset.path, &output).await?;
        }
        Ok(output)
    }

    pub async fn ensure_thumbnails(&self, asset_id: &str) -> Result<ThumbnailStrip, EngineError> {
        let entry = self.entry(asset_id)?;
        let layout = strip_layout(entry.asset.duration);
        let tile = tile_width(entry.width, entry.height);
        // The tile width is part of the name: a strip built for another shape must not be reused.
        let output = self
            .cache_file("thumbs", &format!("{asset_id}-{tile}"), "jpg")
            .await?;
        if !is_file(&output).await {
            let proxy = self.ensure_proxy(asset_id).await?;
            build_thumbnail_strip(&self.binaries, &proxy, &output, layout, tile).await?;
        }
        Ok(ThumbnailStrip {
            path: output,
            step: layout.step,
            tiles: layout.tiles,
            tile_width: tile,
            tile_height: TILE_HEIGHT,
        })
    }

    pub async fn ensure_waveform(&self, asset_id: &str) -> Result<Waveform, EngineError> {
        let entry = self.entry_with_audio(asset_id, "draw a waveform for")?;
        let output = self.cache_file("waves", asset_id, "bin").await?;
        if !is_file(&output).await {
            build_waveform(&self.binaries, &entry.asset.path, &output).await?;
        }
        Ok(Waveform {
            path: output,
            peaks_per_second: PEAKS_PER_SECOND,
        })
    }

    /// `<data>/<kind>/<asset id>.<ext>`, creating the folder.
    async fn cache_file(
        &self,
        kind: &str,
        asset_id: &str,
        ext: &str,
    ) -> Result<PathBuf, EngineError> {
        let dir = self.data_dir.join(kind);
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| io_err(&dir, &e))?;
        Ok(dir.join(format!("{asset_id}.{ext}")))
    }

    // ---- Analysis ----------------------------------------------------------

    pub async fn detect_silences(
        &self,
        asset_id: &str,
        noise_db: Option<f64>,
        min_silence: Option<f64>,
    ) -> Result<SilenceReport, EngineError> {
        let entry = self.entry_with_audio(asset_id, "detect silences in")?;
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
        let entry = self.entry_with_audio(&req.asset_id, "transcribe")?;
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

        Ok(transcript_report(
            req.asset_id,
            TranscriptKey {
                model: model.id().to_owned(),
                language,
                word_timestamps: req.word_timestamps,
            },
            segments,
            was_cached,
        ))
    }

    /// A transcript already computed for `asset_id` (phrase mode preferred), without starting
    /// one: the UI shows it as soon as the clip is selected, and never triggers a long job.
    pub fn find_transcript(&self, asset_id: &str) -> Option<TranscriptReport> {
        let state = lock(&self.state);
        let mut found: Vec<(TranscriptKey, &Vec<TranscriptSegment>)> = state
            .project
            .transcripts
            .iter()
            .filter_map(|(key, segments)| {
                let (id, rest) = key.split_once(':')?;
                (id == asset_id).then_some((TranscriptKey::parse(rest)?, segments))
            })
            .collect();
        found.sort_by_key(|(key, _)| key.word_timestamps);
        let (key, segments) = found.into_iter().next()?;
        Some(transcript_report(
            asset_id.to_owned(),
            key,
            segments.clone(),
            true,
        ))
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
            let edl = if req.append {
                let mut edl = state.project.edl.clone();
                edl.cuts.extend(built.cuts);
                edl
            } else {
                built
            };
            state.commit(edl, "Cut the silences");
            state.summary()
        };
        self.persist().await?;
        Ok(BuildReport {
            edl: summary,
            source_duration: entry.asset.duration,
            removed_seconds: (entry.asset.duration - report.speech_seconds).max(0.0),
        })
    }

    pub fn edl_summary(&self) -> EdlSummary {
        lock(&self.state).summary()
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
            state.commit(edited, describe_ops(&ops));
            state.summary()
        };
        self.persist().await?;
        Ok(summary)
    }

    /// Reverts the last change to the EDL, whoever made it (UI or agent).
    pub async fn undo(&self) -> Result<HistoryStep, EngineError> {
        self.step_history(History::undo, "nothing to undo").await
    }

    /// Applies again the last undone change.
    pub async fn redo(&self) -> Result<HistoryStep, EngineError> {
        self.step_history(History::redo, "nothing to redo").await
    }

    async fn step_history(
        &self,
        step: fn(&mut History<Edl>, &mut Edl) -> Option<String>,
        empty: &str,
    ) -> Result<HistoryStep, EngineError> {
        let result = {
            let mut state = lock(&self.state);
            let State {
                project, history, ..
            } = &mut *state;
            let change = step(history, &mut project.edl)
                .ok_or_else(|| EngineError::Invalid(empty.to_owned()))?;
            HistoryStep {
                change,
                edl: state.summary(),
            }
        };
        self.persist().await?;
        Ok(result)
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

    /// Starts an empty project, not bound to any file.
    pub fn new_project(&self) {
        *lock(&self.state) = State::default();
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
            ..State::default()
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

/// What distinguishes the transcripts of one asset; stored as `model:language:words` after the
/// asset id in the project file.
struct TranscriptKey {
    model: String,
    language: String,
    word_timestamps: bool,
}

impl TranscriptKey {
    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split(':');
        let (model, language, words) = (parts.next()?, parts.next()?, parts.next()?);
        Some(Self {
            model: model.to_owned(),
            language: language.to_owned(),
            word_timestamps: words.parse().ok()?,
        })
    }
}

fn transcript_report(
    asset: String,
    key: TranscriptKey,
    segments: Vec<TranscriptSegment>,
    cached: bool,
) -> TranscriptReport {
    let full_text = segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    TranscriptReport {
        asset,
        model: key.model,
        language: key.language,
        word_timestamps: key.word_timestamps,
        cached,
        segments: segments
            .into_iter()
            .map(|s| TranscriptEntry {
                start: s.range.start,
                end: s.range.end,
                text: s.text,
            })
            .collect(),
        full_text,
    }
}

fn summarize_asset(e: &AssetEntry) -> AssetSummary {
    AssetSummary {
        id: e.asset.id.0.clone(),
        path: e.asset.path.clone(),
        duration: e.asset.duration,
        has_audio: e.asset.has_audio,
        width: e.width,
        height: e.height,
        fps: e.fps,
    }
}

pub fn summarize_edl(edl: &Edl, history: HistoryStatus) -> EdlSummary {
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
        history,
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
/// Drafts shrink so the longer side is at most 640 px (a portrait phone video becomes 360x640,
/// not a huge 640 px wide frame) and cap the frame rate for fast previews.
fn resolve_options(first: &AssetEntry, req: &RenderRequest) -> Result<RenderOptions, EngineError> {
    let mut width = req.width.or(first.width).unwrap_or(1920);
    let mut height = req.height.or(first.height).unwrap_or(1080);
    let mut fps = req.fps.or(first.fps).unwrap_or(30.0);

    if req.draft {
        let longer = width.max(height);
        if req.width.is_none() && req.height.is_none() && longer > DRAFT_LONG_SIDE {
            let shrink = |side: u32| {
                (u64::from(side) * u64::from(DRAFT_LONG_SIDE) / u64::from(longer)) as u32
            };
            (width, height) = (shrink(width), shrink(height));
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
                has_audio: true,
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
        let summary = summarize_edl(&edl, HistoryStatus::default());
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
    fn draft_shrinks_the_longer_side_to_640_with_even_dimensions_and_capped_fps() {
        let o = resolve_options(&entry(Some(1920), Some(1080), Some(60.0)), &req(true)).unwrap();
        assert_eq!((o.width, o.height), (640, 360));
        assert_eq!(o.fps, 30.0);

        // 1000x667 -> 640x426.9 -> must be made even.
        let o = resolve_options(&entry(Some(1000), Some(667), Some(25.0)), &req(true)).unwrap();
        assert_eq!(o.width % 2, 0);
        assert_eq!(o.height % 2, 0);
    }

    fn segment(start: f64, end: f64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            range: TimeRange::new(start, end).unwrap(),
            text: text.to_owned(),
        }
    }

    fn engine_with_transcripts(stored: &[(&str, Vec<TranscriptSegment>)]) -> Engine {
        let dummy = PathBuf::from("unused");
        let engine = Engine::new(
            Binaries {
                ffmpeg: dummy.clone(),
                ffprobe: dummy,
            },
            std::env::temp_dir().join("autolad-unit-transcripts"),
        );
        for (key, segments) in stored {
            lock(&engine.state)
                .project
                .transcripts
                .insert((*key).to_owned(), segments.clone());
        }
        engine
    }

    #[test]
    fn a_stored_transcript_is_found_without_starting_one() {
        let engine = engine_with_transcripts(&[(
            "abc:small:fr:false",
            vec![
                segment(0.0, 2.0, "Bonjour"),
                segment(2.0, 4.5, "tout le monde"),
            ],
        )]);
        let report = engine.find_transcript("abc").unwrap();
        assert_eq!(
            (report.model.as_str(), report.language.as_str()),
            ("small", "fr")
        );
        assert!(report.cached && !report.word_timestamps);
        assert_eq!(report.segments.len(), 2);
        assert_eq!(report.full_text, "Bonjour tout le monde");
    }

    #[test]
    fn phrases_are_preferred_over_words_and_other_assets_are_ignored() {
        let engine = engine_with_transcripts(&[
            ("abc:small:auto:true", vec![segment(0.0, 0.5, "mot")]),
            (
                "abc:small:auto:false",
                vec![segment(0.0, 2.0, "une phrase")],
            ),
            ("abcd:small:auto:false", vec![segment(0.0, 1.0, "autre")]),
        ]);
        let report = engine.find_transcript("abc").unwrap();
        assert_eq!(report.full_text, "une phrase");
        assert!(engine.find_transcript("zzz").is_none());
        // An id that merely starts like another one must not match it.
        assert_eq!(engine.find_transcript("abcd").unwrap().full_text, "autre");
    }

    fn engine_with_asset() -> Engine {
        let engine = engine_with_transcripts(&[]);
        lock(&engine.state)
            .project
            .assets
            .push(entry(Some(640), Some(360), Some(25.0)));
        engine
    }

    fn insert(start: f64, end: f64) -> EdlOp {
        EdlOp::Insert {
            index: 0,
            asset: AssetId("a".into()),
            start,
            end,
        }
    }

    #[tokio::test]
    async fn edits_by_anyone_can_be_undone_and_redone() {
        let engine = engine_with_asset();
        engine.edit_edl(vec![insert(0.0, 4.0)]).await.unwrap();
        let edited = engine
            .edit_edl(vec![EdlOp::Split { index: 0, at: 2.0 }])
            .await
            .unwrap();
        assert_eq!(edited.cuts.len(), 2);
        assert_eq!(edited.history.undo.as_deref(), Some("Split clip 1"));

        let undone = engine.undo().await.unwrap();
        assert_eq!(undone.change, "Split clip 1");
        assert_eq!(undone.edl.cuts.len(), 1);
        assert_eq!(undone.edl.history.redo.as_deref(), Some("Split clip 1"));

        let redone = engine.redo().await.unwrap();
        assert_eq!(redone.edl.cuts.len(), 2);

        engine.undo().await.unwrap();
        engine.undo().await.unwrap();
        assert!(engine.edl_summary().cuts.is_empty());
        assert!(matches!(engine.undo().await, Err(EngineError::Invalid(_))));
    }

    #[tokio::test]
    async fn edits_that_change_nothing_are_not_recorded() {
        let engine = engine_with_asset();
        engine.edit_edl(vec![EdlOp::Clear]).await.unwrap();
        assert_eq!(engine.edl_summary().history, HistoryStatus::default());
    }

    #[tokio::test]
    async fn a_new_project_forgets_the_history() {
        let engine = engine_with_asset();
        engine.edit_edl(vec![insert(0.0, 1.0)]).await.unwrap();
        engine.new_project();
        assert!(engine.undo().await.is_err());
    }

    #[test]
    fn a_portrait_draft_is_tall_not_wide() {
        let o = resolve_options(&entry(Some(1080), Some(1920), Some(30.0)), &req(true)).unwrap();
        assert_eq!((o.width, o.height), (360, 640));
    }

    #[test]
    fn a_draft_never_enlarges_a_small_video() {
        let o = resolve_options(&entry(Some(320), Some(180), Some(24.0)), &req(true)).unwrap();
        assert_eq!((o.width, o.height), (320, 180));
        let o = resolve_options(&entry(Some(180), Some(320), Some(24.0)), &req(true)).unwrap();
        assert_eq!((o.width, o.height), (180, 320));
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
