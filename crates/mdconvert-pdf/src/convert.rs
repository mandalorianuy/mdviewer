use std::{collections::BTreeMap, io::Cursor};

use mdconvert_core::{
    Alignment, Asset, AssetId, Block, ConversionError, ConversionWarning, Converter, Document,
    Inline, ListItem, WarningCode,
};
use url::Url;

use crate::{
    HeuristicConfig, RawDocument, RawImage, RawLink, RawPage, RawRect, RawRule, RuleKind,
    extract_pdf,
    layout::{Line, group_page_lines, intersects, line_position_cmp},
};

#[derive(Debug, Default, Clone, Copy)]
pub struct PdfConverter;

impl Converter for PdfConverter {
    fn convert(
        &self,
        request: &mdconvert_core::ConversionRequest,
    ) -> Result<Document, ConversionError> {
        reconstruct(extract_pdf(request)?)
    }
}

pub fn reconstruct(raw: RawDocument) -> Result<Document, ConversionError> {
    reconstruct_with_config(raw, &HeuristicConfig::default())
}

pub fn reconstruct_with_config(
    raw: RawDocument,
    config: &HeuristicConfig,
) -> Result<Document, ConversionError> {
    config.validate()?;
    let metadata = raw.metadata;
    let mut page_lines = raw
        .pages
        .iter()
        .map(|page| (page.number, group_page_lines(page, config)))
        .collect::<BTreeMap<_, _>>();
    remove_repeated_chrome(&mut page_lines, config);

    let mut warnings = Vec::new();
    let body_size = body_font_size(page_lines.values().flatten());
    let mut positioned = Vec::new();
    for page in &raw.pages {
        let lines = page_lines.remove(&page.number).unwrap_or_default();
        positioned.extend(infer_page(page, lines, body_size, config, &mut warnings));
    }

    let mut assets = Vec::new();
    let mut images = raw
        .pages
        .iter()
        .flat_map(|page| page.images.iter().map(move |image| (page.number, image)))
        .collect::<Vec<_>>();
    images.sort_by(|(left_page, left), (right_page, right)| {
        left_page
            .cmp(right_page)
            .then_with(|| left.bounds.top.total_cmp(&right.bounds.top))
            .then_with(|| left.bounds.left.total_cmp(&right.bounds.left))
            .then_with(|| left.index.cmp(&right.index))
    });
    for (sequence, (page, image)) in images.into_iter().enumerate() {
        let number = sequence + 1;
        let id = AssetId::new(format!("pdf-image-{number:03}"))
            .map_err(ConversionError::InvalidRequest)?;
        assets.push(Asset {
            id: id.clone(),
            file_name: format!("image-{number:03}.png"),
            media_type: "image/png".into(),
            data: encode_png(image)?,
        });
        positioned.push(PositionedBlock {
            page,
            top: image.bounds.top,
            left: image.bounds.left,
            order: positioned
                .iter()
                .filter(|block| block.page == page && block.top < image.bounds.top)
                .count()
                .saturating_mul(2),
            block: Block::Image {
                asset_id: id,
                alt: String::new(),
            },
        });
        warnings.push(ConversionWarning {
            code: WarningCode::MissingImageAlt,
            message: format!("PDF image {number} has no source alternative text"),
            page: Some(page),
        });
    }
    positioned.sort_by(positioned_cmp);

    Ok(Document {
        metadata,
        blocks: positioned.into_iter().map(|item| item.block).collect(),
        assets,
        warnings,
    })
}

#[derive(Debug)]
struct PositionedBlock {
    page: u32,
    top: f32,
    left: f32,
    order: usize,
    block: Block,
}

fn positioned_cmp(left: &PositionedBlock, right: &PositionedBlock) -> std::cmp::Ordering {
    left.page
        .cmp(&right.page)
        .then_with(|| left.order.cmp(&right.order))
        .then_with(|| left.top.total_cmp(&right.top))
        .then_with(|| left.left.total_cmp(&right.left))
}

