//! Pure algorithms turning analysis output (silences) into keepable segments.

use crate::domain::TimeRange;

/// Inverts detected silences into the speech segments to keep.
///
/// `silences` may be unsorted or overlapping; `total` is the asset duration.
pub fn silences_to_segments(silences: &[TimeRange], total: f64) -> Vec<TimeRange> {
    let mut sorted: Vec<TimeRange> = silences.to_vec();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));

    let mut kept = Vec::new();
    let mut cursor = 0.0_f64;
    for s in sorted {
        if s.start > cursor {
            push_range(&mut kept, cursor, s.start.min(total));
        }
        cursor = cursor.max(s.end);
    }
    if cursor < total {
        push_range(&mut kept, cursor, total);
    }
    kept
}

/// Merges segments separated by less than `max_gap` seconds, so tiny pauses
/// don't produce jarring micro-cuts.
pub fn merge_close(segments: &[TimeRange], max_gap: f64) -> Vec<TimeRange> {
    let mut sorted = segments.to_vec();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));

    let mut out: Vec<TimeRange> = Vec::new();
    for s in sorted {
        match out.last_mut() {
            Some(last) if s.start - last.end < max_gap => last.end = last.end.max(s.end),
            _ => out.push(s),
        }
    }
    out
}

/// Expands each segment by `margin` seconds on both sides, clamped to
/// `[0, total]`, then re-merges any segments that now overlap.
pub fn apply_margins(segments: &[TimeRange], margin: f64, total: f64) -> Vec<TimeRange> {
    let padded: Vec<TimeRange> = segments
        .iter()
        .map(|s| TimeRange {
            start: (s.start - margin).max(0.0),
            end: (s.end + margin).min(total),
        })
        .collect();
    // A gap of 0 merges only overlapping/touching ranges.
    merge_close(&padded, f64::MIN_POSITIVE)
}

/// Drops segments shorter than `min_len` seconds.
pub fn drop_short(segments: &[TimeRange], min_len: f64) -> Vec<TimeRange> {
    segments
        .iter()
        .copied()
        .filter(|s| s.duration() >= min_len)
        .collect()
}

fn push_range(out: &mut Vec<TimeRange>, start: f64, end: f64) {
    if let Ok(r) = TimeRange::new(start, end) {
        out.push(r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: f64, e: f64) -> TimeRange {
        TimeRange::new(s, e).unwrap()
    }

    #[test]
    fn no_silence_keeps_everything() {
        assert_eq!(silences_to_segments(&[], 10.0), vec![r(0.0, 10.0)]);
    }

    #[test]
    fn silences_are_inverted() {
        let out = silences_to_segments(&[r(2.0, 3.0), r(6.0, 8.0)], 10.0);
        assert_eq!(out, vec![r(0.0, 2.0), r(3.0, 6.0), r(8.0, 10.0)]);
    }

    #[test]
    fn leading_and_trailing_silence_are_removed() {
        let out = silences_to_segments(&[r(0.0, 1.0), r(9.0, 10.0)], 10.0);
        assert_eq!(out, vec![r(1.0, 9.0)]);
    }

    #[test]
    fn unsorted_overlapping_silences_are_handled() {
        let out = silences_to_segments(&[r(5.0, 7.0), r(1.0, 3.0), r(2.0, 6.0)], 10.0);
        assert_eq!(out, vec![r(0.0, 1.0), r(7.0, 10.0)]);
    }

    #[test]
    fn fully_silent_asset_yields_nothing() {
        assert!(silences_to_segments(&[r(0.0, 10.0)], 10.0).is_empty());
    }

    #[test]
    fn silence_past_end_is_clamped() {
        let out = silences_to_segments(&[r(8.0, 20.0)], 10.0);
        assert_eq!(out, vec![r(0.0, 8.0)]);
    }

    #[test]
    fn merge_close_joins_small_gaps_only() {
        let out = merge_close(&[r(0.0, 1.0), r(1.2, 2.0), r(5.0, 6.0)], 0.5);
        assert_eq!(out, vec![r(0.0, 2.0), r(5.0, 6.0)]);
    }

    #[test]
    fn margins_expand_clamp_and_remerge() {
        let out = apply_margins(&[r(0.2, 1.0), r(1.3, 2.0), r(9.8, 9.9)], 0.25, 10.0);
        assert_eq!(out, vec![r(0.0, 2.25), r(9.55, 10.0)]);
    }

    #[test]
    fn drop_short_filters_by_duration() {
        let out = drop_short(&[r(0.0, 0.1), r(1.0, 2.0)], 0.5);
        assert_eq!(out, vec![r(1.0, 2.0)]);
    }
}
