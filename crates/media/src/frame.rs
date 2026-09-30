//! Still-frame extraction, used to let an agent (or the UI) look at the footage.

use std::path::Path;

use crate::binaries::Binaries;
use crate::error::MediaError;
use crate::process::{capture, command};

/// Writes one PNG of the frame at `time` seconds, downscaled to at most `max_width`.
pub async fn extract_frame(
    binaries: &Binaries,
    source: &Path,
    time: f64,
    output: &Path,
    max_width: u32,
) -> Result<(), MediaError> {
    let scale = format!("scale='min({max_width},iw)':-2");
    let mut cmd = command(&binaries.ffmpeg);
    // -ss before -i seeks fast (keyframe jump, then exact decode to the timestamp).
    cmd.args(["-hide_banner", "-nostdin", "-y", "-ss"])
        .arg(format!("{time:.3}"))
        .arg("-i")
        .arg(source)
        .args(["-frames:v", "1", "-vf", &scale])
        .arg(output);
    capture("ffmpeg", cmd).await?;
    Ok(())
}