fn infer_page(
    page: &RawPage,
    mut lines: Vec<Line>,
    body_size: f32,
    config: &HeuristicConfig,
    warnings: &mut Vec<ConversionWarning>,
) -> Vec<PositionedBlock> {
    let mut output = Vec::new();
    let table_block = if let Some(table) = infer_table(page, &lines, config, warnings) {
        let table_bounds = table.bounds;
        lines.retain(|line| {
            !table
                .consumed
                .iter()
                .any(|index| *index == line_identity(line))
        });
        Some(PositionedBlock {
            page: page.number,
            top: table_bounds.top,
            left: table_bounds.left,
            order: 0,
            block: table.block,
        })
    } else {
        None
    };

    order_columns(&mut lines, config, warnings);
    let links = &page.links;
    let mut index = 0;
    while index < lines.len() {
        if let Some((ordered, start, marker_len)) = parse_list_marker(&lines[index].text) {
            let list_left = lines[index].bounds.left;
            let mut items = Vec::new();
            let top = lines[index].bounds.top;
            while index < lines.len() {
                let Some((next_ordered, number, next_marker_len)) =
                    parse_list_marker(&lines[index].text)
                else {
                    break;
                };
                if next_ordered != ordered
                    || (lines[index].bounds.left - list_left).abs()
                        > config.list_indent_tolerance_points
                    || (ordered && number != start.saturating_add(items.len() as u64))
                {
                    break;
                }
                let content_line = line_without_prefix(&lines[index], next_marker_len);
                items.push(ListItem {
                    blocks: vec![Block::Paragraph {
                        content: linked_inlines(
                            &content_line.text,
                            &content_line,
                            links,
                            config,
                            warnings,
                        ),
                    }],
                });
                index += 1;
            }
            output.push(PositionedBlock {
                page: page.number,
                top,
                left: list_left,
                order: 0,
                block: Block::List {
                    ordered,
                    start: ordered.then_some(start),
                    items,
                },
            });
            let _ = marker_len;
            continue;
        }

        let line = &lines[index];
        if let Some(level) = heading_level(line, body_size, config) {
            output.push(PositionedBlock {
                page: page.number,
                top: line.bounds.top,
                left: line.bounds.left,
                order: 0,
                block: Block::Heading {
                    level,
                    content: linked_inlines(&line.text, line, links, config, warnings),
                },
            });
            index += 1;
            continue;
        }

        let top = line.bounds.top;
        let left = line.bounds.left;
        let mut paragraph_lines = vec![line.clone()];
        index += 1;
        while index < lines.len()
            && same_paragraph(paragraph_lines.last().unwrap(), &lines[index], config)
            && parse_list_marker(&lines[index].text).is_none()
            && heading_level(&lines[index], body_size, config).is_none()
        {
            paragraph_lines.push(lines[index].clone());
            index += 1;
        }
        let content = paragraph_inlines(&paragraph_lines, links, config, warnings);
        output.push(PositionedBlock {
            page: page.number,
            top,
            left,
            order: 0,
            block: Block::Paragraph { content },
        });
    }
    if let Some(table) = table_block {
        let position = output
            .iter()
            .position(|block| block.top > table.top)
            .unwrap_or(output.len());
        output.insert(position, table);
    }
    for (index, block) in output.iter_mut().enumerate() {
        block.order = index.saturating_mul(2).saturating_add(1);
    }
    output
}

fn line_without_prefix(line: &Line, prefix_bytes: usize) -> Line {
    let mut content = line.clone();
    content.text = line.text[prefix_bytes..].trim_start().to_owned();
    content.glyph_spans = line
        .glyph_spans
        .iter()
        .filter_map(|(start, end, bounds)| {
            (*end > prefix_bytes).then_some((
                start.saturating_sub(prefix_bytes),
                end.saturating_sub(prefix_bytes),
                *bounds,
            ))
        })
        .collect();
    if let Some((_, _, first)) = content.glyph_spans.first().copied() {
        content.bounds = content
            .glyph_spans
            .iter()
            .fold(first, |bounds, (_, _, glyph)| bounds.union(*glyph));
    }
    content
}

