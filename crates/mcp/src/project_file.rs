//! On-disk project format: versioned JSON, so files written today stay readable.

use std::collections::BTreeMap;

use autolad_core::ports::TranscriptSegment;
use autolad_core::{Asset, Edl};
use serde::{Deserialize, Serialize};

use crate::error::EngineError;

pub const CURRENT_VERSION: u32 = 1;

/// An imported asset plus the probe facts needed later without re-probing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetEntry {
    pub asset: Asset,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub version: u32,
    pub assets: Vec<AssetEntry>,
    pub edl: Edl,
    /// Transcripts are slow to compute, so they are stored; keyed by
    /// `asset:model:language:words` since each variant differs.
    #[serde(default)]
    pub transcripts: BTreeMap<String, Vec<TranscriptSegment>>,
}

impl Default for ProjectFile {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            assets: Vec::new(),
            edl: Edl::default(),
            transcripts: BTreeMap::new(),
        }
    }
}

impl ProjectFile {
    pub fn to_json(&self) -> Result<String, EngineError> {
        serde_json::to_string_pretty(self).map_err(|e| EngineError::Io(e.to_string()))
    }

    pub fn from_json(json: &str) -> Result<Self, EngineError> {
        let file: Self = serde_json::from_str(json)
            .map_err(|e| EngineError::Invalid(format!("not a valid AutoLad project: {e}")))?;
        if file.version > CURRENT_VERSION {
            return Err(EngineError::Invalid(format!(
                "project version {} is newer than this build supports ({CURRENT_VERSION})",
                file.version
            )));
        }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use autolad_core::{AssetId, Cut, TimeRange};

    use super::*;

    fn sample() -> ProjectFile {
        let id = AssetId("abc".into());
        let mut file = ProjectFile::default();
        file.assets.push(AssetEntry {
            asset: Asset {
                id: id.clone(),
                path: PathBuf::from("C:/rushes/a.mp4"),
                duration: 12.5,
                has_audio: true,
            },
            width: Some(1920),
            height: Some(1080),
            fps: Some(29.97),
        });
        file.edl.cuts.push(Cut {
            asset: id,
            range: TimeRange::new(1.0, 4.0).unwrap(),
        });
        file.transcripts.insert(
            "abc:small:fr:false".into(),
            vec![TranscriptSegment {
                range: TimeRange::new(0.5, 2.0).unwrap(),
                text: "Bonjour".into(),
            }],
        );
        file
    }

    #[test]
    fn round_trips_through_json() {
        let file = sample();
        assert_eq!(
            ProjectFile::from_json(&file.to_json().unwrap()).unwrap(),
            file
        );
    }

    #[test]
    fn files_without_transcripts_still_load() {
        let json = r#"{"version":1,"assets":[],"edl":{"cuts":[]}}"#;
        assert!(ProjectFile::from_json(json).unwrap().transcripts.is_empty());
    }

    #[test]
    fn newer_versions_are_rejected_instead_of_misread() {
        let json = r#"{"version":99,"assets":[],"edl":{"cuts":[]}}"#;
        assert!(matches!(
            ProjectFile::from_json(json),
            Err(EngineError::Invalid(_))
        ));
    }

    #[test]
    fn garbage_is_an_invalid_project() {
        assert!(matches!(
            ProjectFile::from_json("{nope"),
            Err(EngineError::Invalid(_))
        ));
    }
}
