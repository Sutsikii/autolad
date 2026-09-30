//! Manual EDL editing: the operations an editor (human or agent) applies on top of
//! an automatically built EDL.

use serde::{Deserialize, Serialize};

use crate::domain::{Asset, AssetId, Cut, Edl, TimeRange};
use crate::error::CoreError;

/// Probed durations can differ from the real stream end by a few milliseconds.
const DURATION_TOLERANCE: f64 = 0.05;

/// One edit. Indices refer to the EDL as it is when the op runs, so a batch is
/// applied in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EdlOp {
    /// Removes the cut at `index`.
    Delete { index: usize },
    /// Changes the source range of a cut (absolute source seconds).
    Trim { index: usize, start: f64, end: f64 },
    /// Splits a cut in two at source time `at`, which must fall strictly inside it.
    Split { index: usize, at: f64 },
    /// Moves the cut at `from` so that it ends up at position `to`.
    Move { from: usize, to: usize },
    /// Inserts a new cut before position `index` (`index == len` appends).
    Insert {
        index: usize,
        asset: AssetId,
        start: f64,
        end: f64,
    },
    /// Removes every cut.
    Clear,
}

/// Applies `ops` in order to a copy of `edl`. All-or-nothing: if any op is invalid,
/// the error is returned and nothing is changed.
pub fn apply_ops(edl: &Edl, ops: &[EdlOp], assets: &[Asset]) -> Result<Edl, CoreError> {
    let mut cuts = edl.cuts.clone();
    for op in ops {
        apply_one(&mut cuts, op, assets)?;
    }
    Ok(Edl { cuts })
}

fn apply_one(cuts: &mut Vec<Cut>, op: &EdlOp, assets: &[Asset]) -> Result<(), CoreError> {
    match op {
        EdlOp::Delete { index } => {
            check_index(*index, cuts.len())?;
            cuts.remove(*index);
        }
        EdlOp::Trim { index, start, end } => {
            check_index(*index, cuts.len())?;
            let range = checked_range(&cuts[*index].asset, *start, *end, assets)?;
            cuts[*index].range = range;
        }
        EdlOp::Split { index, at } => {
            check_index(*index, cuts.len())?;
            let cut = cuts[*index].clone();
            if !(*at > cut.range.start && *at < cut.range.end) {
                return Err(CoreError::InvalidSetting(
                    "split point must be strictly inside the cut",
                ));
            }
            let first = Cut {
                asset: cut.asset.clone(),
                range: TimeRange::new(cut.range.start, *at)?,
            };
            let second = Cut {
                asset: cut.asset,
                range: TimeRange::new(*at, cut.range.end)?,
            };
            cuts[*index] = first;
            cuts.insert(*index + 1, second);
        }
        EdlOp::Move { from, to } => {
            check_index(*from, cuts.len())?;
            check_index(*to, cuts.len())?;
            let cut = cuts.remove(*from);
            cuts.insert(*to, cut);
        }
        EdlOp::Insert {
            index,
            asset,
            start,
            end,
        } => {
            if *index > cuts.len() {
                return Err(CoreError::IndexOutOfRange {
                    index: *index,
                    len: cuts.len(),
                });
            }
            let range = checked_range(asset, *start, *end, assets)?;
            cuts.insert(
                *index,
                Cut {
                    asset: asset.clone(),
                    range,
                },
            );
        }
        EdlOp::Clear => cuts.clear(),
    }
    Ok(())
}

fn check_index(index: usize, len: usize) -> Result<(), CoreError> {
    if index < len {
        Ok(())
    } else {
        Err(CoreError::IndexOutOfRange { index, len })
    }
}

