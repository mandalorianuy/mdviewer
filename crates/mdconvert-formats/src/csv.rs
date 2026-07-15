use std::collections::BTreeMap;

use mdconvert_core::{
    Alignment, Block, ConversionError, ConversionRequest, ConversionWarning, Converter, Document,
    DocumentMetadata, Inline, WarningCode,
};
use thiserror::Error;

use crate::{
    StructuredFormat, StructuredLimits, ensure_format, limit_exceeded, read_input, strip_utf8_bom,
    utf8,
};

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
    #[error("limit {limit} exceeded: actual {actual}, maximum {maximum}")]
    LimitExceeded {
        limit: &'static str,
        actual: u64,
        maximum: u64,
    },
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CsvConverter;

impl CsvConverter {
    pub fn convert_with_limits(
        &self,
        request: &ConversionRequest,
        limits: &StructuredLimits,
    ) -> Result<Document, ConversionError> {
        limits.validate()?;
        let bytes = read_input(request)?;
        convert_csv_bytes(request, &bytes, limits)
    }
}

pub(crate) fn convert_csv_bytes(
    request: &ConversionRequest,
    bytes: &[u8],
    limits: &StructuredLimits,
) -> Result<Document, ConversionError> {
    limits.validate()?;
    utf8(bytes, &request.source)?;
    ensure_format(request, bytes, StructuredFormat::Csv, limits)?;
    let payload = strip_utf8_bom(bytes);
    let delimiter = match detect_delimiter_with_limits(payload, limits) {
        Ok(delimiter) => delimiter,
        Err(DelimiterDetectionError::NoDelimiter) => b',',
        Err(DelimiterDetectionError::LimitExceeded {
            limit,
            actual,
            maximum,
        }) => return Err(limit_exceeded(limit, actual, maximum)),
        Err(error) => {
            return Err(ConversionError::CorruptInput {
                message: error.to_string(),
            });
        }
    };
    let input = utf8(payload, &request.source)?;
    let records = parse_records(input, delimiter, limits).map_err(parse_failure_to_conversion)?;
    document_from_records(records, delimiter)
}

impl Converter for CsvConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        self.convert_with_limits(request, &StructuredLimits::default())
    }
}

pub fn detect_delimiter(bytes: &[u8]) -> Result<u8, DelimiterDetectionError> {
    detect_delimiter_with_limits(bytes, &StructuredLimits::default())
}

