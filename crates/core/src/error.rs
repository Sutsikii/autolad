use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum CoreError {
    #[error("invalid time range: start {start} must be finite, >= 0 and < end {end}")]
    InvalidRange { start: f64, end: f64 },
    #[error("unknown asset: {0}")]
    UnknownAsset(String),
    #[error("index {index} is out of range (the EDL has {len} cuts)")]
    IndexOutOfRange { index: usize, len: usize },
    #[error("range {start}..{end} exceeds asset {asset} (duration {duration})")]
    OutOfBounds {
        asset: String,
        start: f64,
        end: f64,
        duration: f64,
    },
    #[error("invalid setting: {0}")]
    InvalidSetting(&'static str),
    #[error("{0}")]
    Io(String),
}