fn checked_range(
    asset_id: &AssetId,
    start: f64,
    end: f64,
    assets: &[Asset],
) -> Result<TimeRange, CoreError> {
    let range = TimeRange::new(start, end)?;
    let asset = assets
        .iter()
        .find(|a| &a.id == asset_id)
        .ok_or_else(|| CoreError::UnknownAsset(asset_id.0.clone()))?;
    if range.end > asset.duration + DURATION_TOLERANCE {
        return Err(CoreError::OutOfBounds {
            asset: asset_id.0.clone(),
            start,
            end,
            duration: asset.duration,
        });
    }
    Ok(range)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn id(s: &str) -> AssetId {
        AssetId(s.into())
    }

    fn assets() -> Vec<Asset> {
        vec![
            Asset {
                id: id("a"),
                path: PathBuf::from("a.mp4"),
                duration: 10.0,
                has_audio: true,
            },
            Asset {
                id: id("b"),
                path: PathBuf::from("b.mp4"),
                duration: 5.0,
                has_audio: true,
            },
        ]
    }

    fn cut(a: &str, s: f64, e: f64) -> Cut {
        Cut {
            asset: id(a),
            range: TimeRange::new(s, e).unwrap(),
        }
    }

    fn edl() -> Edl {
        Edl {
            cuts: vec![cut("a", 0.0, 2.0), cut("a", 4.0, 6.0), cut("b", 1.0, 3.0)],
        }
    }

    fn ranges(e: &Edl) -> Vec<(f64, f64)> {
        e.cuts
            .iter()
            .map(|c| (c.range.start, c.range.end))
            .collect()
    }

    #[test]
    fn delete_removes_the_cut() {
        let out = apply_ops(&edl(), &[EdlOp::Delete { index: 1 }], &assets()).unwrap();
        assert_eq!(ranges(&out), vec![(0.0, 2.0), (1.0, 3.0)]);
    }

    #[test]
    fn trim_changes_the_source_range() {
        let op = EdlOp::Trim {
            index: 0,
            start: 0.5,
            end: 1.5,
        };
        let out = apply_ops(&edl(), &[op], &assets()).unwrap();
        assert_eq!(out.cuts[0].range, TimeRange::new(0.5, 1.5).unwrap());
    }

    #[test]
    fn trim_beyond_the_asset_is_rejected() {
        let op = EdlOp::Trim {
            index: 2,
            start: 1.0,
            end: 9.0,
        };
        assert!(matches!(
            apply_ops(&edl(), &[op], &assets()),
            Err(CoreError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn split_makes_two_contiguous_cuts() {
        let out = apply_ops(&edl(), &[EdlOp::Split { index: 1, at: 5.0 }], &assets()).unwrap();
        assert_eq!(
            ranges(&out),
            vec![(0.0, 2.0), (4.0, 5.0), (5.0, 6.0), (1.0, 3.0)]
        );
    }

    #[test]
    fn split_on_or_outside_the_edges_is_rejected() {
        for at in [4.0, 6.0, 7.0, 1.0] {
            let r = apply_ops(&edl(), &[EdlOp::Split { index: 1, at }], &assets());
            assert!(r.is_err(), "split at {at} should fail");
        }
    }

    #[test]
    fn move_reorders_cuts() {
        let out = apply_ops(&edl(), &[EdlOp::Move { from: 2, to: 0 }], &assets()).unwrap();
        assert_eq!(out.cuts[0].asset, id("b"));
        assert_eq!(out.cuts.len(), 3);
    }

    #[test]
    fn insert_adds_a_cut_and_can_append() {
        let op = |index| EdlOp::Insert {
            index,
            asset: id("b"),
            start: 0.0,
            end: 1.0,
        };
        let out = apply_ops(&edl(), &[op(0), op(4)], &assets()).unwrap();
        assert_eq!(out.cuts.len(), 5);
        assert_eq!(out.cuts[0].asset, id("b"));
        assert_eq!(out.cuts[4].asset, id("b"));
        assert!(apply_ops(&edl(), &[op(9)], &assets()).is_err());
    }

    #[test]
    fn insert_of_unknown_asset_is_rejected() {
        let op = EdlOp::Insert {
            index: 0,
            asset: id("ghost"),
            start: 0.0,
            end: 1.0,
        };
        assert_eq!(
            apply_ops(&edl(), &[op], &assets()),
            Err(CoreError::UnknownAsset("ghost".into()))
        );
    }

    #[test]
    fn ops_apply_in_order_against_the_evolving_edl() {
        // After deleting cut 0, the old cut 1 sits at index 0.
        let ops = [
            EdlOp::Delete { index: 0 },
            EdlOp::Trim {
                index: 0,
                start: 4.5,
                end: 5.5,
            },
        ];
        let out = apply_ops(&edl(), &ops, &assets()).unwrap();
        assert_eq!(ranges(&out), vec![(4.5, 5.5), (1.0, 3.0)]);
    }

    #[test]
    fn a_failing_op_leaves_the_input_untouched() {
        let original = edl();
        let ops = [EdlOp::Delete { index: 0 }, EdlOp::Delete { index: 99 }];
        assert_eq!(
            apply_ops(&original, &ops, &assets()),
            Err(CoreError::IndexOutOfRange { index: 99, len: 2 })
        );
        assert_eq!(original, edl());
    }

    #[test]
    fn clear_empties_the_edl() {
        assert!(apply_ops(&edl(), &[EdlOp::Clear], &assets())
            .unwrap()
            .cuts
            .is_empty());
    }

    #[test]
    fn ops_deserialize_from_tagged_json() {
        let op: EdlOp = serde_json::from_str(r#"{"op":"split","index":1,"at":2.5}"#).unwrap();
        assert_eq!(op, EdlOp::Split { index: 1, at: 2.5 });
    }
}
