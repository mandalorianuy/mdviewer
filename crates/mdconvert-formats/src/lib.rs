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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredLimits {
    pub max_csv_records: u64,
    pub max_csv_fields_per_record: u64,
    pub max_csv_cells: u64,
    pub max_csv_field_bytes: u64,
    pub max_json_nodes: u64,
    pub max_json_object_entries: u64,
    pub max_json_array_entries: u64,
    pub max_json_depth: u64,
    pub max_xml_nodes: u64,
    pub max_xml_attributes_per_element: u64,
    pub max_xml_attributes: u64,
    pub max_xml_text_bytes: u64,
    pub max_xml_depth: u64,
}

impl Default for StructuredLimits {
    fn default() -> Self {
        Self {
            max_csv_records: 10_000,
            max_csv_fields_per_record: 4_096,
            max_csv_cells: 100_000,
            max_csv_field_bytes: 64 * 1024,
            max_json_nodes: 50_000,
            max_json_object_entries: 10_000,
            max_json_array_entries: 20_000,
            max_json_depth: 128,
            max_xml_nodes: 10_000,
            max_xml_attributes_per_element: 4_096,
            max_xml_attributes: 100_000,
            max_xml_text_bytes: 1024 * 1024,
            max_xml_depth: 128,
        }
    }
}

impl StructuredLimits {
    fn validate(&self) -> Result<(), ConversionError> {
        let values = [
            ("max_csv_records", self.max_csv_records),
            ("max_csv_fields_per_record", self.max_csv_fields_per_record),
            ("max_csv_cells", self.max_csv_cells),
            ("max_csv_field_bytes", self.max_csv_field_bytes),
            ("max_json_nodes", self.max_json_nodes),
            ("max_json_object_entries", self.max_json_object_entries),
            ("max_json_array_entries", self.max_json_array_entries),
            ("max_json_depth", self.max_json_depth),
            ("max_xml_nodes", self.max_xml_nodes),
            (
                "max_xml_attributes_per_element",
                self.max_xml_attributes_per_element,
            ),
            ("max_xml_attributes", self.max_xml_attributes),
            ("max_xml_text_bytes", self.max_xml_text_bytes),
            ("max_xml_depth", self.max_xml_depth),
        ];
        if let Some((name, _)) = values.into_iter().find(|(_, value)| *value == 0) {
            return Err(ConversionError::ConversionFailed {
                message: format!("structured limit {name} must be greater than zero"),
            });
        }
        if self.max_json_depth > 128 || self.max_xml_depth > 128 {
            return Err(ConversionError::ConversionFailed {
                message: "structured nesting depth cannot exceed the audited maximum of 128".into(),
            });
        }
        Ok(())
    }
}

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
    limits: &StructuredLimits,
) -> Result<(), ConversionError> {
    match detect::detect_format_with_limits(&request.source, bytes, limits) {
        Ok(actual) if actual == expected => Ok(()),
        Ok(actual) => Err(ConversionError::CorruptInput {
            message: format!(
                "converter expected {expected:?}, but input was detected as {actual:?}"
            ),
        }),
        Err(DetectionError::LimitExceeded {
            limit,
            actual,
            maximum,
        }) => Err(ConversionError::LimitExceeded {
            limit,
            actual,
            maximum,
        }),
        Err(error) => Err(ConversionError::CorruptInput {
            message: format!("structured format detection failed: {error}"),
        }),
    }
}

fn limit_exceeded(limit: &'static str, actual: u64, maximum: u64) -> ConversionError {
    ConversionError::LimitExceeded {
        limit,
        actual,
        maximum,
    }
}