fn line_identity(line: &Line) -> (u32, u32, u32) {
    (
        line.page,
        line.bounds.top.to_bits(),
        line.bounds.left.to_bits(),
    )
}

struct TableInference {
    bounds: RawRect,
    block: Block,
    consumed: Vec<(u32, u32, u32)>,
}

fn infer_table(
    page: &RawPage,
    lines: &[Line],
    config: &HeuristicConfig,
    warnings: &mut Vec<ConversionWarning>,
) -> Option<TableInference> {
    if let Some(table) = ruled_table(page, lines, config, warnings) {
        return Some(table);
    }
    let rows = group_rows(lines, config.table_row_y_tolerance_points);
    let multi_rows = rows
        .iter()
        .filter(|row| row.len() >= config.table_min_columns)
        .collect::<Vec<_>>();
    if multi_rows.len() >= config.table_min_rows {
        let width = multi_rows[0].len();
        let aligned = multi_rows.iter().all(|row| {
            row.len() == width
                && row
                    .iter()
                    .zip(multi_rows[0].iter())
                    .all(|(cell, expected)| {
                        (cell.bounds.left - expected.bounds.left).abs()
                            <= config.table_alignment_tolerance_points
                    })
        });
        if aligned {
            return Some(table_from_rows(page, &multi_rows, config, warnings));
        }
    }
    if rows.iter().any(|row| row.len() >= config.table_min_columns) {
        warnings.push(ConversionWarning {
            code: WarningCode::TableDegraded,
            message: "aligned text did not repeat across enough complete rows; preserved as text"
                .into(),
            page: Some(page.number),
        });
    }
    None
}

fn ruled_table(
    page: &RawPage,
    lines: &[Line],
    config: &HeuristicConfig,
    warnings: &mut Vec<ConversionWarning>,
) -> Option<TableInference> {
    let (mut vertical, mut horizontal) = (Vec::new(), Vec::new());
    for rule in &page.rules {
        classify_axis(rule, config, &mut vertical, &mut horizontal);
    }
    deduplicate(&mut vertical, config.rule_axis_tolerance_points);
    deduplicate(&mut horizontal, config.rule_axis_tolerance_points);
    if vertical.len() < config.table_min_columns + 1 || horizontal.len() < config.table_min_rows + 1
    {
        return None;
    }
    vertical.sort_by(f32::total_cmp);
    horizontal.sort_by(f32::total_cmp);
    let bounds = RawRect {
        left: vertical[0],
        top: horizontal[0],
        right: *vertical.last().unwrap(),
        bottom: *horizontal.last().unwrap(),
    };
    if !vertical.iter().all(|coordinate| {
        rule_axis_covered(
            &page.rules,
            *coordinate,
            bounds.top,
            bounds.bottom,
            true,
            config.rule_axis_tolerance_points,
        )
    }) || !horizontal.iter().all(|coordinate| {
        rule_axis_covered(
            &page.rules,
            *coordinate,
            bounds.left,
            bounds.right,
            false,
            config.rule_axis_tolerance_points,
        )
    }) {
        return None;
    }
    let mut rows = Vec::new();
    let mut consumed = Vec::new();
    for y_pair in horizontal.windows(2) {
        let mut row = Vec::new();
        for x_pair in vertical.windows(2) {
            let cell_lines = lines
                .iter()
                .filter(|line| {
                    center_x(line.bounds) >= x_pair[0]
                        && center_x(line.bounds) <= x_pair[1]
                        && center_y(line.bounds) >= y_pair[0]
                        && center_y(line.bounds) <= y_pair[1]
                })
                .collect::<Vec<_>>();
            for line in &cell_lines {
                consumed.push(line_identity(line));
            }
            row.push(table_cell_inlines(
                &cell_lines,
                &page.links,
                config,
                warnings,
            ));
        }
        rows.push(row);
    }
    Some(TableInference {
        bounds,
        block: Block::Table {
            alignments: vec![Alignment::None; vertical.len() - 1],
            rows,
        },
        consumed,
    })
}

