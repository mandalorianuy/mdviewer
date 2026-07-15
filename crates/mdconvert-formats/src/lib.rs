mod csv;
mod detect;
mod json;
mod xml;

use std::{fs, path::Path};

use mdconvert_core::{ConversionError, ConversionRequest};

pub use crate::{
    csv::{CsvConverter, DelimiterDetectionError, detect_delimiter},
    detect::{DetectionError, StructuredFormat, detect_format},
    json::JsonConverter,
    xml::XmlConverter,
};

fn read_input(request: &ConversionRequest) -> Result<Vec<u8>, ConversionError> {
    let metadata = fs::metadata(&request.source).map_err(|source| ConversionError::Io {
        path: request.source.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(ConversionError::CorruptInput {
            message: format!("input is not a regular file: {}", request.source.display()),
        });
    }
    if metadata.len() > request.limits.max_input_bytes {
        return Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            actual: metadata.len(),
            maximum: request.limits.max_input_bytes,
        });
    }

    let bytes = fs::read(&request.source).map_err(|source| ConversionError::Io {
        path: request.source.clone(),
        source,
    })?;
    let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual > request.limits.max_input_bytes {
        return Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            actual,
            maximum: request.limits.max_input_bytes,
        });
    }
    Ok(bytes)
}

fn utf8<'a>(bytes: &'a [u8], path: &Path) -> Result<&'a str, ConversionError> {
    std::str::from_utf8(bytes).map_err(|error| ConversionError::CorruptInput {
        message: format!("{} is not valid UTF-8: {error}", path.display()),
    })
}

fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)
}

fn ensure_format(
    request: &ConversionRequest,
    bytes: &[u8],
    expected: StructuredFormat,
) -> Result<(), ConversionError> {
    match detect_format(&request.source, bytes) {
        Ok(actual) if actual == expected => Ok(()),
        Ok(actual) => Err(ConversionError::CorruptInput {
            message: format!(
                "converter expected {expected:?}, but input was detected as {actual:?}"
            ),
        }),
        Err(error) => Err(ConversionError::CorruptInput {
            message: format!("structured format detection failed: {error}"),
        }),
    }
}
