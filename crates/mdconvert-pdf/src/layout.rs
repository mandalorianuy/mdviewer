use crate::{HeuristicConfig, RawGlyph, RawPage, RawRect, RawWord};

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
    if !page.words.is_empty() {
        return group_page_words(page, config);
    }

    let glyphs = page
        .glyphs
        .iter()
        .enumerate()
        .filter(|(_, glyph)| {
            !glyph
                .text
                .chars()
                .all(|character| matches!(character, '\r' | '\n'))
        })
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
        for segment in segments {
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

fn group_page_words(page: &RawPage, config: &HeuristicConfig) -> Vec<Line> {
    let mut rows: Vec<Vec<&RawWord>> = Vec::new();
    for word in &page.words {
        if let Some(row) = rows
            .iter_mut()
            .find(|row| same_rect_line(row[0].bounds, word.bounds, config))
        {
            row.push(word);
        } else {
            rows.push(vec![word]);
        }
    }

    let mut lines = Vec::new();
    for mut row in rows {
        row.sort_by(|left, right| left.bounds.left.total_cmp(&right.bounds.left));
        let mut segments: Vec<Vec<&RawWord>> = Vec::new();
        for word in row {
            let splits = segments
                .last()
                .and_then(|segment| segment.last())
                .is_some_and(|previous| {
                    word.bounds.left - previous.bounds.right > config.line_segment_gap_points
                });
            if splits || segments.is_empty() {
                segments.push(Vec::new());
            }
            segments.last_mut().unwrap().push(word);
        }
        lines.extend(
            segments
                .into_iter()
                .filter(|segment| !segment.is_empty())
                .filter_map(|segment| build_word_line(page, &segment, config)),
        );
    }
    lines.sort_by(line_position_cmp);
    lines
}

fn build_word_line(page: &RawPage, words: &[&RawWord], config: &HeuristicConfig) -> Option<Line> {
    let mut geometric_words = words
        .iter()
        .filter_map(|word| {
            let mut glyphs = page
                .glyphs
                .get(word.glyph_start..word.glyph_end)?
                .iter()
                .filter(|glyph| {
                    !glyph
                        .text
                        .chars()
                        .all(|character| matches!(character, '\r' | '\n'))
                })
                .cloned()
                .collect::<Vec<_>>();
            glyphs.sort_by(|left, right| left.bounds.left.total_cmp(&right.bounds.left));
            (!glyphs.is_empty()).then_some(glyphs)
        })
        .collect::<Vec<_>>();
    if geometric_words.is_empty() {
        return None;
    }
    let mut glyphs: Vec<RawGlyph> = Vec::new();
    for word in geometric_words.drain(..) {
        if let (Some(previous), Some(next)) = (glyphs.last(), word.first()) {
            glyphs.push(RawGlyph {
                text: " ".into(),
                bounds: RawRect {
                    left: previous.bounds.right.min(next.bounds.left),
                    top: previous.bounds.top.min(next.bounds.top),
                    right: previous.bounds.right.max(next.bounds.left),
                    bottom: previous.bounds.bottom.max(next.bounds.bottom),
                },
                font_size: previous.font_size.max(next.font_size),
                font_name: None,
                font_weight: None,
            });
        }
        glyphs.extend(word);
    }
    Some(build_line(page, glyphs, config))
}

fn same_visual_line(left: &RawGlyph, right: &RawGlyph, config: &HeuristicConfig) -> bool {
    same_rect_line(left.bounds, right.bounds, config)
}

fn same_rect_line(left: RawRect, right: RawRect, config: &HeuristicConfig) -> bool {
    let overlap = left.bottom.min(right.bottom) - left.top.max(right.top);
    let minimum_height = left.height().min(right.height());
    (minimum_height > 0.0 && overlap / minimum_height >= config.line_vertical_overlap_ratio)
        || (left.top - right.top).abs() <= config.line_y_tolerance_points
}

fn build_line(page: &RawPage, glyphs: Vec<RawGlyph>, config: &HeuristicConfig) -> Line {
    let mut bounds = glyphs[0].bounds;
    let mut text = String::new();
    let mut glyph_spans = Vec::with_capacity(glyphs.len());
    let mut size_sum = 0.0;
    let mut weights = Vec::new();
    let mut previous_visible: Option<&RawGlyph> = None;
    for glyph in &glyphs {
        bounds = bounds.union(glyph.bounds);
        if glyph.text.chars().all(char::is_whitespace) {
            if !text.is_empty() && !text.ends_with(' ') {
                text.push(' ');
            }
            continue;
        }
        if previous_visible.is_some_and(|previous| {
            glyph.bounds.left - previous.bounds.right
                > previous.font_size.max(glyph.font_size) * config.word_gap_ratio
        }) && !text.ends_with(' ')
        {
            text.push(' ');
        }
        let start = text.len();
        text.push_str(&glyph.text);
        glyph_spans.push((start, text.len(), glyph.bounds));
        size_sum += glyph.font_size;
        if let Some(weight) = glyph.font_weight {
            weights.push(weight);
        }
        previous_visible = Some(glyph);
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