fn rule_axis_covered(
    rules: &[RawRule],
    coordinate: f32,
    start: f32,
    end: f32,
    vertical: bool,
    tolerance: f32,
) -> bool {
    let mut intervals = Vec::new();
    for rule in rules {
        let coordinates: &[f32] = match (rule.kind, vertical) {
            (RuleKind::Line, true) => std::slice::from_ref(&rule.bounds.left),
            (RuleKind::Line, false) => std::slice::from_ref(&rule.bounds.top),
            (RuleKind::Rectangle, true) => &[rule.bounds.left, rule.bounds.right],
            (RuleKind::Rectangle, false) => &[rule.bounds.top, rule.bounds.bottom],
        };
        if coordinates
            .iter()
            .any(|candidate| (*candidate - coordinate).abs() <= tolerance)
        {
            intervals.push(if vertical {
                (rule.bounds.top, rule.bounds.bottom)
            } else {
                (rule.bounds.left, rule.bounds.right)
            });
        }
    }
    intervals.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut covered = start;
    for (interval_start, interval_end) in intervals {
        if interval_start > covered + tolerance {
            return false;
        }
        covered = covered.max(interval_end);
        if covered + tolerance >= end {
            return true;
        }
    }
    false
}

fn classify_axis(
    rule: &RawRule,
    config: &HeuristicConfig,
    vertical: &mut Vec<f32>,
    horizontal: &mut Vec<f32>,
) {
    match rule.kind {
        RuleKind::Line => {
            if rule.bounds.width() <= config.rule_axis_tolerance_points {
                vertical.push(center_x(rule.bounds));
            }
            if rule.bounds.height() <= config.rule_axis_tolerance_points {
                horizontal.push(center_y(rule.bounds));
            }
        }
        RuleKind::Rectangle => {
            vertical.extend([rule.bounds.left, rule.bounds.right]);
            horizontal.extend([rule.bounds.top, rule.bounds.bottom]);
        }
    }
}

fn deduplicate(values: &mut Vec<f32>, tolerance: f32) {
    values.sort_by(f32::total_cmp);
    values.dedup_by(|left, right| (*left - *right).abs() <= tolerance);
}

fn table_from_rows(
    page: &RawPage,
    rows: &[&Vec<&Line>],
    config: &HeuristicConfig,
    warnings: &mut Vec<ConversionWarning>,
) -> TableInference {
    let bounds = rows
        .iter()
        .flat_map(|row| row.iter())
        .fold(rows[0][0].bounds, |bounds, line| bounds.union(line.bounds));
    let consumed = rows
        .iter()
        .flat_map(|row| row.iter())
        .map(|line| line_identity(line))
        .collect();
    let model_rows = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|line| linked_inlines(&line.text, line, &page.links, config, warnings))
                .collect()
        })
        .collect::<Vec<_>>();
    TableInference {
        bounds,
        block: Block::Table {
            alignments: vec![Alignment::None; rows[0].len()],
            rows: model_rows,
        },
        consumed,
    }
}

fn table_cell_inlines(
    lines: &[&Line],
    links: &[RawLink],
    config: &HeuristicConfig,
    warnings: &mut Vec<ConversionWarning>,
) -> Vec<Inline> {
    let mut output = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            push_inline(&mut output, Inline::Text(" ".into()));
        }
        for inline in linked_inlines(&line.text, line, links, config, warnings) {
            push_inline(&mut output, inline);
        }
    }
    output
}

fn group_rows(lines: &[Line], tolerance: f32) -> Vec<Vec<&Line>> {
    let mut rows: Vec<Vec<&Line>> = Vec::new();
    for line in lines {
        if let Some(row) = rows
            .iter_mut()
            .find(|row| (row[0].bounds.top - line.bounds.top).abs() <= tolerance)
        {
            row.push(line);
        } else {
            rows.push(vec![line]);
        }
    }
    for row in &mut rows {
        row.sort_by(|left, right| left.bounds.left.total_cmp(&right.bounds.left));
    }
    rows
}

