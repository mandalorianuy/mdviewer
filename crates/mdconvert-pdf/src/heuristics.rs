use mdconvert_core::ConversionError;

/// Named deterministic thresholds used by PDF layout reconstruction.
#[derive(Debug, Clone, PartialEq)]
pub struct HeuristicConfig {
    /// Maximum vertical displacement, in PDF points, within one text line.
    pub line_y_tolerance_points: f32,
    /// Minimum shared glyph height ratio for membership in one text line.
    pub line_vertical_overlap_ratio: f32,
    /// Horizontal gap, in PDF points, that splits same-baseline text segments.
    pub line_segment_gap_points: f32,
    /// Horizontal glyph gap as a font-size ratio that inserts a word space.
    pub word_gap_ratio: f32,
    /// Maximum inter-line gap as a ratio of the larger line font size.
    pub paragraph_gap_ratio: f32,
    /// Maximum left-edge displacement, in PDF points, within one paragraph.
    pub paragraph_indent_tolerance_points: f32,
    /// Minimum empty horizontal gap, in PDF points, between two columns.
    pub column_min_gap_points: f32,
    /// Maximum left-edge displacement, in PDF points, within a column cluster.
    pub column_cluster_tolerance_points: f32,
    /// Minimum line count required in each unambiguous column.
    pub column_min_lines_per_column: usize,
    /// Page-width ratio above which a spanning line makes column order ambiguous.
    pub column_ambiguity_span_ratio: f32,
    /// Font-size ratio to body text required for a level-one heading.
    pub heading_level_1_size_ratio: f32,
    /// Font-size ratio to body text required for a level-two heading.
    pub heading_level_2_size_ratio: f32,
    /// Minimum numeric font weight considered bold for heading inference.
    pub heading_bold_weight: u16,
    /// Maximum marker indentation drift, in PDF points, within one list.
    pub list_indent_tolerance_points: f32,
    /// Maximum left-edge drift, in PDF points, for a borderless table column.
    pub table_alignment_tolerance_points: f32,
    /// Maximum baseline drift, in PDF points, within one table row.
    pub table_row_y_tolerance_points: f32,
    /// Maximum adjacent table-row distance as a ratio of row font size.
    pub table_max_row_gap_ratio: f32,
    /// Minimum complete row count required for borderless table inference.
    pub borderless_table_min_rows: usize,
    /// Minimum repeated row count required for ruled table inference.
    pub table_min_rows: usize,
    /// Minimum aligned column count required for table inference.
    pub table_min_columns: usize,
    /// Maximum thickness, in PDF points, considered an axis-aligned grid rule.
    pub rule_axis_tolerance_points: f32,
    /// Top and bottom page-height ratio eligible for repeated chrome removal.
    pub chrome_edge_ratio: f32,
    /// Maximum repeated chrome position drift, in PDF points.
    pub chrome_position_tolerance_points: f32,
    /// Minimum page count on which normalized chrome must repeat.
    pub chrome_min_pages: usize,
    /// Minimum intersected glyph-area ratio required to apply a link.
    pub link_intersection_ratio: f32,
}

impl Default for HeuristicConfig {
    fn default() -> Self {
        Self {
            line_y_tolerance_points: 2.0,
            line_vertical_overlap_ratio: 0.3,
            line_segment_gap_points: 24.0,
            word_gap_ratio: 0.25,
            paragraph_gap_ratio: 0.8,
            paragraph_indent_tolerance_points: 8.0,
            column_min_gap_points: 48.0,
            column_cluster_tolerance_points: 24.0,
            column_min_lines_per_column: 2,
            column_ambiguity_span_ratio: 0.4,
            heading_level_1_size_ratio: 1.8,
            heading_level_2_size_ratio: 1.35,
            heading_bold_weight: 600,
            list_indent_tolerance_points: 8.0,
            table_alignment_tolerance_points: 8.0,
            table_row_y_tolerance_points: 2.0,
            table_max_row_gap_ratio: 4.5,
            borderless_table_min_rows: 3,
            table_min_rows: 2,
            table_min_columns: 2,
            rule_axis_tolerance_points: 2.0,
            chrome_edge_ratio: 0.08,
            chrome_position_tolerance_points: 3.0,
            chrome_min_pages: 2,
            link_intersection_ratio: 0.25,
        }
    }
}

