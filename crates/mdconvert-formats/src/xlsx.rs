use std::collections::HashMap;

use mdconvert_core::{
    Alignment, Block, ConversionError, ConversionRequest, Converter, Document, DocumentMetadata,
    Inline,
};

use crate::{
    archive::{Archive, ArchiveLimits, parse_xml_bytes, relationships, resolve_package_path},
    xml::XmlNode,
};

const MAX_WORKSHEET_ROWS: u64 = 1_048_576;
const MAX_WORKSHEET_COLUMNS: u64 = 16_384;
const MAX_WORKSHEET_CELLS: u64 = 1_000_000;

#[derive(Debug, Default, Clone, Copy)]
pub struct XlsxConverter;

impl Converter for XlsxConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let archive = Archive::open(request, &ArchiveLimits::default())?;
        if archive
            .entries
            .iter()
            .any(|entry| entry.name.starts_with("xl/externalLinks/"))
        {
            return Err(corrupt_error(
                "XLSX external link parts are unsupported and were not resolved",
            ));
        }
        let workbook = parse_xml_bytes(&archive.entry("xl/workbook.xml")?.data, "xl/workbook.xml")?;
        let rels = relationships(&archive, "xl/_rels/workbook.xml.rels")?
            .into_iter()
            .map(|relationship| (relationship.id.clone(), relationship))
            .collect::<HashMap<_, _>>();
        let shared = shared_strings(&archive)?;
        let styles = styles(&archive)?;
        let mut sheets = Vec::new();
        for sheet in workbook.roots[0].descendants("sheet") {
            let name = sheet
                .attr("name")
                .ok_or_else(|| corrupt_error("workbook sheet is missing its name"))?;
            let id = sheet
                .attr_prefixed("id")
                .ok_or_else(|| corrupt_error("workbook sheet is missing its relationship ID"))?;
            let relationship = rels.get(id).ok_or_else(|| {
                corrupt_error(format!(
                    "workbook sheet references missing relationship {id:?}"
                ))
            })?;
            if relationship.external {
                return Err(corrupt_error("external XLSX worksheets are unsupported"));
            }
            sheets.push((
                name.to_owned(),
                resolve_package_path("xl/workbook.xml", &relationship.target)?,
            ));
        }
        if sheets.is_empty() {
            return Err(corrupt_error("XLSX workbook contains no worksheets"));
        }
        let sheet_count = u64::try_from(sheets.len()).unwrap_or(u64::MAX);
        if sheet_count > u64::from(request.limits.max_pages) {
            return Err(ConversionError::LimitExceeded {
                limit: "pages",
                actual: sheet_count,
                maximum: u64::from(request.limits.max_pages),
            });
        }
        let mut blocks = Vec::new();
        for (name, path) in &sheets {
            blocks.push(Block::Heading {
                level: 2,
                content: vec![Inline::Text(name.clone())],
            });
            blocks.push(worksheet_table(&archive, path, &shared, &styles)?);
        }
        Ok(Document {
            metadata: DocumentMetadata {
                source_format: Some("xlsx".into()),
                page_count: Some(u32::try_from(sheets.len()).unwrap_or(u32::MAX)),
                properties: [(
                    "formula_policy".into(),
                    "never_execute_use_cached_value".into(),
                )]
                .into_iter()
                .collect(),
                ..DocumentMetadata::default()
            },
            blocks,
            assets: Vec::new(),
            warnings: Vec::new(),
        })
    }
}

fn shared_strings(archive: &Archive) -> Result<Vec<String>, ConversionError> {
    let Some(entry) = archive.optional("xl/sharedStrings.xml") else {
        return Ok(Vec::new());
    };
    let parsed = parse_xml_bytes(&entry.data, "xl/sharedStrings.xml")?;
    Ok(parsed.roots[0]
        .children()
        .filter(|node| node.local_name() == "si")
        .map(|node| node.descendants("t").map(XmlNode::text).collect())
        .collect())
}

#[derive(Default)]
struct Styles {
    formats: Vec<u32>,
    custom: HashMap<u32, String>,
}

