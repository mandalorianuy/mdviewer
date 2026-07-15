use std::{fs, path::Path};

use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::RcDom;
use mdconvert_core::ConversionError;

pub(crate) fn parse_file(path: &Path, max_input_bytes: u64) -> Result<RcDom, ConversionError> {
    let metadata = fs::metadata(path).map_err(|source| ConversionError::Io {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(ConversionError::CorruptInput {
            message: format!("HTML input is not a regular file: {}", path.display()),
        });
    }
    if metadata.len() > max_input_bytes {
        return Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            actual: metadata.len(),
            maximum: max_input_bytes,
        });
    }

    let bytes = fs::read(path).map_err(|source| ConversionError::Io {
        path: path.to_owned(),
        source,
    })?;
    let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual > max_input_bytes {
        return Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            actual,
            maximum: max_input_bytes,
        });
    }
    let input = std::str::from_utf8(&bytes).map_err(|error| ConversionError::CorruptInput {
        message: format!("HTML input is not valid UTF-8: {error}"),
    })?;

    Ok(parse_document(RcDom::default(), Default::default()).one(input))
}
