use std::collections::BTreeMap;

use mdconvert_core::{
    Alignment, Block, ConversionError, ConversionRequest, ConversionWarning, Converter, Document,
    DocumentMetadata, Inline, WarningCode,
};
use thiserror::Error;

use crate::{StructuredFormat, ensure_format, read_input, strip_utf8_bom, utf8};

const DELIMITERS: [u8; 4] = [b',', b';', b'\t', b'|'];

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DelimiterDetectionError {
    #[error("input is not valid UTF-8: {message}")]
    InvalidUtf8 { message: String },
    #[error("CSV delimiter is ambiguous between {delimiters:?}")]
    Ambiguous { delimiters: Vec<u8> },
    #[error("no supported CSV delimiter was found")]
    NoDelimiter,
    #[error("CSV input is corrupt: {message}")]
    Corrupt { message: String },
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CsvConverter;

impl Converter for CsvConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let bytes = read_input(request)?;
        utf8(&bytes, &request.source)?;
        ensure_format(request, &bytes, StructuredFormat::Csv)?;
        let payload = strip_utf8_bom(&bytes);
        let delimiter = match detect_delimiter(payload) {
            Ok(delimiter) => delimiter,
            Err(DelimiterDetectionError::NoDelimiter) => b',',
            Err(error) => {
                return Err(ConversionError::CorruptInput {
                    message: error.to_string(),
                });
            }
        };
        convert_csv(payload, delimiter)
    }
}

pub fn detect_delimiter(bytes: &[u8]) -> Result<u8, DelimiterDetectionError> {
    std::str::from_utf8(bytes).map_err(|error| DelimiterDetectionError::InvalidUtf8 {
        message: error.to_string(),
    })?;
    let mut scores = Vec::new();
    for delimiter in DELIMITERS {
        let widths = record_widths(bytes, delimiter)?;
        if let Some(score) = delimiter_score(&widths) {
            scores.push((delimiter, score));
        }
    }
    let best_score = scores.iter().map(|(_, score)| *score).max();
    let Some(best_score) = best_score else {
        return Err(DelimiterDetectionError::NoDelimiter);
    };
    let best = scores
        .into_iter()
        .filter_map(|(delimiter, score)| (score == best_score).then_some(delimiter))
        .collect::<Vec<_>>();
    if best.len() == 1 {
        Ok(best[0])
    } else {
        Err(DelimiterDetectionError::Ambiguous { delimiters: best })
    }
}

fn record_widths(bytes: &[u8], delimiter: u8) -> Result<Vec<usize>, DelimiterDetectionError> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes);
    reader
        .records()
        .map(|record| {
            record
                .map(|record| record.len())
                .map_err(|error| DelimiterDetectionError::Corrupt {
                    message: error.to_string(),
                })
        })
        .collect()
}

fn delimiter_score(widths: &[usize]) -> Option<(usize, usize, usize)> {
    let mut frequencies = BTreeMap::new();
    for &width in widths.iter().filter(|&&width| width > 1) {
        *frequencies.entry(width).or_insert(0usize) += 1;
    }
    let (&width, &frequency) = frequencies
        .iter()
        .max_by_key(|(width, frequency)| (**frequency, **width))?;
    Some((frequency, width, widths.len()))
}

fn convert_csv(bytes: &[u8], delimiter: u8) -> Result<Document, ConversionError> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes);
    let records = reader
        .records()
        .map(|record| {
            record
                .map(|record| record.iter().map(ToOwned::to_owned).collect::<Vec<_>>())
                .map_err(|error| ConversionError::CorruptInput {
                    message: format!("invalid CSV: {error}"),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut properties = BTreeMap::new();
    properties.insert("delimiter".into(), delimiter_name(delimiter).into());
    properties.insert("header_policy".into(), "first_record".into());
    let mut warnings = Vec::new();
    if records.is_empty() {
        warnings.push(ConversionWarning {
            code: WarningCode::TableDegraded,
            message: "CSV input is empty; no table was emitted".into(),
            page: None,
        });
    }
    let expected_width = records.first().map(Vec::len).unwrap_or(0);
    if records.iter().any(|record| record.len() != expected_width) {
        warnings.push(ConversionWarning {
            code: WarningCode::TableDegraded,
            message: format!(
                "ragged CSV rows were padded to the widest row; header has {expected_width} fields"
            ),
            page: None,
        });
    }
    let rows = records
        .into_iter()
        .map(|record| {
            record
                .into_iter()
                .map(|field| text_inlines(&field))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let blocks = (!rows.is_empty())
        .then_some(Block::Table {
            alignments: Vec::<Alignment>::new(),
            rows,
        })
        .into_iter()
        .collect();

    Ok(Document {
        metadata: DocumentMetadata {
            source_format: Some("csv".into()),
            properties,
            ..DocumentMetadata::default()
        },
        blocks,
        assets: Vec::new(),
        warnings,
    })
}

fn text_inlines(text: &str) -> Vec<Inline> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut inlines = Vec::new();
    for (index, line) in normalized.split('\n').enumerate() {
        if index > 0 {
            inlines.push(Inline::LineBreak);
        }
        if !line.is_empty() {
            inlines.push(Inline::Text(line.to_owned()));
        }
    }
    inlines
}

fn delimiter_name(delimiter: u8) -> &'static str {
    match delimiter {
        b',' => "comma",
        b';' => "semicolon",
        b'\t' => "tab",
        b'|' => "pipe",
        _ => "unknown",
    }
}
