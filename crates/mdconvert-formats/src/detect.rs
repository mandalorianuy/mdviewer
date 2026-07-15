use std::path::Path;

use thiserror::Error;

use crate::{
    StructuredLimits,
    csv::{DelimiterDetectionError, detect_delimiter_with_limits},
    json::validate_json_candidate,
    strip_utf8_bom,
    xml::validate_xml_candidate,
};

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
    #[error("limit {limit} exceeded: actual {actual}, maximum {maximum}")]
    LimitExceeded {
        limit: &'static str,
        actual: u64,
        maximum: u64,
    },
}

pub fn detect_format(path: &Path, bytes: &[u8]) -> Result<StructuredFormat, DetectionError> {
    detect_format_with_limits(path, bytes, &StructuredLimits::default())
}

pub(crate) fn detect_format_with_limits(
    path: &Path,
    bytes: &[u8],
    limits: &StructuredLimits,
) -> Result<StructuredFormat, DetectionError> {
    let input = std::str::from_utf8(strip_utf8_bom(bytes)).map_err(|error| {
        DetectionError::InvalidUtf8 {
            message: error.to_string(),
        }
    })?;
    let extension = StructuredFormat::from_extension(path);
    let mut candidates = Vec::new();
    let mut first_limit = None;

    match validate_json_candidate(input, limits) {
        Ok(true) => candidates.push(StructuredFormat::Json),
        Ok(false) => {}
        Err(error) => remember_limit(&mut first_limit, error),
    }
    match validate_xml_candidate(input, limits) {
        Ok(true) => candidates.push(StructuredFormat::Xml),
        Ok(false) => {}
        Err(error) => remember_limit(&mut first_limit, error),
    }
    match detect_delimiter_with_limits(bytes, limits) {
        Ok(_) => candidates.push(StructuredFormat::Csv),
        Err(DelimiterDetectionError::Ambiguous { .. }) => {
            return Err(DetectionError::Ambiguous {
                candidates: vec![StructuredFormat::Csv],
            });
        }
        Err(DelimiterDetectionError::LimitExceeded {
            limit,
            actual,
            maximum,
        }) => {
            first_limit.get_or_insert((limit, actual, maximum));
        }
        Err(
            DelimiterDetectionError::InvalidUtf8 { .. }
            | DelimiterDetectionError::NoDelimiter
            | DelimiterDetectionError::Corrupt { .. },
        ) => {}
    };

    candidates.sort_by_key(|format| match format {
        StructuredFormat::Csv => 0,
        StructuredFormat::Json => 1,
        StructuredFormat::Xml => 2,
    });
    candidates.dedup();
    match candidates.as_slice() {
        [] => {
            if let Some((limit, actual, maximum)) = first_limit {
                Err(DetectionError::LimitExceeded {
                    limit,
                    actual,
                    maximum,
                })
            } else {
                extension.ok_or(DetectionError::Unsupported)
            }
        }
        [signature] => {
            if let Some(extension) = extension
                && extension != *signature
            {
                Err(DetectionError::Conflict {
                    extension,
                    signature: *signature,
                })
            } else {
                Ok(*signature)
            }
        }
        _ => Err(DetectionError::Ambiguous { candidates }),
    }
}

fn remember_limit(
    slot: &mut Option<(&'static str, u64, u64)>,
    error: mdconvert_core::ConversionError,
) {
    if let mdconvert_core::ConversionError::LimitExceeded {
        limit,
        actual,
        maximum,
    } = error
    {
        slot.get_or_insert((limit, actual, maximum));
    }
}
