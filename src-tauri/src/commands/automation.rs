use autolad_core::edl::{build_silence_cut_edl, SilenceSettings};
use autolad_core::{Asset, Edl, TimeRange};

use crate::error::AppError;

#[tauri::command]
#[specta::specta]
pub fn build_silence_edl(
    asset: Asset,
    silences: Vec<TimeRange>,
    settings: SilenceSettings,
) -> Result<Edl, AppError> {
    Ok(build_silence_cut_edl(&asset, &silences, &settings)?)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use autolad_core::AssetId;

    use super::*;

    fn asset() -> Asset {
        Asset {
            id: AssetId("a".into()),
            path: PathBuf::from("a.mp4"),
            duration: 10.0,
        }
    }

    #[test]
    fn builds_an_edl() {
        let silences = vec![TimeRange {
            start: 4.0,
            end: 6.0,
        }];
        let edl = build_silence_edl(asset(), silences, SilenceSettings::default()).unwrap();
        assert_eq!(edl.cuts.len(), 2);
    }

    #[test]
    fn invalid_settings_become_invalid_input() {
        let settings = SilenceSettings {
            margin: -1.0,
            ..SilenceSettings::default()
        };
        let err = build_silence_edl(asset(), vec![], settings).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }
}
