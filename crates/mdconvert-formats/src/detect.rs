use std::path::Path;

use thiserror::Error;

use crate::{DelimiterDetectionError, detect_delimiter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructuredFormat {
    Csv,
    Json,
    Xml,
}

impl StructuredFormat {
    fn from_extension(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "csv" => Some(Self::Csv),
            "json" => Some(Self::Json),
            "xml" => Some(Self::Xml),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DetectionError {
    #[error("input is not valid UTF-8: {message}")]
    InvalidUtf8 { message: String },
    #[error("format detection is ambiguous: {candidates:?}")]
    Ambiguous { candidates: Vec<StructuredFormat> },
    #[error("extension indicates {extension:?}, but content indicates {signature:?}")]
    Conflict {
        extension: StructuredFormat,
        signature: StructuredFormat,
    },
    #[error("no supported structured format signature was found")]
    Unsupported,
}

pub fn detect_format(path: &Path, bytes: &[u8]) -> Result<StructuredFormat, DetectionError> {
    let input = std::str::from_utf8(bytes).map_err(|error| DetectionError::InvalidUtf8 {
        message: error.to_string(),
    })?;
    let extension = StructuredFormat::from_extension(path);
    let trimmed = input.trim_start_matches('\u{feff}').trim_start();
    let strong_signature = if trimmed.starts_with('<') {
        Some(StructuredFormat::Xml)
    } else if trimmed.starts_with(['{', '[']) || is_json_scalar(trimmed) {
        Some(StructuredFormat::Json)
    } else {
        None
    };

    if let Some(signature) = strong_signature {
        if let Some(extension) = extension
            && extension != signature
        {
            return Err(DetectionError::Conflict {
                extension,
                signature,
            });
        }
        return Ok(signature);
    }

    match detect_delimiter(bytes) {
        Ok(_) => {
            if let Some(extension) = extension
                && extension != StructuredFormat::Csv
            {
                return Err(DetectionError::Conflict {
                    extension,
                    signature: StructuredFormat::Csv,
                });
            }
            Ok(StructuredFormat::Csv)
        }
        Err(DelimiterDetectionError::Ambiguous { .. }) => Err(DetectionError::Ambiguous {
            candidates: vec![StructuredFormat::Csv],
        }),
        Err(DelimiterDetectionError::InvalidUtf8 { message }) => {
            Err(DetectionError::InvalidUtf8 { message })
        }
        Err(DelimiterDetectionError::NoDelimiter) => extension.ok_or(DetectionError::Unsupported),
        Err(DelimiterDetectionError::Corrupt { .. }) => {
            extension.ok_or(DetectionError::Unsupported)
        }
    }
}

fn is_json_scalar(input: &str) -> bool {
    if input.is_empty() {
        return false;
    }
    let mut deserializer = serde_json::Deserializer::from_str(input);
    let parsed = serde::Deserialize::deserialize(&mut deserializer);
    matches!(
        parsed,
        Ok(serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_))
    ) && deserializer.end().is_ok()
}