impl HeuristicConfig {
    pub(crate) fn validate(&self) -> Result<(), ConversionError> {
        let values = [
            ("line_y_tolerance_points", self.line_y_tolerance_points),
            (
                "line_vertical_overlap_ratio",
                self.line_vertical_overlap_ratio,
            ),
            ("line_segment_gap_points", self.line_segment_gap_points),
            ("word_gap_ratio", self.word_gap_ratio),
            ("paragraph_gap_ratio", self.paragraph_gap_ratio),
            (
                "paragraph_indent_tolerance_points",
                self.paragraph_indent_tolerance_points,
            ),
            ("column_min_gap_points", self.column_min_gap_points),
            (
                "column_cluster_tolerance_points",
                self.column_cluster_tolerance_points,
            ),
            (
                "column_ambiguity_span_ratio",
                self.column_ambiguity_span_ratio,
            ),
            (
                "heading_level_1_size_ratio",
                self.heading_level_1_size_ratio,
            ),
            (
                "heading_level_2_size_ratio",
                self.heading_level_2_size_ratio,
            ),
            (
                "list_indent_tolerance_points",
                self.list_indent_tolerance_points,
            ),
            (
                "table_alignment_tolerance_points",
                self.table_alignment_tolerance_points,
            ),
            (
                "table_row_y_tolerance_points",
                self.table_row_y_tolerance_points,
            ),
            ("table_max_row_gap_ratio", self.table_max_row_gap_ratio),
            (
                "rule_axis_tolerance_points",
                self.rule_axis_tolerance_points,
            ),
            ("chrome_edge_ratio", self.chrome_edge_ratio),
            (
                "chrome_position_tolerance_points",
                self.chrome_position_tolerance_points,
            ),
            ("link_intersection_ratio", self.link_intersection_ratio),
        ];
        for (name, value) in values {
            if !value.is_finite() || value < 0.0 {
                return Err(ConversionError::ConversionFailed {
                    message: format!(
                        "invalid PDF heuristic {name}: expected a finite nonnegative value"
                    ),
                });
            }
        }
        for (name, value) in [
            (
                "line_vertical_overlap_ratio",
                self.line_vertical_overlap_ratio,
            ),
            (
                "column_ambiguity_span_ratio",
                self.column_ambiguity_span_ratio,
            ),
            ("link_intersection_ratio", self.link_intersection_ratio),
        ] {
            if value <= 0.0 || value > 1.0 {
                return Err(ConversionError::ConversionFailed {
                    message: format!("invalid PDF heuristic {name}: expected a ratio in (0, 1]"),
                });
            }
        }
        if !(0.0..0.5).contains(&self.chrome_edge_ratio) {
            return Err(ConversionError::ConversionFailed {
                message: "invalid PDF heuristic chrome_edge_ratio: top and bottom ranges must not overlap"
                    .into(),
            });
        }
        if self.heading_level_2_size_ratio < 1.0
            || self.heading_level_1_size_ratio <= self.heading_level_2_size_ratio
        {
            return Err(ConversionError::ConversionFailed {
                message: "invalid PDF heading ratios: level 1 must exceed level 2 and both must be at least 1"
                    .into(),
            });
        }
        if !(1..=1_000).contains(&self.heading_bold_weight) {
            return Err(ConversionError::ConversionFailed {
                message: "invalid PDF heuristic heading_bold_weight: expected a weight from 1 through 1000"
                    .into(),
            });
        }
        for (name, value) in [
            (
                "column_min_lines_per_column",
                self.column_min_lines_per_column,
            ),
            ("table_min_rows", self.table_min_rows),
            ("table_min_columns", self.table_min_columns),
            ("chrome_min_pages", self.chrome_min_pages),
        ] {
            if value < 2 {
                return Err(ConversionError::ConversionFailed {
                    message: format!(
                        "invalid PDF heuristic {name}: expected a count of at least 2"
                    ),
                });
            }
        }
        if self.borderless_table_min_rows < 3 {
            return Err(ConversionError::ConversionFailed {
                message: "invalid PDF heuristic borderless_table_min_rows: expected a count of at least 3"
                    .into(),
            });
        }
        if self.borderless_table_min_rows.checked_add(1).is_none() {
            return Err(ConversionError::ConversionFailed {
                message: "invalid PDF heuristic borderless_table_min_rows: count exceeds the overflow-safe range"
                    .into(),
            });
        }
        for (name, value) in [
            ("table_min_rows", self.table_min_rows),
            ("table_min_columns", self.table_min_columns),
        ] {
            if value.checked_add(1).is_none() {
                return Err(ConversionError::ConversionFailed {
                    message: format!(
                        "invalid PDF heuristic {name}: count cannot accommodate a boundary"
                    ),
                });
            }
        }
        Ok(())
    }
}
