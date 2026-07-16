use std::collections::HashMap;

use mdconvert_core::{
    Alignment, Block, ConversionError, ConversionRequest, Converter, Document, DocumentMetadata,
    Inline,
};

use crate::{
    archive::{
        Archive, ArchiveLimits, authenticate_ooxml, parse_xml_bytes, relationships,
        resolve_package_path,
    },
    xml::XmlNode,
};

const XLSX_MAIN_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const WORKSHEET_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const S_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const MAX_WORKSHEET_ROWS: u64 = 1_048_576;
const MAX_MATERIALIZED_ROWS: u64 = 100_000;
const MAX_WORKSHEET_COLUMNS: u64 = 16_384;
const MAX_WORKSHEET_CELLS: u64 = 1_000_000;

#[derive(Debug, Default, Clone, Copy)]
pub struct XlsxConverter;

impl Converter for XlsxConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let bytes = crate::read_input(request)?;
        self.convert_bytes(&bytes, request)
    }
}

impl XlsxConverter {
    pub fn convert_bytes(
        &self,
        bytes: &[u8],
        request: &ConversionRequest,
    ) -> Result<Document, ConversionError> {
        let archive = Archive::from_bytes(request, bytes, &ArchiveLimits::default())?;
        let content_types =
            authenticate_ooxml(&archive, "xl/workbook.xml", XLSX_MAIN_CONTENT_TYPE)?;
        reject_external_data(&archive, &content_types)?;
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
        let workbook_root = &workbook.roots[0];
        if !workbook_root.is(S_NS, "workbook") {
            return Err(corrupt_error(
                "XLSX workbook root has the wrong expanded name",
            ));
        }
        reject_external_formulas(workbook_root)?;
        for sheet in workbook_root.descendants_ns(S_NS, "sheet") {
            let name = sheet
                .attr_ns(None, "name")
                .ok_or_else(|| corrupt_error("workbook sheet is missing its name"))?;
            let id = sheet
                .attr_ns(Some(R_NS), "id")
                .ok_or_else(|| corrupt_error("workbook sheet is missing its relationship ID"))?;
            let relationship = rels.get(id).ok_or_else(|| {
                corrupt_error(format!(
                    "workbook sheet references missing relationship {id:?}"
                ))
            })?;
            if relationship.external {
                return Err(corrupt_error("external XLSX worksheets are unsupported"));
            }
            if relationship.kind != WORKSHEET_REL {
                return Err(corrupt_error(
                    "workbook sheet relationship is not a worksheet",
                ));
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
                properties: [
                    (
                        "formula_policy".into(),
                        "never_execute_use_cached_value".into(),
                    ),
                    ("ooxml_profile".into(), "transitional_only".into()),
                ]
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

fn reject_external_data(
    archive: &Archive,
    content_types: &crate::archive::ContentTypes,
) -> Result<(), ConversionError> {
    let forbidden = [
        "externallink",
        "externaldata",
        "connections",
        "querytable",
        "webquery",
        "oleobject",
        "linkeddata",
        "datamodel",
    ];
    if content_types.media_types().any(|media_type| {
        let lower = media_type.to_ascii_lowercase();
        forbidden.iter().any(|needle| lower.contains(needle))
    }) {
        return Err(corrupt_error(
            "XLSX external-data content types are unsupported",
        ));
    }
    for part in archive
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .filter(|name| name.starts_with("xl/") && name.ends_with(".rels"))
    {
        for relationship in relationships(archive, part)? {
            let lower = relationship.kind.to_ascii_lowercase();
            if relationship.external || forbidden.iter().any(|needle| lower.contains(needle)) {
                return Err(corrupt_error(format!(
                    "XLSX relationship {:?} is external or external-data",
                    relationship.kind
                )));
            }
        }
    }
    Ok(())
}

fn shared_strings(archive: &Archive) -> Result<Vec<String>, ConversionError> {
    let Some(entry) = archive.optional("xl/sharedStrings.xml") else {
        return Ok(Vec::new());
    };
    let parsed = parse_xml_bytes(&entry.data, "xl/sharedStrings.xml")?;
    if !parsed.roots[0].is(S_NS, "sst") {
        return Err(corrupt_error(
            "XLSX shared strings root has the wrong expanded name",
        ));
    }
    Ok(parsed.roots[0]
        .children()
        .filter(|node| node.is(S_NS, "si"))
        .map(|node| node.descendants_ns(S_NS, "t").map(XmlNode::text).collect())
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
    if !root.is(S_NS, "styleSheet") {
        return Err(corrupt_error(
            "XLSX styles root has the wrong expanded name",
        ));
    }
    let custom = root
        .descendants_ns(S_NS, "numFmt")
        .filter_map(|node| {
            Some((
                node.attr_ns(None, "numFmtId")?.parse().ok()?,
                node.attr_ns(None, "formatCode")?.to_owned(),
            ))
        })
        .collect();
    let formats = root
        .child_ns(S_NS, "cellXfs")
        .map(|node| {
            node.children()
                .filter(|node| node.is(S_NS, "xf"))
                .map(|node| {
                    node.attr_ns(None, "numFmtId")
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
    if !root.is(S_NS, "worksheet") {
        return Err(corrupt_error(
            "XLSX worksheet root has the wrong expanded name",
        ));
    }
    reject_external_formulas(root)?;
    validate_dimension(root)?;
    let sheet_data = root
        .child_ns(S_NS, "sheetData")
        .ok_or_else(|| corrupt_error(format!("worksheet {path:?} has no sheetData")))?;
    let mut sparse_rows = Vec::new();
    let mut cell_count = 0u64;
    let mut maximum_columns = 0usize;
    let mut previous_row = 0u64;
    for row in sheet_data.children().filter(|node| node.is(S_NS, "row")) {
        let row_number = row
            .attr_ns(None, "r")
            .ok_or_else(|| corrupt_error("worksheet row is missing its r attribute"))?
            .parse::<u64>()
            .map_err(|_| corrupt_error("worksheet row has an invalid r attribute"))?;
        if row_number == 0 || row_number > MAX_WORKSHEET_ROWS {
            return Err(limit("worksheet_rows", row_number, MAX_WORKSHEET_ROWS));
        }
        if row_number > MAX_MATERIALIZED_ROWS {
            return Err(limit("worksheet_rows", row_number, MAX_MATERIALIZED_ROWS));
        }
        if row_number <= previous_row {
            return Err(corrupt_error(
                "worksheet rows must be unique and strictly increasing",
            ));
        }
        previous_row = row_number;
        let mut output_row = Vec::new();
        let mut previous_column = None;
        for cell in row.children().filter(|node| node.is(S_NS, "c")) {
            cell_count = cell_count
                .checked_add(1)
                .ok_or_else(|| limit("worksheet_cells", u64::MAX, MAX_WORKSHEET_CELLS))?;
            if cell_count > MAX_WORKSHEET_CELLS {
                return Err(limit("worksheet_cells", cell_count, MAX_WORKSHEET_CELLS));
            }
            let reference = cell
                .attr_ns(None, "r")
                .ok_or_else(|| corrupt_error("worksheet cell is missing its reference"))?;
            let (column, reference_row) = parse_a1(reference)?;
            if reference_row != row_number {
                return Err(corrupt_error(format!(
                    "worksheet cell reference {reference:?} disagrees with containing row {row_number}"
                )));
            }
            if u64::try_from(column + 1).unwrap_or(u64::MAX) > MAX_WORKSHEET_COLUMNS {
                return Err(limit(
                    "worksheet_columns",
                    u64::try_from(column + 1).unwrap_or(u64::MAX),
                    MAX_WORKSHEET_COLUMNS,
                ));
            }
            if previous_column.is_some_and(|previous| column <= previous) {
                return Err(corrupt_error(
                    "worksheet cells must be unique and strictly column-ordered",
                ));
            }
            previous_column = Some(column);
            while output_row.len() <= column {
                output_row.push(Vec::new());
            }
            output_row[column] = cell_value(cell, shared, styles)?;
            maximum_columns = maximum_columns.max(column + 1);
        }
        sparse_rows.push((row_number, output_row));
    }
    if sparse_rows.is_empty() {
        sparse_rows.push((1, vec![Vec::new()]));
        maximum_columns = 1;
    }
    let maximum_row = sparse_rows.last().map_or(1, |(row, _)| *row);
    let materialized_cells = maximum_row
        .checked_mul(u64::try_from(maximum_columns).unwrap_or(u64::MAX))
        .ok_or_else(|| limit("worksheet_cells", u64::MAX, MAX_WORKSHEET_CELLS))?;
    if materialized_cells > MAX_WORKSHEET_CELLS {
        return Err(limit(
            "worksheet_cells",
            materialized_cells,
            MAX_WORKSHEET_CELLS,
        ));
    }
    let row_count = usize::try_from(maximum_row).map_err(|_| ConversionError::LimitExceeded {
        limit: "worksheet_rows",
        actual: maximum_row,
        maximum: MAX_MATERIALIZED_ROWS,
    })?;
    let mut rows = vec![Vec::new(); row_count];
    for (row_number, row) in sparse_rows {
        rows[usize::try_from(row_number - 1).expect("bounded worksheet row")] = row;
    }
    for row in &mut rows {
        row.resize(maximum_columns, Vec::new());
    }
    Ok(Block::Table {
        alignments: vec![Alignment::None; maximum_columns],
        rows,
    })
}

fn validate_dimension(root: &XmlNode) -> Result<(), ConversionError> {
    let Some(reference) = root
        .child_ns(S_NS, "dimension")
        .and_then(|node| node.attr_ns(None, "ref"))
    else {
        return Ok(());
    };
    let (start, end) = reference.split_once(':').unwrap_or((reference, reference));
    let (start_column, start_row) = parse_a1(start)?;
    let (end_column, end_row) = parse_a1(end)?;
    if start_row > end_row || start_column > end_column {
        return Err(corrupt_error("worksheet dimension is reversed"));
    }
    if end_row > MAX_MATERIALIZED_ROWS {
        return Err(limit("worksheet_rows", end_row, MAX_MATERIALIZED_ROWS));
    }
    let columns = u64::try_from(end_column - start_column + 1).unwrap_or(u64::MAX);
    if u64::try_from(end_column + 1).unwrap_or(u64::MAX) > MAX_WORKSHEET_COLUMNS {
        return Err(limit(
            "worksheet_columns",
            u64::try_from(end_column + 1).unwrap_or(u64::MAX),
            MAX_WORKSHEET_COLUMNS,
        ));
    }
    let rows = end_row - start_row + 1;
    let cells = rows.saturating_mul(columns);
    if cells > MAX_WORKSHEET_CELLS {
        return Err(limit(
            "worksheet_dimension_cells",
            cells,
            MAX_WORKSHEET_CELLS,
        ));
    }
    Ok(())
}

fn cell_value(
    cell: &XmlNode,
    shared: &[String],
    styles: &Styles,
) -> Result<Vec<Inline>, ConversionError> {
    let kind = cell.attr_ns(None, "t").unwrap_or("n");
    let raw = cell
        .child_ns(S_NS, "v")
        .map(XmlNode::text)
        .unwrap_or_default();
    let display = match kind {
        "s" => {
            let index = raw
                .parse::<usize>()
                .map_err(|_| corrupt_error("shared-string cell contains an invalid index"))?;
            shared.get(index).cloned().ok_or_else(|| {
                corrupt_error(format!("shared-string index {index} is out of range"))
            })?
        }
        "inlineStr" => cell.descendants_ns(S_NS, "t").map(XmlNode::text).collect(),
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
        .child_ns(S_NS, "f")
        .map(XmlNode::text)
        .filter(|value| !value.is_empty())
    {
        reject_external_formula(&formula)?;
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

fn reject_external_formula(formula: &str) -> Result<(), ConversionError> {
    let mut lexical = String::with_capacity(formula.len());
    let mut characters = formula.chars().peekable();
    let mut quoted_string = false;
    while let Some(character) = characters.next() {
        if character == '"' {
            if quoted_string && characters.peek() == Some(&'"') {
                characters.next();
                continue;
            }
            quoted_string = !quoted_string;
            continue;
        }
        if !quoted_string {
            lexical.push(character);
        }
    }
    if quoted_string {
        return Err(corrupt_error(
            "XLSX formula has an unterminated string literal",
        ));
    }
    let mut function_lexical = String::with_capacity(lexical.len());
    let mut characters = lexical.chars().peekable();
    let mut quoted_sheet = false;
    while let Some(character) = characters.next() {
        if character == '\'' {
            if quoted_sheet && characters.peek() == Some(&'\'') {
                characters.next();
                continue;
            }
            quoted_sheet = !quoted_sheet;
            continue;
        }
        if !quoted_sheet {
            function_lexical.push(character);
        }
    }
    if quoted_sheet {
        return Err(corrupt_error(
            "XLSX formula has an unterminated quoted sheet token",
        ));
    }
    if function_lexical.contains('|') {
        return Err(corrupt_error("XLSX DDE formula references are unsupported"));
    }
    let compact = function_lexical
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    for function in ["WEBSERVICE", "RTD", "IMAGE", "STOCKHISTORY"] {
        let needle = format!("{function}(");
        if compact.match_indices(&needle).any(|(start, _)| {
            compact[..start]
                .chars()
                .next_back()
                .is_none_or(|previous| !previous.is_ascii_alphanumeric() && previous != '_')
        }) {
            return Err(corrupt_error(format!(
                "XLSX external-data formula function {function} is unsupported"
            )));
        }
    }
    for (bang, _) in lexical.match_indices('!') {
        let prefix = lexical[..bang].trim_end();
        let token = if prefix.ends_with('\'') {
            quoted_sheet_token(prefix)?
        } else {
            prefix
                .rsplit(|character: char| {
                    character.is_whitespace() || "+-*/^&,=(><:".contains(character)
                })
                .next()
                .unwrap_or_default()
        };
        if token.contains('[') && token.contains(']') {
            return Err(corrupt_error(
                "XLSX external-workbook formula references are unsupported",
            ));
        }
    }
    Ok(())
}

fn quoted_sheet_token(prefix: &str) -> Result<&str, ConversionError> {
    let bytes = prefix.as_bytes();
    let mut cursor = bytes
        .len()
        .checked_sub(1)
        .ok_or_else(|| corrupt_error("XLSX formula has an empty quoted sheet token"))?;
    while cursor > 0 {
        cursor -= 1;
        if bytes[cursor] != b'\'' {
            continue;
        }
        if cursor > 0 && bytes[cursor - 1] == b'\'' {
            cursor -= 1;
            continue;
        }
        return Ok(&prefix[cursor..]);
    }
    Err(corrupt_error(
        "XLSX formula has an unterminated quoted sheet token",
    ))
}

fn reject_external_formulas(root: &XmlNode) -> Result<(), ConversionError> {
    for node in root
        .descendants_ns(S_NS, "f")
        .chain(root.descendants_ns(S_NS, "definedName"))
    {
        let formula = node.text();
        if !formula.is_empty() {
            reject_external_formula(&formula)?;
        }
    }
    Ok(())
}

fn format_display(raw: &str, cell: &XmlNode, styles: &Styles) -> String {
    let style_index = cell
        .attr_ns(None, "s")
        .and_then(|value| value.parse::<usize>().ok());
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

fn parse_a1(reference: &str) -> Result<(usize, u64), ConversionError> {
    let mut value = 0u64;
    let mut letters = 0usize;
    for byte in reference.bytes() {
        if !byte.is_ascii_uppercase() {
            break;
        }
        letters += 1;
        value = value
            .checked_mul(26)
            .and_then(|value| value.checked_add(u64::from(byte - b'A' + 1)))
            .ok_or_else(|| corrupt_error("worksheet column reference overflow"))?;
    }
    let suffix = &reference[letters..];
    if letters == 0
        || suffix.is_empty()
        || suffix.starts_with('0')
        || !suffix.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(corrupt_error(format!(
            "invalid worksheet cell reference {reference:?}"
        )));
    }
    let column = usize::try_from(value - 1)
        .map_err(|_| corrupt_error("worksheet column reference does not fit this platform"))?;
    let row = suffix
        .parse::<u64>()
        .map_err(|_| corrupt_error("worksheet row reference overflow"))?;
    Ok((column, row))
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
