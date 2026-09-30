//! Catalogue of downloadable whisper.cpp models (ggml, quantized) and their local store.
//! Hashes come from the Hugging Face LFS metadata of `ggerganov/whisper.cpp`.

use std::path::{Path, PathBuf};

use crate::download::download_verified;
use crate::error::TranscribeError;

const BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhisperModel {
    /// ~32 MB. Only good enough for tests and quick drafts.
    Tiny,
    /// ~60 MB. Weak on French and noisy audio.
    Base,
    /// ~190 MB. Best default trade-off between size, speed and accuracy.
    #[default]
    Small,
    /// ~574 MB. Near large-v3 quality at a fraction of the cost; best on a GPU.
    LargeV3Turbo,
}

impl WhisperModel {
    pub const ALL: [WhisperModel; 4] = [
        WhisperModel::Tiny,
        WhisperModel::Base,
        WhisperModel::Small,
        WhisperModel::LargeV3Turbo,
    ];

    pub fn id(self) -> &'static str {
        match self {
            WhisperModel::Tiny => "tiny",
            WhisperModel::Base => "base",
            WhisperModel::Small => "small",
            WhisperModel::LargeV3Turbo => "large-v3-turbo",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }

    pub fn file_name(self) -> &'static str {
        match self {
            WhisperModel::Tiny => "ggml-tiny-q5_1.bin",
            WhisperModel::Base => "ggml-base-q5_1.bin",
            WhisperModel::Small => "ggml-small-q5_1.bin",
            WhisperModel::LargeV3Turbo => "ggml-large-v3-turbo-q5_0.bin",
        }
    }

    pub fn url(self) -> String {
        format!("{BASE_URL}/{}", self.file_name())
    }

    pub fn sha256(self) -> &'static str {
        match self {
            WhisperModel::Tiny => {
                "818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7"
            }
            WhisperModel::Base => {
                "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898"
            }
            WhisperModel::Small => {
                "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb"
            }
            WhisperModel::LargeV3Turbo => {
                "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2"
            }
        }
    }

    pub fn size_bytes(self) -> u64 {
        match self {
            WhisperModel::Tiny => 32_152_673,
            WhisperModel::Base => 59_707_625,
            WhisperModel::Small => 190_085_487,
            WhisperModel::LargeV3Turbo => 574_041_195,
        }
    }
}

/// Directory holding downloaded models (the app's data dir, never the install dir).
#[derive(Debug, Clone)]
pub struct ModelStore {
    dir: PathBuf,
}

impl ModelStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_of(&self, model: WhisperModel) -> PathBuf {
        self.dir.join(model.file_name())
    }

    /// A cheap check: the hash is verified once, at download time, and files are
    /// only ever moved into place after that, so a matching size is enough here.
    pub fn is_installed(&self, model: WhisperModel) -> bool {
        std::fs::metadata(self.path_of(model)).is_ok_and(|m| m.len() == model.size_bytes())
    }

    /// Returns the model path, downloading it first if needed.
    /// `progress` receives `(bytes_done, bytes_total)`.
    pub async fn ensure(
        &self,
        model: WhisperModel,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<PathBuf, TranscribeError> {
        let path = self.path_of(model);
        if !self.is_installed(model) {
            download_verified(&model.url(), &path, model.sha256(), progress).await?;
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_is_small() {
        assert_eq!(WhisperModel::default(), WhisperModel::Small);
    }

    #[test]
    fn ids_round_trip() {
        for m in WhisperModel::ALL {
            assert_eq!(WhisperModel::from_id(m.id()), Some(m));
        }
        assert_eq!(WhisperModel::from_id("huge"), None);
    }

    #[test]
    fn catalogue_entries_are_well_formed() {
        for m in WhisperModel::ALL {
            assert_eq!(m.sha256().len(), 64, "{m:?}");
            assert!(m.sha256().chars().all(|c| c.is_ascii_hexdigit()), "{m:?}");
            assert!(m.url().starts_with("https://") && m.url().ends_with(m.file_name()));
            assert!(m.size_bytes() > 1_000_000);
        }
    }

    #[test]
    fn missing_or_truncated_file_is_not_installed() {
        let dir = std::env::temp_dir().join(format!("autolad-models-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = ModelStore::new(&dir);
        assert!(!store.is_installed(WhisperModel::Tiny));
        std::fs::write(store.path_of(WhisperModel::Tiny), b"partial").unwrap();
        assert!(!store.is_installed(WhisperModel::Tiny));
    }
}
