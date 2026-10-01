use std::path::{Path, PathBuf};
use std::process::Stdio;

use autolad_core::ports::{ProgressFn, RenderOptions, Renderer};
use autolad_core::{Asset, CoreError, Edl};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use crate::binaries::Binaries;
use crate::encoder::{max_bitrate_kbps, Encoder};
use crate::error::MediaError;
use crate::filtergraph::{build_filter_graph, PlacedCut};
use crate::parse::parse_progress_time;
use crate::process::{command, spawn_error, tail};

/// What one ffmpeg run needs besides the encoder, so a retry can reuse it as is.
#[derive(Clone, Copy)]
struct Job<'a> {
    inputs: &'a [&'a Path],
    graph_path: &'a Path,
    output: &'a Path,
    total: f64,
    max_kbps: u32,
    /// ffmpeg runs here: the subtitle file is referred to by name (see `build_filter_graph`).
    workdir: Option<&'a Path>,
}

/// Renders an EDL with ffmpeg. Cancel by dropping the future: the process is killed.
#[derive(Debug, Clone)]
pub struct FfmpegRenderer {
    binaries: Binaries,
    encoder: Encoder,
}

impl FfmpegRenderer {
    pub fn new(binaries: Binaries, encoder: Encoder) -> Self {
        Self { binaries, encoder }
    }

    pub async fn render_edl(
        &self,
        assets: &[Asset],
        edl: &Edl,
        options: &RenderOptions,
        output: &Path,
        progress: ProgressFn<'_>,
    ) -> Result<(), MediaError> {
        validate(options)?;
        let (inputs, audio, cuts) = place_cuts(assets, edl)?;
        let graph = build_filter_graph(&cuts, &audio, options)?;

        // A graph with hundreds of cuts overflows the Windows command line, so it goes in a file.
        let graph_path = graph_file_path(output);
        std::fs::write(&graph_path, graph).map_err(|e| spawn_error("ffmpeg", &e))?;
        // A guard rather than a trailing remove: the future is dropped on cancellation.
        let _cleanup = RemoveOnDrop(graph_path.clone());
        let job = Job {
            inputs: &inputs,
            graph_path: &graph_path,
            output,
            total: edl.total_duration(),
            max_kbps: max_bitrate_kbps(options.width, options.height, options.fps),
            workdir: options.subtitles.as_deref().and_then(Path::parent),
        };
        let encoder = self.encoder.for_size(options.width, options.height);
        match self.run(encoder, &job, progress).await {
            // Hardware encoders can refuse a job for reasons no size rule predicts (driver
            // limits, busy GPU): software encoding always works, only slower.
            Err(MediaError::Failed { .. }) if encoder.is_hardware() => {
                self.run(Encoder::X264, &job, progress).await
            }
            result => result,
        }
    }

    async fn run(
        &self,
        encoder: Encoder,
        job: &Job<'_>,
        progress: ProgressFn<'_>,
    ) -> Result<(), MediaError> {
        let Job {
            inputs,
            graph_path,
            output,
            total,
            max_kbps,
            workdir,
        } = *job;
        let mut cmd = command(&self.binaries.ffmpeg);
        if let Some(dir) = workdir {
            cmd.current_dir(dir);
        }
        cmd.args([
            "-hide_banner",
            "-nostdin",
            "-y",
            "-nostats",
            "-progress",
            "pipe:1",
        ]);
        for input in inputs {
            cmd.arg("-i").arg(input);
        }
        cmd.arg("-/filter_complex")
            .arg(graph_path)
            .args(["-map", "[outv]", "-map", "[outa]"])
            .args(encoder.args(max_kbps))
            .args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k"])
            .args(["-movflags", "+faststart"])
            .arg(output)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| spawn_error("ffmpeg", &e))?;
        let (Some(stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err(spawn_error(
                "ffmpeg",
                &std::io::Error::other("stdio pipes unavailable"),
            ));
        };

        // Drain stderr concurrently, otherwise a full pipe would stall ffmpeg.
        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf).await;
            buf
        });

        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(t) = parse_progress_time(&line) {
                progress((t / total).clamp(0.0, 1.0));
            }
        }

        let status = child.wait().await.map_err(|e| spawn_error("ffmpeg", &e))?;
        let stderr_text = stderr_task.await.unwrap_or_default();
        if status.success() {
            progress(1.0);
            Ok(())
        } else {
            Err(MediaError::Failed {
                tool: "ffmpeg",
                code: status.code(),
                stderr: tail(&stderr_text),
            })
        }
    }
}

impl Renderer for FfmpegRenderer {
    async fn render(
        &self,
        assets: &[Asset],
        edl: &Edl,
        options: &RenderOptions,
        output: &Path,
        progress: ProgressFn<'_>,
    ) -> Result<(), CoreError> {
        Ok(self
            .render_edl(assets, edl, options, output, progress)
            .await?)
    }
}

