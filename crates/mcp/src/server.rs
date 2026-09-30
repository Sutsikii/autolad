//! rmcp adapter: parses tool arguments, calls the [`Engine`], maps errors.
//! No editing logic lives here.

use std::path::PathBuf;
use std::sync::Arc;

use autolad_core::edl::SilenceSettings;
use autolad_core::edl_edit::EdlOp;
use autolad_core::AssetId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::base64;
use crate::engine::{BuildEdlRequest, Engine, FrameTarget, RenderRequest, TranscribeRequest};
use crate::error::EngineError;

/// Adapter between MCP and the engine. Cheap to clone.
#[derive(Clone)]
pub struct AutoladServer {
    engine: Arc<Engine>,
}

impl AutoladServer {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine: Arc::new(engine),
        }
    }
}

// ---- Tool arguments (their doc comments become the JSON schema descriptions) ----

#[derive(Deserialize, JsonSchema)]
struct ImportArgs {
    /// Absolute path of a video file that has both a video and an audio track.
    path: String,
}

#[derive(Deserialize, JsonSchema)]
struct DetectSilencesArgs {
    /// Asset id returned by import_media.
    asset_id: String,
    /// Level below which audio counts as silence, in dBFS (-90..0). Default -30; use a
    /// higher value like -25 for noisy rooms.
    noise_db: Option<f64>,
    /// Shortest silence to report, in seconds. Default 0.3.
    min_silence: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
struct TranscribeArgs {
    /// Asset id returned by import_media.
    asset_id: String,
    /// ISO 639-1 language code such as "fr" or "en"; omit to auto-detect.
    language: Option<String>,
    /// One of "tiny", "base", "small" (default), "large-v3-turbo". The first use of a
    /// model downloads it, which can take minutes.
    model: Option<String>,
    /// Return one segment per word (precise timings, but words can occasionally be
    /// dropped) instead of one per phrase. Default false.
    word_timestamps: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct BuildSilenceEdlArgs {
    /// Asset id returned by import_media.
    asset_id: String,
    /// Pauses shorter than this (seconds) between kept parts are bridged. Default 0.3.
    max_gap: Option<f64>,
    /// Padding kept around each part so words aren't clipped (seconds). Default 0.1.
    margin: Option<f64>,
    /// Kept parts shorter than this are dropped (seconds). Default 0.2.
    min_segment: Option<f64>,
    /// Silence threshold in dBFS. Default -30.
    noise_db: Option<f64>,
    /// Shortest silence to remove, in seconds. Default 0.3.
    min_silence: Option<f64>,
    /// Add the new cuts after the existing ones instead of replacing the whole EDL.
    append: Option<bool>,
}

/// One EDL edit. Indices refer to the EDL as it is when this op runs.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
enum EdlOpInput {
    /// Remove the cut at `index`.
    Delete { index: usize },
    /// Set the source range of a cut, in absolute source seconds.
    Trim { index: usize, start: f64, end: f64 },
    /// Split a cut in two at source time `at`, which must lie strictly inside it.
    Split { index: usize, at: f64 },
    /// Move the cut at `from` so it ends up at position `to`.
    Move { from: usize, to: usize },
    /// Insert a new cut before position `index` (index == number of cuts appends).
    Insert {
        index: usize,
        asset: String,
        start: f64,
        end: f64,
    },
    /// Remove every cut.
    Clear,
}

impl From<EdlOpInput> for EdlOp {
    fn from(op: EdlOpInput) -> Self {
        match op {
            EdlOpInput::Delete { index } => EdlOp::Delete { index },
            EdlOpInput::Trim { index, start, end } => EdlOp::Trim { index, start, end },
            EdlOpInput::Split { index, at } => EdlOp::Split { index, at },
            EdlOpInput::Move { from, to } => EdlOp::Move { from, to },
            EdlOpInput::Insert {
                index,
                asset,
                start,
                end,
            } => EdlOp::Insert {
                index,
                asset: AssetId(asset),
                start,
                end,
            },
            EdlOpInput::Clear => EdlOp::Clear,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct EditEdlArgs {
    /// Edits applied in order. If any is invalid, none is applied.
    ops: Vec<EdlOpInput>,
}

#[derive(Deserialize, JsonSchema)]
struct PreviewFrameArgs {
    /// Look at a moment of the EDIT: seconds on the edited timeline. Give this OR
    /// asset_id + source_time.
    timeline_time: Option<f64>,
    /// Look at a moment of a source file: its asset id.
    asset_id: Option<String>,
    /// With asset_id: seconds in the source file.
    source_time: Option<f64>,
    /// Maximum image width in pixels (64..1920). Default 640.
    max_width: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct PathArgs {
    /// File path of the project (.json).
    path: String,
}

#[derive(Deserialize, JsonSchema)]
struct RenderStartArgs {
    /// Output file path; must end in .mp4 and its folder must exist.
    output: String,
    /// Fast low-resolution preview render (640 px wide, at most 30 fps). Default false.
    draft: Option<bool>,
    /// Output width in pixels. Defaults to the first cut's source.
    width: Option<u32>,
    /// Output height in pixels. Defaults to the first cut's source.
    height: Option<u32>,
    /// Output frame rate. Defaults to the first cut's source.
    fps: Option<f64>,
    /// Replace the output if it already exists. Default false.
    overwrite: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct JobArgs {
    /// Job id returned by render_start.
    job_id: String,
}

// ---- Result helpers ----

/// Tool-level errors are returned as results (not protocol errors) so the agent sees the message.
fn respond<T: Serialize>(result: Result<T, EngineError>) -> Result<CallToolResult, ErrorData> {
    match result {
        Ok(value) => serde_json::to_value(&value)
            .map(CallToolResult::structured)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None)),
        Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(
            e.to_string(),
        )])),
    }
}

// ---- Tools ----

#[tool_router]
impl AutoladServer {
    #[tool(
        description = "Import a video file as an asset and get its id, duration and format. Importing the same file twice returns the same id."
    )]
    async fn import_media(
        &self,
        Parameters(args): Parameters<ImportArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.engine.import_media(&PathBuf::from(args.path)).await)
    }

    #[tool(
        description = "Show the current project: imported assets, the EDL (cuts with their timeline positions) and the bound project file."
    )]
    async fn project_status(&self) -> Result<CallToolResult, ErrorData> {
        respond(Ok::<_, EngineError>(self.engine.project_status()))
    }

    #[tool(
        description = "Find silent stretches in an asset's audio. Read-only: it does not change the EDL."
    )]
    async fn detect_silences(
        &self,
        Parameters(a): Parameters<DetectSilencesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(
            self.engine
                .detect_silences(&a.asset_id, a.noise_db, a.min_silence)
                .await,
        )
    }

    #[tool(
        description = "Transcribe an asset's speech locally (Whisper, GPU-accelerated). Returns timed segments and the full text. Results are cached."
    )]
    async fn transcribe(
        &self,
        Parameters(a): Parameters<TranscribeArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(
            self.engine
                .transcribe(TranscribeRequest {
                    asset_id: a.asset_id,
                    language: a.language,
                    model: a.model,
                    word_timestamps: a.word_timestamps.unwrap_or(false),
                })
                .await,
        )
    }

    #[tool(
        description = "Automatically cut the silences out of an asset and build the EDL from what remains. Replaces the current EDL unless append is true."
    )]
    async fn build_silence_edl(
        &self,
        Parameters(a): Parameters<BuildSilenceEdlArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let defaults = SilenceSettings::default();
        respond(
            self.engine
                .build_silence_edl(BuildEdlRequest {
                    asset_id: a.asset_id,
                    settings: SilenceSettings {
                        max_gap: a.max_gap.unwrap_or(defaults.max_gap),
                        margin: a.margin.unwrap_or(defaults.margin),
                        min_segment: a.min_segment.unwrap_or(defaults.min_segment),
                    },
                    noise_db: a.noise_db,
                    min_silence: a.min_silence,
                    append: a.append.unwrap_or(false),
                })
                .await,
        )
    }

    #[tool(
        description = "Get the current EDL: the ordered cuts (asset, source start/end, duration, position on the edited timeline) and the total duration."
    )]
    async fn get_edl(&self) -> Result<CallToolResult, ErrorData> {
        respond(Ok::<_, EngineError>(self.engine.edl_summary()))
    }

    #[tool(
        description = "Edit the EDL with a batch of operations (delete, trim, split, move, insert, clear). Atomic: if one operation is invalid nothing changes. Returns the new EDL."
    )]
    async fn edit_edl(
        &self,
        Parameters(a): Parameters<EditEdlArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let ops = a.ops.into_iter().map(EdlOp::from).collect();
        respond(self.engine.edit_edl(ops).await)
    }

    #[tool(
        description = "See a single frame as an image: either a moment of the edit (timeline_time) or of a source file (asset_id + source_time). Use it to check cuts visually."
    )]
    async fn preview_frame(
        &self,
        Parameters(a): Parameters<PreviewFrameArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let target = match (a.timeline_time, a.asset_id, a.source_time) {
            (Some(time), None, None) => FrameTarget::Timeline { time },
            (None, Some(asset_id), Some(time)) => FrameTarget::Source { asset_id, time },
            _ => {
                let msg = "give either timeline_time, or both asset_id and source_time";
                return Ok(CallToolResult::error(vec![ContentBlock::text(msg)]));
            }
        };
        match self
            .engine
            .preview_frame(target, a.max_width.unwrap_or(640))
            .await
        {
            Ok(frame) => Ok(CallToolResult::success(vec![
                ContentBlock::text(frame.description),
                ContentBlock::image(base64::encode(&frame.png), "image/png"),
            ])),
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(
                e.to_string(),
            )])),
        }
    }

    #[tool(description = "Save the project to a JSON file and keep it updated after every change.")]
    async fn save_project(
        &self,
        Parameters(a): Parameters<PathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(
            self.engine
                .save_project(&PathBuf::from(a.path))
                .await
                .map(|path| serde_json::json!({ "saved_to": path })),
        )
    }

    #[tool(
        description = "Open a project file, replacing the current project. Reports source files that are missing."
    )]
    async fn open_project(
        &self,
        Parameters(a): Parameters<PathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.engine.open_project(&PathBuf::from(a.path)).await)
    }

    #[tool(
        description = "Start rendering the EDL to an .mp4 in the background and return a job id. Poll render_status; it may take a while for long edits."
    )]
    async fn render_start(
        &self,
        Parameters(a): Parameters<RenderStartArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(
            self.engine
                .render_start(RenderRequest {
                    output: PathBuf::from(a.output),
                    draft: a.draft.unwrap_or(false),
                    width: a.width,
                    height: a.height,
                    fps: a.fps,
                    overwrite: a.overwrite.unwrap_or(false),
                })
                .await
                .map(|job_id| serde_json::json!({ "job_id": job_id })),
        )
    }

    #[tool(
        description = "Progress of a render job: state (running, done, failed, cancelled), progress 0..1, output path, and the error if it failed."
    )]
    async fn render_status(
        &self,
        Parameters(a): Parameters<JobArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.engine.render_status(&a.job_id))
    }

    #[tool(description = "Cancel a running render and delete its partial output.")]
    async fn render_cancel(
        &self,
        Parameters(a): Parameters<JobArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.engine.render_cancel(&a.job_id).await)
    }
}

#[tool_handler(
    name = "autolad",
    instructions = "\
AutoLad edits videos locally. Typical workflow:
1. import_media for each rush (returns an asset id).
2. detect_silences to inspect pauses, and/or transcribe to read what is said.
3. build_silence_edl to cut the silences out automatically (creates the EDL, the ordered list of kept ranges).
4. Look at the result: get_edl for the cut list, preview_frame to see any moment of the source or of the edit.
5. Refine with edit_edl (delete / trim / split / move / insert cuts; a batch is applied atomically).
6. render_start (use draft=true for a quick low-res check), poll render_status until state is 'done', or render_cancel.
Times are in seconds. Cut indices refer to get_edl. Use save_project to persist your work."
)]
impl ServerHandler for AutoladServer {}

/// Runs the MCP server over stdin/stdout until the client disconnects.
/// stdout carries the protocol, so nothing else may ever be printed to it.
pub async fn serve_stdio(engine: Engine) -> Result<(), EngineError> {
    let service = AutoladServer::new(engine)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| EngineError::Io(format!("MCP handshake failed: {e}")))?;
    service
        .waiting()
        .await
        .map_err(|e| EngineError::Io(format!("MCP server stopped abnormally: {e}")))?;
    Ok(())
}