fn order_columns(
    lines: &mut [Line],
    config: &HeuristicConfig,
    warnings: &mut Vec<ConversionWarning>,
) {
    lines.sort_by(line_position_cmp);
    if lines.len() < 2 {
        return;
    }
    let mut lefts = lines
        .iter()
        .map(|line| line.bounds.left)
        .collect::<Vec<_>>();
    lefts.sort_by(f32::total_cmp);
    let Some((_, split)) = lefts
        .windows(2)
        .enumerate()
        .map(|(index, pair)| (pair[1] - pair[0], index))
        .max_by(|left, right| left.0.total_cmp(&right.0))
    else {
        return;
    };
    let gap = lefts[split + 1] - lefts[split];
    if gap < config.column_min_gap_points {
        return;
    }
    let boundary = (lefts[split] + lefts[split + 1]) / 2.0;
    let left_count = lines
        .iter()
        .filter(|line| line.bounds.left < boundary)
        .count();
    let right_count = lines.len() - left_count;
    let left_edges = lines
        .iter()
        .filter(|line| line.bounds.left < boundary)
        .map(|line| line.bounds.left)
        .collect::<Vec<_>>();
    let right_edges = lines
        .iter()
        .filter(|line| line.bounds.left >= boundary)
        .map(|line| line.bounds.left)
        .collect::<Vec<_>>();
    let clustered = edge_spread(&left_edges) <= config.column_cluster_tolerance_points
        && edge_spread(&right_edges) <= config.column_cluster_tolerance_points;
    let left_right = lines
        .iter()
        .filter(|line| line.bounds.left < boundary)
        .map(|line| line.bounds.right)
        .max_by(f32::total_cmp)
        .unwrap_or(boundary);
    let right_left = right_edges
        .iter()
        .copied()
        .min_by(f32::total_cmp)
        .unwrap_or(boundary);
    let has_empty_gutter = right_left - left_right >= config.column_min_gap_points;
    if left_count >= config.column_min_lines_per_column
        && right_count >= config.column_min_lines_per_column
        && clustered
        && has_empty_gutter
    {
        lines.sort_by(|left, right| {
            let left_column = usize::from(left.bounds.left >= boundary);
            let right_column = usize::from(right.bounds.left >= boundary);
            left_column
                .cmp(&right_column)
                .then_with(|| left.bounds.top.total_cmp(&right.bounds.top))
        });
    } else if lines
        .iter()
        .any(|line| line.bounds.width() / line.page_width >= config.column_ambiguity_span_ratio)
    {
        warnings.push(ConversionWarning {
            code: WarningCode::AmbiguousReadingOrder,
            message: "possible columns lacked enough independent lines; preserved geometric order"
                .into(),
            page: Some(lines[0].page),
        });
    }
}

fn edge_spread(edges: &[f32]) -> f32 {
    let Some(minimum) = edges.iter().copied().min_by(f32::total_cmp) else {
        return 0.0;
    };
    let maximum = edges
        .iter()
        .copied()
        .max_by(f32::total_cmp)
        .unwrap_or(minimum);
    maximum - minimum
}

fn body_font_size<'a>(lines: impl Iterator<Item = &'a Line>) -> f32 {
    let mut counts: BTreeMap<u32, (usize, f32)> = BTreeMap::new();
    for line in lines.filter(|line| !line.text.trim().is_empty()) {
        let key = line.font_size.to_bits();
        let entry = counts.entry(key).or_insert((0, line.font_size));
        entry.0 += 1;
    }
    let highest_count = counts.values().map(|entry| entry.0).max().unwrap_or(0);
    counts
        .values()
        .filter(|entry| entry.0 == highest_count)
        .map(|entry| entry.1)
        .min_by(f32::total_cmp)
        .unwrap_or(10.0)
}