pub(crate) fn detect_delimiter_with_limits(
    bytes: &[u8],
    limits: &StructuredLimits,
) -> Result<u8, DelimiterDetectionError> {
    let input = std::str::from_utf8(strip_utf8_bom(bytes)).map_err(|error| {
        DelimiterDetectionError::InvalidUtf8 {
            message: error.to_string(),
        }
    })?;
    let mut scores = Vec::new();
    let mut first_corrupt = None;
    let mut first_limit = None;
    for delimiter in DELIMITERS {
        match parse_records(input, delimiter, limits) {
            Ok(records) => {
                let widths = records.iter().map(Vec::len).collect::<Vec<_>>();
                if let Some(score) = delimiter_score(&widths) {
                    scores.push((delimiter, score));
                }
            }
            Err(ParseFailure::Corrupt { message }) => {
                first_corrupt.get_or_insert(message);
            }
            Err(ParseFailure::LimitExceeded {
                limit,
                actual,
                maximum,
            }) => {
                first_limit.get_or_insert((limit, actual, maximum));
            }
        }
    }
    let Some(best_score) = scores.iter().map(|(_, score)| *score).max() else {
        if let Some((limit, actual, maximum)) = first_limit {
            return Err(DelimiterDetectionError::LimitExceeded {
                limit,
                actual,
                maximum,
            });
        }
        if let Some(message) = first_corrupt {
            return Err(DelimiterDetectionError::Corrupt { message });
        }
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

#[derive(Debug)]
enum ParseFailure {
    Corrupt {
        message: String,
    },
    LimitExceeded {
        limit: &'static str,
        actual: u64,
        maximum: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldState {
    Start,
    Unquoted,
    Quoted,
    AfterQuote,
}

struct RecordBuilder<'a> {
    limits: &'a StructuredLimits,
    records: Vec<Vec<String>>,
    row: Vec<String>,
    field: String,
    cells: u64,
}

impl<'a> RecordBuilder<'a> {
    fn new(limits: &'a StructuredLimits) -> Self {
        Self {
            limits,
            records: Vec::new(),
            row: Vec::new(),
            field: String::new(),
            cells: 0,
        }
    }

    fn append(&mut self, character: char) -> Result<(), ParseFailure> {
        let actual = checked_add_usize(self.field.len(), character.len_utf8(), "csv_field_bytes")?;
        let actual = u64::try_from(actual).unwrap_or(u64::MAX);
        if actual > self.limits.max_csv_field_bytes {
            return Err(ParseFailure::LimitExceeded {
                limit: "csv_field_bytes",
                actual,
                maximum: self.limits.max_csv_field_bytes,
            });
        }
        self.field.push(character);
        Ok(())
    }

    fn finish_field(&mut self) -> Result<(), ParseFailure> {
        let actual_fields = checked_add_usize(self.row.len(), 1, "csv_fields_per_record")?;
        let actual_fields = u64::try_from(actual_fields).unwrap_or(u64::MAX);
        if actual_fields > self.limits.max_csv_fields_per_record {
            return Err(ParseFailure::LimitExceeded {
                limit: "csv_fields_per_record",
                actual: actual_fields,
                maximum: self.limits.max_csv_fields_per_record,
            });
        }
        let actual_cells = self
            .cells
            .checked_add(1)
            .ok_or(ParseFailure::LimitExceeded {
                limit: "csv_cells",
                actual: u64::MAX,
                maximum: self.limits.max_csv_cells,
            })?;
        if actual_cells > self.limits.max_csv_cells {
            return Err(ParseFailure::LimitExceeded {
                limit: "csv_cells",
                actual: actual_cells,
                maximum: self.limits.max_csv_cells,
            });
        }
        self.cells = actual_cells;
        self.row.push(std::mem::take(&mut self.field));
        Ok(())
    }

    fn finish_record(&mut self) -> Result<(), ParseFailure> {
        let actual = checked_add_usize(self.records.len(), 1, "csv_records")?;
        let actual = u64::try_from(actual).unwrap_or(u64::MAX);
        if actual > self.limits.max_csv_records {
            return Err(ParseFailure::LimitExceeded {
                limit: "csv_records",
                actual,
                maximum: self.limits.max_csv_records,
            });
        }
        self.records.push(std::mem::take(&mut self.row));
        Ok(())
    }
}

fn parse_records(
    input: &str,
    delimiter: u8,
    limits: &StructuredLimits,
) -> Result<Vec<Vec<String>>, ParseFailure> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let delimiter = char::from(delimiter);
    let mut builder = RecordBuilder::new(limits);
    let mut state = FieldState::Start;
    let mut characters = input.chars().peekable();
    let mut ended_with_record = false;
    while let Some(character) = characters.next() {
        ended_with_record = false;
        match state {
            FieldState::Quoted => {
                if character == '"' {
                    if characters.peek() == Some(&'"') {
                        characters.next();
                        builder.append('"')?;
                    } else {
                        state = FieldState::AfterQuote;
                    }
                } else if character == '\r' && characters.peek() == Some(&'\n') {
                    characters.next();
                    builder.append('\r')?;
                    builder.append('\n')?;
                } else if character == '\r' {
                    return Err(ParseFailure::Corrupt {
                        message: "quoted newlines must be LF or CRLF; lone CR is invalid".into(),
                    });
                } else {
                    builder.append(character)?;
                }
            }
            FieldState::AfterQuote => match character {
                value if value == delimiter => {
                    builder.finish_field()?;
                    state = FieldState::Start;
                }
                '\n' => {
                    builder.finish_field()?;
                    builder.finish_record()?;
                    state = FieldState::Start;
                    ended_with_record = true;
                }
                '\r' if characters.peek() == Some(&'\n') => {
                    characters.next();
                    builder.finish_field()?;
                    builder.finish_record()?;
                    state = FieldState::Start;
                    ended_with_record = true;
                }
                _ => {
                    return Err(ParseFailure::Corrupt {
                        message: "only a delimiter, CRLF, LF, or EOF may follow a closing quote"
                            .into(),
                    });
                }
            },
            FieldState::Start | FieldState::Unquoted => match character {
                '"' if state == FieldState::Start => state = FieldState::Quoted,
                '"' => {
                    return Err(ParseFailure::Corrupt {
                        message: "a quote may only appear at the start of a CSV field".into(),
                    });
                }
                value if value == delimiter => {
                    builder.finish_field()?;
                    state = FieldState::Start;
                }
                '\n' => {
                    builder.finish_field()?;
                    builder.finish_record()?;
                    state = FieldState::Start;
                    ended_with_record = true;
                }
                '\r' if characters.peek() == Some(&'\n') => {
                    characters.next();
                    builder.finish_field()?;
                    builder.finish_record()?;
                    state = FieldState::Start;
                    ended_with_record = true;
                }
                '\r' => {
                    return Err(ParseFailure::Corrupt {
                        message: "record line endings must be LF or CRLF; lone CR is invalid"
                            .into(),
                    });
                }
                _ => {
                    builder.append(character)?;
                    state = FieldState::Unquoted;
                }
            },
        }
    }
    if state == FieldState::Quoted {
        return Err(ParseFailure::Corrupt {
            message: "quoted CSV field is not terminated".into(),
        });
    }
    if !ended_with_record {
        builder.finish_field()?;
        builder.finish_record()?;
    }
    Ok(builder.records)
}

fn checked_add_usize(
    left: usize,
    right: usize,
    limit: &'static str,
) -> Result<usize, ParseFailure> {
    left.checked_add(right).ok_or(ParseFailure::LimitExceeded {
        limit,
        actual: u64::MAX,
        maximum: u64::MAX - 1,
    })
}

fn parse_failure_to_conversion(error: ParseFailure) -> ConversionError {
    match error {
        ParseFailure::Corrupt { message } => ConversionError::CorruptInput {
            message: format!("invalid CSV: {message}"),
        },
        ParseFailure::LimitExceeded {
            limit,
            actual,
            maximum,
        } => limit_exceeded(limit, actual, maximum),
    }
}

fn document_from_records(
    records: Vec<Vec<String>>,
    delimiter: u8,
) -> Result<Document, ConversionError> {
    let mut properties = BTreeMap::new();
    properties.insert("delimiter".into(), delimiter_name(delimiter).into());
    properties.insert("header_policy".into(), "first_record".into());
    properties.insert(
        "blank_record_policy".into(),
        "preserve_one_empty_cell".into(),
    );
    properties.insert("line_ending_policy".into(), "lf_or_crlf".into());
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
