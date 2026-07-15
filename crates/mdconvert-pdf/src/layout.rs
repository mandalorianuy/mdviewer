use crate::{HeuristicConfig, RawGlyph, RawPage, RawRect};

#[derive(Debug, Clone)]
pub(crate) struct Line {
    pub page: u32,
    pub page_width: f32,
    pub page_height: f32,
    pub text: String,
    pub bounds: RawRect,
    pub font_size: f32,
    pub font_weight: Option<u16>,
    pub glyph_spans: Vec<(usize, usize, RawRect)>,
}

pub(crate) fn group_page_lines(page: &RawPage, config: &HeuristicConfig) -> Vec<Line> {
    let glyphs = page
        .glyphs
        .iter()
        .enumerate()
        .filter(|(_, glyph)| !glyph.text.chars().all(char::is_whitespace))
        .map(|(index, glyph)| (index, glyph.clone()))
        .collect::<Vec<_>>();

    let mut rows: Vec<Vec<(usize, RawGlyph)>> = Vec::new();
    for glyph in glyphs {
        if let Some(row) = rows
            .iter_mut()
            .find(|row| same_visual_line(&row[0].1, &glyph.1, config))
        {
            row.push(glyph);
        } else {
            rows.push(vec![glyph]);
        }
    }

    let mut lines = Vec::new();
    for row in rows {
        let mut geometric = row;
        geometric.sort_by(|left, right| left.1.bounds.left.total_cmp(&right.1.bounds.left));
        let mut segments: Vec<Vec<(usize, RawGlyph)>> = Vec::new();
        for glyph in geometric {
            let splits = segments
                .last()
                .and_then(|segment| segment.last())
                .is_some_and(|previous| {
                    glyph.1.bounds.left - previous.1.bounds.right > config.line_segment_gap_points
                });
            if splits || segments.is_empty() {
                segments.push(Vec::new());
            }
            segments.last_mut().unwrap().push(glyph);
        }
        for mut segment in segments {
            segment.sort_by_key(|(source_index, _)| *source_index);
            let segment = segment
                .into_iter()
                .map(|(_, glyph)| glyph)
                .collect::<Vec<_>>();
            if !segment.is_empty() {
                lines.push(build_line(page, segment, config));
            }
        }
    }
    lines.retain(|line| !line.text.is_empty());
    lines.sort_by(line_position_cmp);
    lines
}

fn same_visual_line(left: &RawGlyph, right: &RawGlyph, config: &HeuristicConfig) -> bool {
    let overlap =
        left.bounds.bottom.min(right.bounds.bottom) - left.bounds.top.max(right.bounds.top);
    let minimum_height = left.bounds.height().min(right.bounds.height());
    (minimum_height > 0.0 && overlap / minimum_height >= config.line_vertical_overlap_ratio)
        || (left.bounds.top - right.bounds.top).abs() <= config.line_y_tolerance_points
}

fn build_line(page: &RawPage, glyphs: Vec<RawGlyph>, config: &HeuristicConfig) -> Line {
    let mut bounds = glyphs[0].bounds;
    let mut text = String::new();
    let mut glyph_spans = Vec::with_capacity(glyphs.len());
    let mut size_sum = 0.0;
    let mut weights = Vec::new();
    let mut previous: Option<&RawGlyph> = None;
    for glyph in &glyphs {
        bounds = bounds.union(glyph.bounds);
        if previous.is_some_and(|previous| {
            glyph.bounds.left - previous.bounds.right
                > previous.font_size.max(glyph.font_size) * config.word_gap_ratio
        }) {
            text.push(' ');
        }
        let start = text.len();
        text.push_str(&glyph.text);
        glyph_spans.push((start, text.len(), glyph.bounds));
        if !glyph.text.chars().all(char::is_whitespace) {
            size_sum += glyph.font_size;
            if let Some(weight) = glyph.font_weight {
                weights.push(weight);
            }
        }
        previous = Some(glyph);
    }
    let visible_count = glyphs
        .iter()
        .filter(|glyph| !glyph.text.chars().all(char::is_whitespace))
        .count()
        .max(1);
    weights.sort_unstable();
    Line {
        page: page.number,
        page_width: page.width,
        page_height: page.height,
        text: text.trim().to_owned(),
        bounds,
        font_size: size_sum / visible_count as f32,
        font_weight: weights.get(weights.len() / 2).copied(),
        glyph_spans,
    }
}

pub(crate) fn line_position_cmp(left: &Line, right: &Line) -> std::cmp::Ordering {
    left.page
        .cmp(&right.page)
        .then_with(|| left.bounds.top.total_cmp(&right.bounds.top))
        .then_with(|| left.bounds.left.total_cmp(&right.bounds.left))
}

pub(crate) fn intersects(left: RawRect, right: RawRect) -> bool {
    left.left < right.right
        && left.right > right.left
        && left.top < right.bottom
        && left.bottom > right.top
}