fn heading_level(line: &Line, body_size: f32, config: &HeuristicConfig) -> Option<u8> {
    if line.font_weight.unwrap_or_default() < config.heading_bold_weight || body_size <= 0.0 {
        return None;
    }
    let ratio = line.font_size / body_size;
    if ratio >= config.heading_level_1_size_ratio {
        Some(1)
    } else if ratio >= config.heading_level_2_size_ratio {
        Some(2)
    } else {
        None
    }
}

fn parse_list_marker(text: &str) -> Option<(bool, u64, usize)> {
    if let Some(marker) = ["- ", "* ", "• "]
        .into_iter()
        .find(|marker| text.starts_with(marker))
    {
        return Some((false, 1, marker.len()));
    }
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && text.get(digits..)?.starts_with(". ") {
        return Some((true, text[..digits].parse().ok()?, digits + 2));
    }
    None
}

fn same_paragraph(previous: &Line, next: &Line, config: &HeuristicConfig) -> bool {
    previous.page == next.page
        && next.bounds.top >= previous.bounds.top
        && next.bounds.top - previous.bounds.bottom
            <= previous.font_size.max(next.font_size) * config.paragraph_gap_ratio
        && (previous.bounds.left - next.bounds.left).abs()
            <= config.paragraph_indent_tolerance_points
}

fn should_dehyphenate(previous: &str, continuation: &str) -> bool {
    previous
        .chars()
        .rev()
        .nth(1)
        .is_some_and(char::is_alphabetic)
        && previous.ends_with('-')
        && continuation.chars().next().is_some_and(char::is_lowercase)
}

fn paragraph_inlines(
    lines: &[Line],
    links: &[RawLink],
    config: &HeuristicConfig,
    warnings: &mut Vec<ConversionWarning>,
) -> Vec<Inline> {
    let mut output = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            if should_dehyphenate(&lines[index - 1].text, &line.text) {
                remove_trailing_hyphen(&mut output);
            } else {
                push_inline(&mut output, Inline::Text(" ".into()));
            }
        }
        for inline in linked_inlines(&line.text, line, links, config, warnings) {
            push_inline(&mut output, inline);
        }
    }
    output
}

fn push_inline(output: &mut Vec<Inline>, inline: Inline) {
    if let (Some(Inline::Text(previous)), Inline::Text(next)) = (output.last_mut(), &inline) {
        previous.push_str(next);
    } else {
        output.push(inline);
    }
}

fn remove_trailing_hyphen(inlines: &mut [Inline]) {
    let Some(last) = inlines.last_mut() else {
        return;
    };
    match last {
        Inline::Text(text) => {
            if text.ends_with('-') {
                text.pop();
            }
        }
        Inline::Link { content, .. } | Inline::Emphasis(content) | Inline::Strong(content) => {
            remove_trailing_hyphen(content)
        }
        Inline::Code(_) | Inline::LineBreak => {}
    }
}

fn linked_inlines(
    text: &str,
    line: &Line,
    links: &[RawLink],
    config: &HeuristicConfig,
    warnings: &mut Vec<ConversionWarning>,
) -> Vec<Inline> {
    let mut ranges = Vec::new();
    for link in links
        .iter()
        .filter(|link| intersects(line.bounds, link.bounds))
    {
        if !safe_link(&link.target) {
            warnings.push(ConversionWarning {
                code: WarningCode::InvalidLinkSkipped,
                message: format!("unsafe PDF link destination was skipped: {}", link.target),
                page: Some(line.page),
            });
            continue;
        }
        let mut first = None;
        let mut last = None;
        for (start, end, bounds) in &line.glyph_spans {
            let intersection = intersection_area(*bounds, link.bounds);
            let glyph_area = bounds.width() * bounds.height();
            if glyph_area > 0.0 && intersection / glyph_area >= config.link_intersection_ratio {
                first.get_or_insert(*start);
                last = Some(*end);
            }
        }
        if let (Some(start), Some(end)) = (first, last) {
            ranges.push((start, end, link.target.clone()));
        }
    }
    ranges.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    let mut output = Vec::new();
    let mut cursor = 0;
    for (start, end, target) in ranges {
        if start < cursor || end <= start {
            continue;
        }
        if start > cursor {
            output.push(Inline::Text(text[cursor..start].into()));
        }
        output.push(Inline::Link {
            url: target,
            title: None,
            content: vec![Inline::Text(text[start..end].into())],
        });
        cursor = end;
    }
    if cursor < text.len() {
        output.push(Inline::Text(text[cursor..].into()));
    }
    if output.is_empty() {
        output.push(Inline::Text(text.into()));
    }
    output
}

