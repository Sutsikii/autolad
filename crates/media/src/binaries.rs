use std::env::consts::EXE_SUFFIX;
use std::path::{Path, PathBuf};

use crate::error::MediaError;

/// Target triple in the sidecars' file names (see `scripts/fetch-ffmpeg.*`).
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const SIDECAR_TRIPLE: &str = "aarch64-apple-darwin";
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const SIDECAR_TRIPLE: &str = "x86_64-apple-darwin";
#[cfg(not(target_os = "macos"))]
const SIDECAR_TRIPLE: &str = "x86_64-pc-windows-msvc";

/// Env var overriding where ffmpeg/ffprobe are looked up (dev, tests, custom builds).
pub const DIR_ENV: &str = "AUTOLAD_FFMPEG_DIR";

#[derive(Debug, Clone, PartialEq)]
pub struct Binaries {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Binaries {
    /// Finds the sidecar next to the running executable (where Tauri installs it),
    /// after `AUTOLAD_FFMPEG_DIR`, and finally on `PATH`.
    pub fn discover() -> Result<Self, MediaError> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Some(dir) = std::env::var_os(DIR_ENV) {
            dirs.push(PathBuf::from(dir));
        }
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
        {
            dirs.push(dir);
        }
        if let Some(path) = std::env::var_os("PATH") {
            dirs.extend(std::env::split_paths(&path));
        }
        Self::discover_in(&dirs)
    }

    /// The sidecars as installed in `dir` before bundling (`src-tauri/binaries`): Tauri wants
    /// them suffixed with the target triple.
    pub fn sidecars_in(dir: &Path) -> Self {
        Self {
            ffmpeg: dir.join(format!("ffmpeg-{SIDECAR_TRIPLE}{EXE_SUFFIX}")),
            ffprobe: dir.join(format!("ffprobe-{SIDECAR_TRIPLE}{EXE_SUFFIX}")),
        }
    }

    pub fn discover_in(dirs: &[PathBuf]) -> Result<Self, MediaError> {
        let ffmpeg_name = format!("ffmpeg{EXE_SUFFIX}");
        let ffprobe_name = format!("ffprobe{EXE_SUFFIX}");
        dirs.iter()
            .find(|d| d.join(&ffmpeg_name).is_file() && d.join(&ffprobe_name).is_file())
            .map(|d| Self {
                ffmpeg: d.join(&ffmpeg_name),
                ffprobe: d.join(&ffprobe_name),
            })
            .ok_or_else(|| {
                let looked = dirs
                    .iter()
                    .map(|d| d.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                MediaError::BinariesNotFound(looked)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("autolad-bin-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_first_directory_holding_both_tools() {
        let empty = scratch("empty");
        let full = scratch("full");
        std::fs::write(full.join(format!("ffmpeg{EXE_SUFFIX}")), b"").unwrap();
        std::fs::write(full.join(format!("ffprobe{EXE_SUFFIX}")), b"").unwrap();

        let found = Binaries::discover_in(&[empty, full.clone()]).unwrap();
        assert_eq!(found.ffmpeg, full.join(format!("ffmpeg{EXE_SUFFIX}")));
    }

    #[test]
    fn requires_both_tools() {
        let half = scratch("half");
        std::fs::write(half.join(format!("ffmpeg{EXE_SUFFIX}")), b"").unwrap();
        assert!(matches!(
            Binaries::discover_in(&[half]),
            Err(MediaError::BinariesNotFound(_))
        ));
    }
}