fn styles(archive: &Archive) -> Result<Styles, ConversionError> {
    let Some(entry) = archive.optional("xl/styles.xml") else {
        return Ok(Styles::default());
    };
    let parsed = parse_xml_bytes(&entry.data, "xl/styles.xml")?;
    let root = &parsed.roots[0];
    let custom = root
        .descendants("numFmt")
        .filter_map(|node| {
            Some((
                node.attr("numFmtId")?.parse().ok()?,
                node.attr("formatCode")?.to_owned(),
            ))
        })
        .collect();
    let formats = root
        .child("cellXfs")
        .map(|node| {
            node.children()
                .filter(|node| node.local_name() == "xf")
                .map(|node| {
                    node.attr("numFmtId")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0)
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Styles { formats, custom })
}

fn worksheet_table(
    archive: &Archive,
    path: &str,
    shared: &[String],
    styles: &Styles,
) -> Result<Block, ConversionError> {
    let parsed = parse_xml_bytes(&archive.entry(path)?.data, path)?;
    let root = &parsed.roots[0];
    let sheet_data = root
        .child("sheetData")
        .ok_or_else(|| corrupt_error(format!("worksheet {path:?} has no sheetData")))?;
    let mut rows = Vec::new();
    let mut cell_count = 0u64;
    let mut maximum_columns = 0usize;
    for row in sheet_data
        .children()
        .filter(|node| node.local_name() == "row")
    {
        let row_number = row
            .attr("r")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_else(|| u64::try_from(rows.len() + 1).unwrap_or(u64::MAX));
        if row_number == 0 || row_number > MAX_WORKSHEET_ROWS {
            return Err(limit("worksheet_rows", row_number, MAX_WORKSHEET_ROWS));
        }
        let row_index = usize::try_from(row_number - 1)
            .map_err(|_| corrupt_error("worksheet row number does not fit this platform"))?;
        while rows.len() <= row_index {
            rows.push(Vec::new());
        }
        for cell in row.children().filter(|node| node.local_name() == "c") {
            cell_count = cell_count
                .checked_add(1)
                .ok_or_else(|| limit("worksheet_cells", u64::MAX, MAX_WORKSHEET_CELLS))?;
            if cell_count > MAX_WORKSHEET_CELLS {
                return Err(limit("worksheet_cells", cell_count, MAX_WORKSHEET_CELLS));
            }
            let reference = cell
                .attr("r")
                .ok_or_else(|| corrupt_error("worksheet cell is missing its reference"))?;
            let column = column_index(reference)?;
            if u64::try_from(column + 1).unwrap_or(u64::MAX) > MAX_WORKSHEET_COLUMNS {
                return Err(limit(
                    "worksheet_columns",
                    u64::try_from(column + 1).unwrap_or(u64::MAX),
                    MAX_WORKSHEET_COLUMNS,
                ));
            }
            while rows[row_index].len() <= column {
                rows[row_index].push(Vec::new());
            }
            rows[row_index][column] = cell_value(cell, shared, styles)?;
            maximum_columns = maximum_columns.max(column + 1);
        }
    }
    if rows.is_empty() {
        rows.push(vec![Vec::new()]);
        maximum_columns = 1;
    }
    for row in &mut rows {
        row.resize(maximum_columns, Vec::new());
    }
    Ok(Block::Table {
        alignments: vec![Alignment::None; maximum_columns],
        rows,
    })
}

fn cell_value(
    cell: &XmlNode,
    shared: &[String],
    styles: &Styles,
) -> Result<Vec<Inline>, ConversionError> {
    let kind = cell.attr("t").unwrap_or("n");
    let raw = cell.child("v").map(XmlNode::text).unwrap_or_default();
    let display = match kind {
        "s" => {
            let index = raw
                .parse::<usize>()
                .map_err(|_| corrupt_error("shared-string cell contains an invalid index"))?;
            shared.get(index).cloned().ok_or_else(|| {
                corrupt_error(format!("shared-string index {index} is out of range"))
            })?
        }
        "inlineStr" => cell.descendants("t").map(XmlNode::text).collect(),
        "b" => match raw.as_str() {
            "0" => "FALSE".into(),
            "1" => "TRUE".into(),
            _ => return Err(corrupt_error("boolean cell must contain 0 or 1")),
        },
        "n" | "str" | "e" | "d" => format_display(&raw, cell, styles),
        other => {
            return Err(corrupt_error(format!(
                "unsupported XLSX cell type {other:?}"
            )));
        }
    };
    if let Some(formula) = cell
        .child("f")
        .map(XmlNode::text)
        .filter(|value| !value.is_empty())
    {
        let mut output = vec![Inline::Code(format!("={formula}"))];
        if !display.is_empty() {
            output.push(Inline::Text(format!(" ({display})")));
        }
        Ok(output)
    } else if display.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![Inline::Text(display)])
    }
}

fn format_display(raw: &str, cell: &XmlNode, styles: &Styles) -> String {
    let style_index = cell.attr("s").and_then(|value| value.parse::<usize>().ok());
    let format = style_index
        .and_then(|index| styles.formats.get(index))
        .copied()
        .unwrap_or(0);
    let custom = styles.custom.get(&format).map(String::as_str).unwrap_or("");
    if (matches!(format, 9 | 10) || custom.contains('%'))
        && let Ok(value) = raw.parse::<f64>()
    {
        let decimals = if format == 10 || custom.contains("0.00%") {
            2
        } else {
            0
        };
        return format!("{:.*}%", decimals, value * 100.0);
    }
    raw.to_owned()
}

fn column_index(reference: &str) -> Result<usize, ConversionError> {
    let mut value = 0u64;
    let mut letters = 0usize;
    for byte in reference.bytes() {
        if !byte.is_ascii_alphabetic() {
            break;
        }
        letters += 1;
        value = value
            .checked_mul(26)
            .and_then(|value| value.checked_add(u64::from(byte.to_ascii_uppercase() - b'A' + 1)))
            .ok_or_else(|| corrupt_error("worksheet column reference overflow"))?;
    }
    if letters == 0
        || !reference[letters..]
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    {
        return Err(corrupt_error(format!(
            "invalid worksheet cell reference {reference:?}"
        )));
    }
    usize::try_from(value - 1)
        .map_err(|_| corrupt_error("worksheet column reference does not fit this platform"))
}

fn limit(name: &'static str, actual: u64, maximum: u64) -> ConversionError {
    ConversionError::LimitExceeded {
        limit: name,
        actual,
        maximum,
    }
}

fn corrupt_error(message: impl Into<String>) -> ConversionError {
    ConversionError::CorruptInput {
        message: message.into(),
    }
}
