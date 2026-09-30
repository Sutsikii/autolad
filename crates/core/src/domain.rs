use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Stable identifier of an imported asset (content hash, see cache rules in CLAUDE.md).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AssetId(pub String);

/// Half-open interval `[start, end)` in seconds on a source timeline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
}

impl TimeRange {
    pub fn new(start: f64, end: f64) -> Result<Self, CoreError> {
        // is_finite() rules out NaN/inf first, so the comparisons below are total.
        if !start.is_finite() || !end.is_finite() || start < 0.0 || start >= end {
            return Err(CoreError::InvalidRange { start, end });
        }
        Ok(Self { start, end })
    }

    pub fn duration(&self) -> f64 {
        self.end - self.start
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Asset {
    pub id: AssetId,
    pub path: PathBuf,
    pub duration: f64,
}

/// One kept portion of a source asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Cut {
    pub asset: AssetId,
    pub range: TimeRange,
}

/// Edit decision list: the ordered cuts that make up the final video.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Edl {
    pub cuts: Vec<Cut>,
}

impl Edl {
    pub fn total_duration(&self) -> f64 {
        self.cuts.iter().map(|c| c.range.duration()).sum()
    }

    /// Maps a position on the edited timeline to `(cut index, source time)`.
    /// `None` when `time` is negative, not finite, or past the end.
    pub fn locate(&self, time: f64) -> Option<(usize, f64)> {
        if !time.is_finite() || time < 0.0 {
            return None;
        }
        let mut offset = 0.0;
        for (index, cut) in self.cuts.iter().enumerate() {
            let len = cut.range.duration();
            if time < offset + len {
                return Some((index, cut.range.start + (time - offset)));
            }
            offset += len;
        }
        None
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Project {
    pub assets: Vec<Asset>,
    pub edl: Edl,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_rejects_invalid_bounds() {
        assert!(TimeRange::new(1.0, 1.0).is_err());
        assert!(TimeRange::new(2.0, 1.0).is_err());
        assert!(TimeRange::new(-0.1, 1.0).is_err());
        assert!(TimeRange::new(f64::NAN, 1.0).is_err());
        assert!(TimeRange::new(0.0, f64::INFINITY).is_err());
    }

    #[test]
    fn range_duration() {
        let r = TimeRange::new(1.5, 4.0).unwrap();
        assert_eq!(r.duration(), 2.5);
    }

    #[test]
    fn locate_maps_timeline_time_to_source_time() {
        let cut = |s, e| Cut {
            asset: AssetId("a".into()),
            range: TimeRange::new(s, e).unwrap(),
        };
        let edl = Edl {
            cuts: vec![cut(10.0, 12.0), cut(30.0, 33.0)],
        };
        assert_eq!(edl.locate(0.0), Some((0, 10.0)));
        assert_eq!(edl.locate(1.5), Some((0, 11.5)));
        // Exactly on the boundary belongs to the next cut.
        assert_eq!(edl.locate(2.0), Some((1, 30.0)));
        assert_eq!(edl.locate(4.5), Some((1, 32.5)));
        assert_eq!(edl.locate(5.0), None);
        assert_eq!(edl.locate(-1.0), None);
        assert_eq!(edl.locate(f64::NAN), None);
        assert_eq!(Edl::default().locate(0.0), None);
    }

    #[test]
    fn edl_total_duration_sums_cuts() {
        let cut = |s, e| Cut {
            asset: AssetId("a".into()),
            range: TimeRange::new(s, e).unwrap(),
        };
        let edl = Edl {
            cuts: vec![cut(0.0, 1.0), cut(5.0, 7.5)],
        };
        assert_eq!(edl.total_duration(), 3.5);
    }
}
