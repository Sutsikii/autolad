use autolad_core::CoreError;
use serde::Serialize;
use specta::Type;
use thiserror::Error;

/// Error surfaced to the front. Serialized as `{ kind, message }` so the UI can
/// branch on `kind` without parsing text.
#[derive(Debug, Error, Serialize, Type, PartialEq)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum AppError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("{0}")]
    Internal(String),
}

impl From<CoreError> for AppError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::InvalidRange { .. }
            | CoreError::InvalidSetting(_)
            | CoreError::IndexOutOfRange { .. }
            | CoreError::OutOfBounds { .. }
            | CoreError::UnknownAsset(_) => Self::InvalidInput(e.to_string()),
            CoreError::Io(_) => Self::Internal(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_validation_errors_map_to_invalid_input() {
        let e: AppError = CoreError::InvalidSetting("margin").into();
        assert!(matches!(e, AppError::InvalidInput(_)));
    }

    #[test]
    fn io_errors_map_to_internal() {
        let e: AppError = CoreError::Io("boom".into()).into();
        assert_eq!(e, AppError::Internal("boom".into()));
    }

    #[test]
    fn serializes_with_kind_tag() {
        let json = serde_json::to_string(&AppError::Internal("x".into())).unwrap();
        assert_eq!(json, r#"{"kind":"internal","message":"x"}"#);
    }
}
