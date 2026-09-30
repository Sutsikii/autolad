//! Parsers for ffmpeg text output.

use autolad_core::TimeRange;

use crate::error::MediaError;

/// Parses `silencedetect` stderr lines into silent ranges.
///
/// A trailing `silence_start` with no matching end means the file ends in
/// silence; it is closed at `total_duration`.
pub fn parse_silencedetect(
    stderr: &str,
    total_duration: f64,
) -> Result<Vec<TimeRange>, MediaError> {
    let mut silences = Vec::new();
    let mut open: Option<f64> = None;

    for line in stderr.lines() {
        if let Some(v) = value_after(line, "silence_start:") {
            open = Some(parse_secs(v)?);
        } else if let Some(v) = value_after(line, "silence_end:") {
            let end = parse_secs(v)?;
            // ffmpeg can report a start slightly below zero; clamp it.
            let start = open.take().unwrap_or(0.0).max(0.0);
            silences.push(range(start, end)?);
        }
    }
    if let Some(start) = open {
        silences.push(range(start.max(0.0), total_duration)?);
    }
    Ok(silences)
}

/// Extracts the current position in seconds from one `-progress pipe:1` line.
/// Returns `None` for lines that don't carry a usable timestamp
/// (ffmpeg emits `out_time_us=N/A` before the first frame).
pub fn parse_progress_time(line: &str) -> Option<f64> {
    let micros: i64 = line.trim().strip_prefix("out_time_us=")?.parse().ok()?;
    (micros >= 0).then(|| micros as f64 / 1_000_000.0)
}

fn value_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = &line[line.find(key)? + key.len()..];
    rest.split_whitespace().next()
}

fn parse_secs(v: &str) -> Result<f64, MediaError> {
    v.parse()
        .map_err(|_| MediaError::Parse(format!("not a number: {v}")))
}

fn range(start: f64, end: f64) -> Result<TimeRange, MediaError> {
    TimeRange::new(start, end).map_err(|e| MediaError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: f64, e: f64) -> TimeRange {
        TimeRange::new(s, e).unwrap()
    }

    const SAMPLE: &str = "\
[silencedetect @ 0x1] silence_start: 1.5
[silencedetect @ 0x1] silence_end: 3.25 | silence_duration: 1.75
size=N/A time=00:00:05.00 bitrate=N/A
[silencedetect @ 0x1] silence_start: 7
[silencedetect @ 0x1] silence_end: 8.5 | silence_duration: 1.5
";

    #[test]
    fn parses_start_end_pairs() {
        let out = parse_silencedetect(SAMPLE, 10.0).unwrap();
        assert_eq!(out, vec![r(1.5, 3.25), r(7.0, 8.5)]);
    }

    #[test]
    fn unterminated_silence_closes_at_total_duration() {
        let out = parse_silencedetect("silence_start: 9.0\n", 10.0).unwrap();
        assert_eq!(out, vec![r(9.0, 10.0)]);
    }

    #[test]
    fn negative_start_is_clamped() {
        let out = parse_silencedetect("silence_start: -0.02\nsilence_end: 1.0\n", 10.0).unwrap();
        assert_eq!(out, vec![r(0.0, 1.0)]);
    }

    #[test]
    fn garbage_number_is_an_error() {
        assert!(parse_silencedetect("silence_start: abc\n", 10.0).is_err());
    }

    #[test]
    fn no_silence_gives_empty() {
        assert!(parse_silencedetect("frame=1 fps=0\n", 10.0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn progress_time_is_parsed() {
        assert_eq!(parse_progress_time("out_time_us=2500000"), Some(2.5));
        assert_eq!(parse_progress_time("out_time_us=N/A"), None);
        assert_eq!(parse_progress_time("out_time_us=-5"), None);
        assert_eq!(parse_progress_time("frame=10"), None);
    }
}