fn safe_link(target: &str) -> bool {
    Url::parse(target).is_ok_and(|url| matches!(url.scheme(), "http" | "https" | "mailto" | "file"))
}

fn intersection_area(left: RawRect, right: RawRect) -> f32 {
    (left.right.min(right.right) - left.left.max(right.left)).max(0.0)
        * (left.bottom.min(right.bottom) - left.top.max(right.top)).max(0.0)
}

fn remove_repeated_chrome(pages: &mut BTreeMap<u32, Vec<Line>>, config: &HeuristicConfig) {
    let mut occurrences: BTreeMap<String, Vec<(u32, f32, bool)>> = BTreeMap::new();
    for (page, lines) in pages.iter() {
        for line in lines {
            let top_edge = line.bounds.top <= line.page_height * config.chrome_edge_ratio;
            let bottom_edge =
                line.bounds.bottom >= line.page_height * (1.0 - config.chrome_edge_ratio);
            if top_edge || bottom_edge {
                occurrences
                    .entry(normalize_chrome(&line.text))
                    .or_default()
                    .push((*page, line.bounds.top, bottom_edge));
            }
        }
    }
    let repeated = occurrences
        .into_iter()
        .filter_map(|(text, entries)| {
            let first = entries.first()?;
            let pages = entries
                .iter()
                .map(|entry| entry.0)
                .collect::<std::collections::BTreeSet<_>>();
            (pages.len() >= config.chrome_min_pages
                && entries.iter().all(|entry| {
                    entry.2 == first.2
                        && (entry.1 - first.1).abs() <= config.chrome_position_tolerance_points
                }))
            .then_some(text)
        })
        .collect::<std::collections::BTreeSet<_>>();
    for lines in pages.values_mut() {
        lines.retain(|line| {
            let at_edge = line.bounds.top <= line.page_height * config.chrome_edge_ratio
                || line.bounds.bottom >= line.page_height * (1.0 - config.chrome_edge_ratio);
            !at_edge || !repeated.contains(&normalize_chrome(&line.text))
        });
    }
}

fn normalize_chrome(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn encode_png(image: &RawImage) -> Result<Vec<u8>, ConversionError> {
    let expected = usize::try_from(image.pixel_width)
        .ok()
        .and_then(|width| {
            usize::try_from(image.pixel_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| ConversionError::ConversionFailed {
            message: "PDF image dimensions overflow addressable memory".into(),
        })?;
    if expected != image.rgba.len() {
        return Err(ConversionError::ConversionFailed {
            message: format!(
                "PDF image RGBA length mismatch: expected {expected}, received {}",
                image.rgba.len()
            ),
        });
    }
    let mut data = Vec::new();
    {
        let mut encoder = png::Encoder::new(
            Cursor::new(&mut data),
            image.pixel_width,
            image.pixel_height,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer =
            encoder
                .write_header()
                .map_err(|error| ConversionError::ConversionFailed {
                    message: format!("could not initialize PNG encoder: {error}"),
                })?;
        writer.write_image_data(&image.rgba).map_err(|error| {
            ConversionError::ConversionFailed {
                message: format!("could not encode PDF image as PNG: {error}"),
            }
        })?;
    }
    Ok(data)
}

fn center_x(rect: RawRect) -> f32 {
    (rect.left + rect.right) / 2.0
}

fn center_y(rect: RawRect) -> f32 {
    (rect.top + rect.bottom) / 2.0
}