/// Deletes a temp file when dropped, whatever way the render ends.
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        // Best effort: a leftover temp file is harmless and must not mask the render result.
        let _ = std::fs::remove_file(&self.0);
    }
}

fn validate(o: &RenderOptions) -> Result<(), MediaError> {
    // yuv420p needs even dimensions.
    if o.width == 0 || o.height == 0 || !o.width.is_multiple_of(2) || !o.height.is_multiple_of(2) {
        return Err(MediaError::InvalidOptions(
            "width and height must be even and > 0",
        ));
    }
    if !o.fps.is_finite() || o.fps <= 0.0 || o.fps > 240.0 {
        return Err(MediaError::InvalidOptions("fps must be in (0, 240]"));
    }
    // loudnorm's accepted range for the integrated target.
    if o.loudness.is_some_and(|l| !(-70.0..=-5.0).contains(&l)) {
        return Err(MediaError::InvalidOptions(
            "loudness target must be in [-70, -5] LUFS",
        ));
    }
    Ok(())
}

/// Assigns each distinct asset one `-i` input, in order of first use. Returns those inputs,
/// whether each has audio, and the cuts pointing at them.
#[allow(clippy::type_complexity)]
fn place_cuts<'a>(
    assets: &'a [Asset],
    edl: &Edl,
) -> Result<(Vec<&'a Path>, Vec<bool>, Vec<PlacedCut>), MediaError> {
    let mut order: Vec<&Asset> = Vec::new();
    let mut cuts = Vec::with_capacity(edl.cuts.len());
    for cut in &edl.cuts {
        let asset = assets
            .iter()
            .find(|a| a.id == cut.asset)
            .ok_or_else(|| MediaError::UnknownAsset(cut.asset.0.clone()))?;
        let input = match order.iter().position(|a| a.id == asset.id) {
            Some(i) => i,
            None => {
                order.push(asset);
                order.len() - 1
            }
        };
        cuts.push(PlacedCut {
            input,
            range: cut.range,
        });
    }
    let paths = order.iter().map(|a| a.path.as_path()).collect();
    let audio = order.iter().map(|a| a.has_audio).collect();
    Ok((paths, audio, cuts))
}

fn graph_file_path(output: &Path) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".filtergraph");
    output.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use autolad_core::{AssetId, Cut, TimeRange};

    use super::*;

    fn asset(id: &str) -> Asset {
        Asset {
            id: AssetId(id.into()),
            path: PathBuf::from(format!("{id}.mp4")),
            duration: 10.0,
            has_audio: true,
        }
    }

    fn cut(id: &str, s: f64, e: f64) -> Cut {
        Cut {
            asset: AssetId(id.into()),
            range: TimeRange::new(s, e).unwrap(),
        }
    }

    #[test]
    fn inputs_are_numbered_by_first_use() {
        let assets = [asset("a"), asset("b")];
        let edl = Edl {
            cuts: vec![cut("b", 0.0, 1.0), cut("a", 0.0, 1.0), cut("b", 2.0, 3.0)],
        };
        let (inputs, _, cuts) = place_cuts(&assets, &edl).unwrap();
        assert_eq!(inputs, vec![Path::new("b.mp4"), Path::new("a.mp4")]);
        let idx: Vec<usize> = cuts.iter().map(|c| c.input).collect();
        assert_eq!(idx, vec![0, 1, 0]);
    }

    #[test]
    fn unknown_asset_is_reported() {
        let edl = Edl {
            cuts: vec![cut("ghost", 0.0, 1.0)],
        };
        assert_eq!(
            place_cuts(&[asset("a")], &edl).unwrap_err(),
            MediaError::UnknownAsset("ghost".into())
        );
    }

    #[test]
    fn odd_dimensions_and_bad_fps_are_rejected() {
        let ok = RenderOptions {
            width: 1280,
            height: 720,
            fps: 30.0,
            loudness: Some(-14.0),
            subtitles: None,
        };
        assert!(validate(&ok).is_ok());
        assert!(validate(&RenderOptions {
            loudness: Some(0.0),
            ..ok.clone()
        })
        .is_err());
        assert!(validate(&RenderOptions {
            width: 1281,
            ..ok.clone()
        })
        .is_err());
        assert!(validate(&RenderOptions {
            fps: 0.0,
            ..ok.clone()
        })
        .is_err());
        assert!(validate(&RenderOptions {
            fps: f64::NAN,
            ..ok
        })
        .is_err());
    }

    #[test]
    fn graph_file_sits_next_to_output() {
        assert_eq!(
            graph_file_path(Path::new("out/final.mp4")),
            PathBuf::from("out/final.mp4.filtergraph")
        );
    }
}
