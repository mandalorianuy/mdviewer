use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelError {
    #[error("heading level must be between 1 and 6, received {0}")]
    InvalidHeadingLevel(u8),
    #[error("asset ID must not be empty")]
    EmptyAssetId,
    #[error("source path must not be empty")]
    EmptySourcePath,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EmitError {
    #[error("asset ID {asset_id:?} is referenced but missing")]
    MissingAsset { asset_id: String },
    #[error("asset ID {asset_id:?} appears more than once")]
    DuplicateAssetId { asset_id: String },
    #[error("table alignment width mismatch: expected {expected} alignments, received {actual}")]
    TableAlignmentWidthMismatch { expected: usize, actual: usize },
    #[error("code language must not contain a line ending")]
    InvalidCodeLanguage,
}

#[derive(Debug, Error)]
pub enum ConversionError {
    #[error("invalid conversion request: {0}")]
    InvalidRequest(#[from] ModelError),
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("unsupported format: {format}")]
    UnsupportedFormat { format: String },
    #[error("corrupt input: {message}")]
    CorruptInput { message: String },
    #[error("encrypted input is not supported")]
    EncryptedInput,
    #[error("limit {limit} exceeded: actual {actual}, maximum {maximum}")]
    LimitExceeded {
        limit: &'static str,
        actual: u64,
        maximum: u64,
    },
    #[error("OCR is required to convert this input")]
    OcrRequired,
    #[error("conversion failed: {message}")]
    ConversionFailed { message: String },
}

impl ConversionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidRequest(_) => "invalid_request",
            Self::Io { .. } => "io",
            Self::UnsupportedFormat { .. } => "unsupported_format",
            Self::CorruptInput { .. } => "corrupt_input",
            Self::EncryptedInput => "encrypted_input",
            Self::LimitExceeded { .. } => "limit_exceeded",
            Self::OcrRequired => "ocr_required",
            Self::ConversionFailed { .. } => "conversion_failed",
        }
    }
}
