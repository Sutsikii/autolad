//! Builds an [`Edl`] from analysis results and user settings.

use serde::{Deserialize, Serialize};

use crate::domain::{Asset, Cut, Edl, TimeRange};
use crate::error::CoreError;
use crate::segments::{apply_margins, drop_short, merge_close, silences_to_segments};

/// User-facing silence-removal settings, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SilenceSettings {
    /// Pauses shorter than this between kept segments are bridged.
    pub max_gap: f64,
    /// Padding kept around each segment so words aren't clipped.
    pub margin: f64,
    /// Kept segments shorter than this are discarded.
    pub min_segment: f64,
}

impl Default for SilenceSettings {
    fn default() -> Self {
        Self {
            max_gap: 0.3,
            margin: 0.1,
            min_segment: 0.2,
        }
    }
}

impl SilenceSettings {
    fn validate(&self) -> Result<(), CoreError> {
        let ok = |v: f64| v.is_finite() && v >= 0.0;
        if !ok(self.max_gap) {
            return Err(CoreError::InvalidSetting("max_gap"));
        }
        if !ok(self.margin) {
            return Err(CoreError::InvalidSetting("margin"));
        }
        if !ok(self.min_segment) {
            return Err(CoreError::InvalidSetting("min_segment"));
        }
        Ok(())
    }
}

/// Builds the EDL that removes `silences` from a single asset.
pub fn build_silence_cut_edl(
    asset: &Asset,
    silences: &[TimeRange],
    settings: &SilenceSettings,
) -> Result<Edl, CoreError> {
    settings.validate()?;

    let speech = silences_to_segments(silences, asset.duration);
    let speech = merge_close(&speech, settings.max_gap);
    let speech = apply_margins(&speech, settings.margin, asset.duration);
    let speech = drop_short(&speech, settings.min_segment);

    Ok(Edl {
        cuts: speech
            .into_iter()
            .map(|range| Cut {
                asset: asset.id.clone(),
                range,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::AssetId;

    fn asset(duration: f64) -> Asset {
        Asset {
            id: AssetId("a".into()),
            path: PathBuf::from("a.mp4"),
            duration,
        }
    }

    fn r(s: f64, e: f64) -> TimeRange {
        TimeRange::new(s, e).unwrap()
    }

    #[test]
    fn removes_silences_and_pads_cuts() {
        let settings = SilenceSettings {
            max_gap: 0.0,
            margin: 0.5,
            min_segment: 0.0,
        };
        let edl = build_silence_cut_edl(&asset(10.0), &[r(3.0, 7.0)], &settings).unwrap();
        let ranges: Vec<_> = edl.cuts.iter().map(|c| c.range).collect();
        assert_eq!(ranges, vec![r(0.0, 3.5), r(6.5, 10.0)]);
        assert!(edl.cuts.iter().all(|c| c.asset == AssetId("a".into())));
    }

    #[test]
    fn short_pauses_are_bridged() {
        let settings = SilenceSettings {
            max_gap: 0.5,
            margin: 0.0,
            min_segment: 0.0,
        };
        let edl = build_silence_cut_edl(&asset(10.0), &[r(4.0, 4.3)], &settings).unwrap();
        assert_eq!(edl.cuts.len(), 1);
        assert_eq!(edl.total_duration(), 10.0);
    }

    #[test]
    fn negative_setting_is_rejected() {
        let settings = SilenceSettings {
            margin: -1.0,
            ..SilenceSettings::default()
        };
        assert_eq!(
            build_silence_cut_edl(&asset(10.0), &[], &settings),
            Err(CoreError::InvalidSetting("margin"))
        );
    }

    #[test]
    fn silent_asset_gives_empty_edl() {
        let edl = build_silence_cut_edl(&asset(5.0), &[r(0.0, 5.0)], &SilenceSettings::default())
            .unwrap();
        assert!(edl.cuts.is_empty());
    }
}
