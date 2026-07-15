use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use mdconvert_core::{
    Block, ConversionError, ConversionRequest, Converter, DocumentMetadata, GfmOptions, Inline,
    WarningCode, emit_gfm,
};
use mdconvert_pdf::{
    HeuristicConfig, PdfConverter, RawDocument, RawGlyph, RawImage, RawLink, RawPage, RawRect,
    RawRule, RawWord, RuleKind, reconstruct, reconstruct_with_config,
};

fn workspace_path(relative: impl AsRef<Path>) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> RawRect {
    RawRect {
        left,
        top,
        right,
        bottom,
    }
}

fn glyph_line(text: &str, left: f32, top: f32, font_size: f32, weight: u16) -> Vec<RawGlyph> {
    text.chars()
        .enumerate()
        .map(|(index, character)| RawGlyph {
            text: character.to_string(),
            bounds: rect(
                left + index as f32 * 5.0,
                top,
                left + (index + 1) as f32 * 5.0,
                top + font_size,
            ),
            font_size,
            font_name: Some(if weight >= 700 { "Bold" } else { "Regular" }.into()),
            font_weight: Some(weight),
        })
        .collect()
}

fn page(number: u32, lines: &[(&str, f32, f32, f32, u16)]) -> RawPage {
    RawPage {
        number,
        width: 300.0,
        height: 400.0,
        rotation_degrees: 0,
        glyphs: lines
            .iter()
            .flat_map(|(text, left, top, size, weight)| {
                glyph_line(text, *left, *top, *size, *weight)
            })
            .collect(),
        words: Vec::new(),
        images: Vec::new(),
        links: Vec::new(),
        rules: Vec::new(),
    }
}

fn document(pages: Vec<RawPage>) -> RawDocument {
    RawDocument {
        metadata: DocumentMetadata {
            title: Some("Source title".into()),
            author: Some("Author".into()),
            source_format: Some("pdf".into()),
            page_count: Some(pages.len() as u32),
            properties: BTreeMap::from([("producer".into(), "fixture".into())]),
            ..DocumentMetadata::default()
        },
        pages,
    }
}

fn inline_text(content: &[Inline]) -> String {
    content
        .iter()
        .map(|inline| match inline {
            Inline::Text(text) => text.clone(),
            Inline::Link { content, .. } => inline_text(content),
            Inline::Emphasis(content) | Inline::Strong(content) => inline_text(content),
            Inline::Code(text) => text.clone(),
            Inline::LineBreak => "\n".into(),
        })
        .collect()
}

fn block_text(block: &Block) -> String {
    match block {
        Block::Heading { content, .. } | Block::Paragraph { content } => inline_text(content),
        Block::List { items, .. } => items
            .iter()
            .flat_map(|item| item.blocks.iter())
            .map(block_text)
            .collect::<Vec<_>>()
            .join(" "),
        Block::Table { rows, .. } => rows
            .iter()
            .flat_map(|row| row.iter())
            .map(|cell| inline_text(cell))
            .collect::<Vec<_>>()
            .join(" "),
        Block::Code { text, .. } => text.clone(),
        Block::Quote { blocks } => blocks.iter().map(block_text).collect::<Vec<_>>().join(" "),
        Block::Image { .. } | Block::ThematicBreak => String::new(),
    }
}

fn all_text(blocks: &[Block]) -> String {
    blocks.iter().map(block_text).collect::<Vec<_>>().join(" ")
}

#[test]
fn groups_glyphs_into_lines_and_joins_nearby_lines_into_a_paragraph() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("First line", 20.0, 50.0, 10.0, 400),
            ("second line", 20.0, 63.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert_eq!(
        output.blocks,
        vec![Block::Paragraph {
            content: vec![Inline::Text("First line second line".into())]
        }]
    );
}

#[test]
fn dehyphenates_only_alphabetic_line_end_before_lowercase_continuation() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("determin-", 20.0, 50.0, 10.0, 400),
            ("istic A-", 20.0, 63.0, 10.0, 400),
            ("Frame 9-", 20.0, 76.0, 10.0, 400),
            ("patch", 20.0, 89.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert_eq!(all_text(&output.blocks), "deterministic A- Frame 9- patch");
}

#[test]
fn orders_unambiguous_columns_top_to_bottom_then_left_to_right() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Left one", 20.0, 60.0, 10.0, 400),
            ("Right one", 180.0, 55.0, 10.0, 400),
            ("Left two", 20.0, 100.0, 10.0, 400),
            ("Right two", 180.0, 95.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert_eq!(
        output.blocks.iter().map(block_text).collect::<Vec<_>>(),
        ["Left one", "Left two", "Right one", "Right two"]
    );
    assert!(output.warnings.is_empty());
}

#[test]
fn same_baseline_fragments_stay_separate_when_source_object_order_is_right_to_left() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Right", 180.0, 50.0, 10.0, 400),
            ("Left", 20.0, 50.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert_eq!(
        output.blocks.iter().map(block_text).collect::<Vec<_>>(),
        ["Left", "Right"]
    );
}

#[test]
fn ambiguous_columns_preserve_every_text_once_and_warn() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Wide bridge across columns", 20.0, 40.0, 10.0, 400),
            ("Left", 20.0, 60.0, 10.0, 400),
            ("Right", 180.0, 60.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    let text = all_text(&output.blocks);
    for expected in ["Wide bridge across columns", "Left", "Right"] {
        assert_eq!(text.matches(expected).count(), 1);
    }
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::AmbiguousReadingOrder)
    );
}

#[test]
fn infers_heading_levels_from_named_size_and_weight_thresholds() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Primary", 20.0, 20.0, 20.0, 700),
            ("Secondary", 20.0, 60.0, 15.0, 700),
            ("Body", 20.0, 100.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert!(matches!(output.blocks[0], Block::Heading { level: 1, .. }));
    assert!(matches!(output.blocks[1], Block::Heading { level: 2, .. }));
    assert!(matches!(output.blocks[2], Block::Paragraph { .. }));
}

#[test]
fn heading_ratio_boundary_is_configurable() {
    let config = HeuristicConfig {
        heading_level_2_size_ratio: 1.6,
        ..HeuristicConfig::default()
    };
    let output = reconstruct_with_config(
        document(vec![page(
            1,
            &[
                ("Boundary", 20.0, 20.0, 15.0, 700),
                ("Body", 20.0, 80.0, 10.0, 400),
            ],
        )]),
        &config,
    )
    .unwrap();

    assert!(matches!(output.blocks[0], Block::Paragraph { .. }));
}

#[test]
fn infers_contiguous_unordered_and_ordered_lists() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("- Alpha", 20.0, 30.0, 10.0, 400),
            ("- Beta", 20.0, 45.0, 10.0, 400),
            ("3. Third", 20.0, 80.0, 10.0, 400),
            ("4. Fourth", 20.0, 95.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert!(matches!(
        output.blocks[0],
        Block::List { ordered: false, ref items, .. } if items.len() == 2
    ));
    assert!(matches!(
        output.blocks[1],
        Block::List { ordered: true, start: Some(3), ref items } if items.len() == 2
    ));
}

#[test]
fn requires_actual_rule_grid_for_bordered_table() {
    let mut source = page(
        1,
        &[
            ("Name", 20.0, 15.0, 10.0, 700),
            ("Value", 90.0, 15.0, 10.0, 700),
            ("Alpha", 20.0, 35.0, 10.0, 400),
            ("One", 90.0, 35.0, 10.0, 400),
        ],
    );
    for x in [10.0, 80.0, 150.0] {
        source.rules.push(RawRule {
            kind: RuleKind::Line,
            bounds: rect(x, 10.0, x, 50.0),
            stroke_width: 1.0,
        });
    }
    for y in [10.0, 30.0, 50.0] {
        source.rules.push(RawRule {
            kind: RuleKind::Line,
            bounds: rect(10.0, y, 150.0, y),
            stroke_width: 1.0,
        });
    }

    let output = reconstruct(document(vec![source])).unwrap();
    assert!(matches!(
        output.blocks.as_slice(),
        [Block::Table { rows, .. }] if rows.len() == 2 && rows[0].len() == 2
    ));
}

#[test]
fn table_keeps_its_geometric_position_after_preceding_text() {
    let mut source = page(
        1,
        &[
            ("Introduction", 20.0, 5.0, 10.0, 400),
            ("Name", 20.0, 55.0, 10.0, 700),
            ("Value", 90.0, 55.0, 10.0, 700),
            ("Alpha", 20.0, 75.0, 10.0, 400),
            ("One", 90.0, 75.0, 10.0, 400),
        ],
    );
    for x in [10.0, 80.0, 150.0] {
        source.rules.push(RawRule {
            kind: RuleKind::Line,
            bounds: rect(x, 50.0, x, 90.0),
            stroke_width: 1.0,
        });
    }
    for y in [50.0, 70.0, 90.0] {
        source.rules.push(RawRule {
            kind: RuleKind::Line,
            bounds: rect(10.0, y, 150.0, y),
            stroke_width: 1.0,
        });
    }

    let output = reconstruct(document(vec![source])).unwrap();
    assert!(matches!(output.blocks[0], Block::Paragraph { .. }));
    assert!(matches!(output.blocks[1], Block::Table { .. }));
}

#[test]
fn ambiguous_two_by_two_alignment_preserves_text_and_warns() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Key", 20.0, 20.0, 10.0, 400),
            ("Amount", 130.0, 20.0, 10.0, 400),
            ("Alpha", 20.0, 40.0, 10.0, 400),
            ("10", 130.0, 40.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert!(
        !output
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    for expected in ["Key", "Amount", "Alpha", "10"] {
        assert_eq!(all_text(&output.blocks).matches(expected).count(), 1);
    }
    assert!(output.warnings.iter().any(|warning| {
        warning.code == WarningCode::TableDegraded
            && warning.message
                == "aligned borderless layout lacked table-specific multi-row column-type consistency; preserved as text"
            && warning.page == Some(1)
    }));
}

#[test]
fn bold_per_column_headings_do_not_turn_compact_columns_into_a_table() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Left heading", 20.0, 20.0, 10.0, 700),
            ("Right heading", 180.0, 20.0, 10.0, 700),
            ("Left body", 20.0, 40.0, 10.0, 400),
            ("Right body", 180.0, 40.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert!(
        !output
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    assert_eq!(
        output.blocks.iter().map(block_text).collect::<Vec<_>>(),
        ["Left heading", "Left body", "Right heading", "Right body"]
    );
    assert!(output.warnings.iter().any(|warning| {
        warning.code == WarningCode::TableDegraded
            && warning.message
                == "aligned borderless layout lacked table-specific multi-row column-type consistency; preserved as text"
            && warning.page == Some(1)
    }));
}

#[test]
fn infers_borderless_table_from_repeated_heterogeneous_column_types() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Key", 20.0, 20.0, 10.0, 400),
            ("Amount", 130.0, 20.0, 10.0, 400),
            ("Alpha", 20.0, 40.0, 10.0, 400),
            ("10", 130.0, 40.0, 10.0, 400),
            ("Beta", 20.0, 60.0, 10.0, 400),
            ("20", 130.0, 60.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert!(matches!(output.blocks.as_slice(), [Block::Table { rows, .. }] if rows.len() == 3));
    assert!(
        output
            .warnings
            .iter()
            .all(|warning| warning.code != WarningCode::TableDegraded)
    );
}

#[test]
fn borderless_table_min_rows_defaults_to_three() {
    assert_eq!(HeuristicConfig::default().borderless_table_min_rows, 3);
}

#[test]
fn configured_borderless_row_boundary_is_deterministic() {
    let config = HeuristicConfig {
        borderless_table_min_rows: 4,
        ..HeuristicConfig::default()
    };
    let three_rows = reconstruct_with_config(
        document(vec![page(
            1,
            &[
                ("Key", 20.0, 20.0, 10.0, 400),
                ("Amount", 130.0, 20.0, 10.0, 400),
                ("Alpha", 20.0, 40.0, 10.0, 400),
                ("10", 130.0, 40.0, 10.0, 400),
                ("Beta", 20.0, 60.0, 10.0, 400),
                ("20", 130.0, 60.0, 10.0, 400),
            ],
        )]),
        &config,
    )
    .unwrap();
    assert!(
        !three_rows
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    assert!(
        three_rows
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::TableDegraded)
    );

    let four_rows = reconstruct_with_config(
        document(vec![page(
            1,
            &[
                ("Key", 20.0, 20.0, 10.0, 400),
                ("Amount", 130.0, 20.0, 10.0, 400),
                ("Alpha", 20.0, 40.0, 10.0, 400),
                ("10", 130.0, 40.0, 10.0, 400),
                ("Beta", 20.0, 60.0, 10.0, 400),
                ("20", 130.0, 60.0, 10.0, 400),
                ("Gamma", 20.0, 80.0, 10.0, 400),
                ("30", 130.0, 80.0, 10.0, 400),
            ],
        )]),
        &config,
    )
    .unwrap();
    assert!(matches!(four_rows.blocks.as_slice(), [Block::Table { rows, .. }] if rows.len() == 4));
}

#[test]
fn links_intersecting_table_cell_text_remain_inside_that_cell() {
    let mut source = page(
        1,
        &[
            ("Key", 20.0, 20.0, 10.0, 700),
            ("Amount", 130.0, 20.0, 10.0, 700),
            ("Alpha", 20.0, 40.0, 10.0, 400),
            ("10", 130.0, 40.0, 10.0, 400),
            ("Beta", 20.0, 60.0, 10.0, 400),
            ("20", 130.0, 60.0, 10.0, 400),
        ],
    );
    source.links.push(RawLink {
        bounds: rect(20.0, 39.0, 45.0, 51.0),
        target: "https://cell.test".into(),
    });

    let output = reconstruct(document(vec![source])).unwrap();
    let Block::Table { rows, .. } = &output.blocks[0] else {
        panic!("expected table");
    };
    assert!(matches!(rows[1][0][0], Inline::Link { .. }));
    assert_eq!(inline_text(&rows[1][0]), "Alpha");
}

#[test]
fn weak_table_alignment_degrades_to_text_with_warning() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Key", 20.0, 20.0, 10.0, 700),
            ("Amount", 130.0, 20.0, 10.0, 700),
            ("Alpha only", 20.0, 40.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert!(
        !output
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    assert!(all_text(&output.blocks).contains("Key Amount Alpha only"));
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::TableDegraded)
    );
}

#[test]
fn removes_only_repeated_normalized_chrome_at_matching_page_positions() {
    let output = reconstruct(document(vec![
        page(
            1,
            &[
                ("Quarterly Report", 20.0, 5.0, 9.0, 400),
                ("Body duplicate", 20.0, 100.0, 10.0, 400),
                ("Confidential", 20.0, 380.0, 9.0, 400),
            ],
        ),
        page(
            2,
            &[
                ("  quarterly   report ", 20.0, 6.0, 9.0, 400),
                ("Body duplicate", 20.0, 100.0, 10.0, 400),
                ("CONFIDENTIAL", 20.0, 379.0, 9.0, 400),
            ],
        ),
    ]))
    .unwrap();

    let text = all_text(&output.blocks).to_lowercase();
    assert!(!text.contains("quarterly report"));
    assert!(!text.contains("confidential"));
    assert_eq!(text.matches("body duplicate").count(), 2);
}

#[test]
fn repeated_chrome_text_is_not_removed_when_it_also_appears_in_the_body() {
    let output = reconstruct(document(vec![
        page(
            1,
            &[
                ("Report", 20.0, 5.0, 9.0, 400),
                ("Report", 20.0, 100.0, 10.0, 400),
            ],
        ),
        page(2, &[("report", 20.0, 6.0, 9.0, 400)]),
    ]))
    .unwrap();

    assert_eq!(all_text(&output.blocks), "Report");
}

#[test]
fn places_images_in_reading_order_and_encodes_checked_rgba_as_png() {
    let mut source = page(
        1,
        &[
            ("Before", 20.0, 20.0, 10.0, 400),
            ("After", 20.0, 100.0, 10.0, 400),
        ],
    );
    source.images.push(RawImage {
        index: 7,
        bounds: rect(20.0, 60.0, 40.0, 80.0),
        pixel_width: 1,
        pixel_height: 1,
        rgba: vec![255, 0, 0, 255],
    });

    let output = reconstruct(document(vec![source])).unwrap();
    assert!(matches!(output.blocks[1], Block::Image { .. }));
    assert_eq!(output.assets[0].id.as_str(), "pdf-image-001");
    assert_eq!(output.assets[0].file_name, "image-001.png");
    assert_eq!(output.assets[0].media_type, "image/png");
    assert_eq!(&output.assets[0].data[..8], b"\x89PNG\r\n\x1a\n");
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::MissingImageAlt)
    );
}

#[test]
fn invalid_image_dimensions_return_typed_conversion_error() {
    let mut source = page(1, &[("Text", 20.0, 20.0, 10.0, 400)]);
    source.images.push(RawImage {
        index: 1,
        bounds: rect(20.0, 60.0, 40.0, 80.0),
        pixel_width: u32::MAX,
        pixel_height: u32::MAX,
        rgba: vec![],
    });

    assert!(matches!(
        reconstruct(document(vec![source])),
        Err(ConversionError::ConversionFailed { .. })
    ));
}

#[test]
fn wraps_only_intersecting_text_for_safe_links_and_warns_for_unsafe_links() {
    let mut source = page(
        1,
        &[
            ("Safe link tail", 20.0, 20.0, 10.0, 400),
            ("Unsafe", 20.0, 50.0, 10.0, 400),
        ],
    );
    source.links.push(RawLink {
        bounds: rect(20.0, 19.0, 65.0, 31.0),
        target: "https://example.test".into(),
    });
    source.links.push(RawLink {
        bounds: rect(20.0, 49.0, 50.0, 61.0),
        target: "javascript:alert(1)".into(),
    });

    let output = reconstruct(document(vec![source])).unwrap();
    let Block::Paragraph { content } = &output.blocks[0] else {
        panic!("first block should be a paragraph");
    };
    assert!(matches!(
        content.as_slice(),
        [Inline::Link { url, content, .. }, Inline::Text(tail)]
            if url == "https://example.test" && inline_text(content) == "Safe link" && tail == " tail"
    ));
    assert!(all_text(&output.blocks).contains("Unsafe"));
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::InvalidLinkSkipped)
    );
}

#[test]
fn wraps_each_intersecting_range_when_a_line_contains_multiple_safe_links() {
    let mut source = page(1, &[("One and Two", 20.0, 20.0, 10.0, 400)]);
    source.links = vec![
        RawLink {
            bounds: rect(20.0, 19.0, 35.0, 31.0),
            target: "https://one.test".into(),
        },
        RawLink {
            bounds: rect(60.0, 19.0, 75.0, 31.0),
            target: "mailto:two@example.test".into(),
        },
    ];

    let output = reconstruct(document(vec![source])).unwrap();
    let Block::Paragraph { content } = &output.blocks[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(
        content
            .iter()
            .filter(|inline| matches!(inline, Inline::Link { .. }))
            .count(),
        2
    );
    assert_eq!(inline_text(content), "One and Two");
}

#[test]
fn paragraph_join_preserves_links_from_each_source_line() {
    let mut source = page(
        1,
        &[
            ("Linked first", 20.0, 20.0, 10.0, 400),
            ("plain second", 20.0, 33.0, 10.0, 400),
        ],
    );
    source.links.push(RawLink {
        bounds: rect(20.0, 19.0, 50.0, 31.0),
        target: "file:///tmp/source".into(),
    });

    let output = reconstruct(document(vec![source])).unwrap();
    let Block::Paragraph { content } = &output.blocks[0] else {
        panic!("expected paragraph");
    };
    assert!(matches!(content[0], Inline::Link { .. }));
    assert_eq!(inline_text(content), "Linked first plain second");
}

#[test]
fn list_bullets_are_unicode_safe_and_ordered_discontinuities_start_a_new_list() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("• Alpha", 20.0, 20.0, 10.0, 400),
            ("• Beta", 20.0, 35.0, 10.0, 400),
            ("1. One", 20.0, 70.0, 10.0, 400),
            ("3. Three", 20.0, 85.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert!(matches!(
        &output.blocks[0],
        Block::List { ordered: false, items, .. }
            if items.iter().map(|item| block_text(&item.blocks[0])).collect::<Vec<_>>() == ["Alpha", "Beta"]
    ));
    assert!(
        matches!(&output.blocks[1], Block::List { ordered: true, start: Some(1), items, .. } if items.len() == 1)
    );
    assert!(
        matches!(&output.blocks[2], Block::List { ordered: true, start: Some(3), items, .. } if items.len() == 1)
    );
}

#[test]
fn link_ranges_in_list_items_are_adjusted_after_removing_the_marker() {
    let mut source = page(1, &[("- Linked item", 20.0, 20.0, 10.0, 400)]);
    source.links.push(RawLink {
        bounds: rect(30.0, 19.0, 60.0, 31.0),
        target: "https://list.test".into(),
    });

    let output = reconstruct(document(vec![source])).unwrap();
    let Block::List { items, .. } = &output.blocks[0] else {
        panic!("expected list");
    };
    let Block::Paragraph { content } = &items[0].blocks[0] else {
        panic!("expected list paragraph");
    };
    assert!(matches!(content[0], Inline::Link { .. }));
    assert_eq!(inline_text(content), "Linked item");
}

#[test]
fn disconnected_rules_do_not_qualify_as_an_actual_table_grid() {
    let mut source = page(
        1,
        &[
            ("Name", 20.0, 15.0, 10.0, 700),
            ("Value", 90.0, 15.0, 10.0, 700),
            ("Alpha", 30.0, 35.0, 10.0, 400),
            ("One", 105.0, 35.0, 10.0, 400),
        ],
    );
    for x in [10.0, 80.0, 150.0] {
        source.rules.push(RawRule {
            kind: RuleKind::Line,
            bounds: rect(x, 200.0, x, 210.0),
            stroke_width: 1.0,
        });
    }
    for y in [10.0, 30.0, 50.0] {
        source.rules.push(RawRule {
            kind: RuleKind::Line,
            bounds: rect(200.0, y, 210.0, y),
            stroke_width: 1.0,
        });
    }

    let output = reconstruct(document(vec![source])).unwrap();
    assert!(
        !output
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    let text = all_text(&output.blocks);
    for expected in ["Name", "Value", "Alpha", "One"] {
        assert_eq!(text.matches(expected).count(), 1, "{text}");
    }
}

#[test]
fn preserves_source_metadata_and_page_count_without_artificial_page_headings() {
    let output = reconstruct(document(vec![
        page(1, &[("First", 20.0, 20.0, 10.0, 400)]),
        page(2, &[("Second", 20.0, 20.0, 10.0, 400)]),
    ]))
    .unwrap();

    assert_eq!(output.metadata.title.as_deref(), Some("Source title"));
    assert_eq!(output.metadata.page_count, Some(2));
    assert_eq!(
        output
            .metadata
            .properties
            .get("producer")
            .map(String::as_str),
        Some("fixture")
    );
    assert!(!all_text(&output.blocks).contains("Página"));
}

#[test]
fn rejects_non_finite_or_negative_thresholds_without_panicking() {
    for invalid in [f32::NAN, f32::INFINITY, -0.1] {
        let config = HeuristicConfig {
            line_y_tolerance_points: invalid,
            ..HeuristicConfig::default()
        };
        let error = reconstruct_with_config(document(vec![]), &config).unwrap_err();
        assert_eq!(error.code(), "conversion_failed");
    }
}

#[test]
fn pdf_converter_matches_shared_emitter_goldens_for_digital_layout_fixtures() {
    for fixture in [
        "digital-basic",
        "two-columns",
        "headings-lists",
        "table-bordered",
        "table-aligned",
        "repeated-chrome",
    ] {
        let request =
            ConversionRequest::new(workspace_path(format!("tests/fixtures/pdf/{fixture}.pdf")))
                .unwrap();
        let document = PdfConverter.convert(&request).unwrap();
        let markdown = emit_gfm(
            &document,
            &GfmOptions {
                final_newline: true,
            },
        )
        .unwrap();
        let golden =
            fs::read_to_string(workspace_path(format!("tests/golden/pdf/{fixture}.md"))).unwrap();
        assert_eq!(markdown, golden, "fixture {fixture}");
        assert!(!markdown.contains("## Página"), "fixture {fixture}");
    }
}

#[test]
fn fixture_models_cover_columns_headings_lists_tables_chrome_images_and_links() {
    let convert = |name: &str| {
        PdfConverter
            .convert(
                &ConversionRequest::new(workspace_path(format!("tests/fixtures/pdf/{name}.pdf")))
                    .unwrap(),
            )
            .unwrap()
    };

    let columns = convert("two-columns");
    assert_eq!(
        columns.blocks.iter().map(block_text).collect::<Vec<_>>(),
        ["Left one", "Left two", "Right one", "Right two"]
    );

    let headings_lists = convert("headings-lists");
    assert!(
        matches!(headings_lists.blocks[0], Block::Heading { level: 1, .. }),
        "{:?}",
        headings_lists.blocks
    );
    assert!(matches!(
        headings_lists.blocks[1],
        Block::Heading { level: 2, .. }
    ));
    assert!(
        headings_lists
            .blocks
            .iter()
            .any(|block| matches!(block, Block::List { ordered: false, .. }))
    );
    assert!(
        headings_lists
            .blocks
            .iter()
            .any(|block| matches!(block, Block::List { ordered: true, .. }))
    );

    assert!(
        matches!(convert("table-bordered").blocks.as_slice(), [Block::Table { rows, .. }] if rows.len() == 2)
    );
    let aligned = convert("table-aligned");
    assert!(
        !aligned
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    for expected in ["Key", "Amount", "Alpha", "10"] {
        assert_eq!(all_text(&aligned.blocks).matches(expected).count(), 1);
    }
    assert!(
        aligned
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::TableDegraded)
    );

    let chrome = convert("repeated-chrome");
    assert_eq!(all_text(&chrome.blocks), "First body Second body");

    let digital = convert("digital-basic");
    assert_eq!(digital.assets.len(), 1);
    assert!(
        matches!(digital.blocks[1], Block::Paragraph { ref content } if content.iter().any(|inline| matches!(inline, Inline::Link { .. })))
    );
    assert!(matches!(digital.blocks[2], Block::Image { .. }));
    assert!(
        digital
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::MissingImageAlt)
    );
}

#[test]
fn scanned_fixture_reaches_task_7_ocr_required_through_pdf_converter() {
    let error = PdfConverter
        .convert(&ConversionRequest::new(workspace_path("tests/fixtures/pdf/scanned.pdf")).unwrap())
        .unwrap_err();
    assert!(matches!(error, ConversionError::OcrRequired));
}

#[test]
fn aligned_two_column_prose_is_not_invented_as_a_borderless_table() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Left first", 20.0, 20.0, 10.0, 400),
            ("Right first", 180.0, 20.0, 10.0, 400),
            ("Left second", 20.0, 70.0, 10.0, 400),
            ("Right second", 180.0, 70.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert!(
        !output
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    assert_eq!(
        output.blocks.iter().map(block_text).collect::<Vec<_>>(),
        ["Left first", "Left second", "Right first", "Right second"]
    );
}

#[test]
fn normally_spaced_aligned_column_prose_is_not_a_borderless_table() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Left first", 20.0, 20.0, 10.0, 400),
            ("Right first", 180.0, 20.0, 10.0, 400),
            ("Left second", 20.0, 40.0, 10.0, 400),
            ("Right second", 180.0, 40.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert!(
        !output
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    assert_eq!(
        output.blocks.iter().map(block_text).collect::<Vec<_>>(),
        ["Left first", "Left second", "Right first", "Right second"]
    );
}

#[test]
fn incomplete_borderless_table_row_preserves_all_text_and_degrades() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Key", 20.0, 20.0, 10.0, 700),
            ("Amount", 130.0, 20.0, 10.0, 700),
            ("Alpha", 20.0, 40.0, 10.0, 400),
            ("10", 130.0, 40.0, 10.0, 400),
            ("Incomplete", 20.0, 60.0, 10.0, 400),
        ],
    )]))
    .unwrap();

    assert!(
        !output
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    let text = all_text(&output.blocks);
    for expected in ["Key", "Amount", "Alpha", "10", "Incomplete"] {
        assert_eq!(text.matches(expected).count(), 1, "{text}");
    }
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::TableDegraded)
    );
}

#[test]
fn glyphs_shuffled_within_one_segment_are_reconstructed_geometrically() {
    let mut source = page(1, &[]);
    source.glyphs = vec![
        RawGlyph {
            text: "C".into(),
            bounds: rect(30.0, 20.0, 35.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
        RawGlyph {
            text: "A".into(),
            bounds: rect(20.0, 20.0, 25.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
        RawGlyph {
            text: "B".into(),
            bounds: rect(25.0, 20.0, 30.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
    ];

    let output = reconstruct(document(vec![source])).unwrap();
    assert_eq!(all_text(&output.blocks), "ABC");
}

#[test]
fn populated_words_rebuild_shuffled_glyph_text_space_and_link_geometrically() {
    let mut source = page(1, &[]);
    source.glyphs = vec![
        RawGlyph {
            text: "C".into(),
            bounds: rect(30.0, 20.0, 35.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
        RawGlyph {
            text: "A".into(),
            bounds: rect(20.0, 20.0, 25.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
        RawGlyph {
            text: "B".into(),
            bounds: rect(25.0, 20.0, 30.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
        RawGlyph {
            text: " ".into(),
            bounds: rect(35.0, 20.0, 36.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
        RawGlyph {
            text: "E".into(),
            bounds: rect(41.0, 20.0, 46.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
        RawGlyph {
            text: "D".into(),
            bounds: rect(36.0, 20.0, 41.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
    ];
    source.words = vec![
        RawWord {
            text: "ED".into(),
            bounds: rect(36.0, 20.0, 46.0, 30.0),
            glyph_start: 4,
            glyph_end: 6,
        },
        RawWord {
            text: "CAB".into(),
            bounds: rect(20.0, 20.0, 35.0, 30.0),
            glyph_start: 0,
            glyph_end: 3,
        },
    ];
    source.links.push(RawLink {
        bounds: rect(25.0, 19.0, 30.0, 31.0),
        target: "https://b.test".into(),
    });

    let output = reconstruct(document(vec![source])).unwrap();
    assert_eq!(all_text(&output.blocks), "ABC DE");
    let Block::Paragraph { content } = &output.blocks[0] else {
        panic!("expected paragraph");
    };
    assert!(content.iter().any(|inline| {
        matches!(inline, Inline::Link { url, content, .. }
            if url == "https://b.test" && inline_text(content) == "B")
    }));
}

#[test]
fn explicit_narrow_space_is_preserved_below_inferred_word_gap_threshold() {
    let mut source = page(1, &[]);
    source.glyphs = vec![
        RawGlyph {
            text: "A".into(),
            bounds: rect(20.0, 20.0, 25.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
        RawGlyph {
            text: " ".into(),
            bounds: rect(25.0, 20.0, 26.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
        RawGlyph {
            text: "B".into(),
            bounds: rect(26.0, 20.0, 31.0, 30.0),
            font_size: 10.0,
            font_name: None,
            font_weight: Some(400),
        },
    ];

    let output = reconstruct(document(vec![source])).unwrap();
    assert_eq!(all_text(&output.blocks), "A B");
}

#[test]
fn images_share_the_detected_column_reading_order_with_text() {
    let mut source = page(
        1,
        &[
            ("Left above", 20.0, 20.0, 10.0, 400),
            ("Right above", 180.0, 20.0, 10.0, 400),
            ("Left below", 20.0, 100.0, 10.0, 400),
            ("Right below", 180.0, 100.0, 10.0, 400),
        ],
    );
    source.images = vec![
        RawImage {
            index: 1,
            bounds: rect(20.0, 60.0, 30.0, 70.0),
            pixel_width: 1,
            pixel_height: 1,
            rgba: vec![255, 0, 0, 255],
        },
        RawImage {
            index: 2,
            bounds: rect(180.0, 60.0, 190.0, 70.0),
            pixel_width: 1,
            pixel_height: 1,
            rgba: vec![0, 0, 255, 255],
        },
    ];

    let output = reconstruct(document(vec![source])).unwrap();
    assert_eq!(
        output
            .blocks
            .iter()
            .map(|block| match block {
                Block::Image { asset_id, .. } => asset_id.as_str().to_owned(),
                _ => block_text(block),
            })
            .collect::<Vec<_>>(),
        [
            "Left above",
            "pdf-image-001",
            "Left below",
            "Right above",
            "pdf-image-002",
            "Right below",
        ]
    );
}

#[test]
fn plausible_column_split_with_too_few_lines_warns_with_reason() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Left", 20.0, 20.0, 10.0, 400),
            ("Right", 180.0, 20.0, 10.0, 400),
        ],
    )]))
    .unwrap();
    assert!(output.warnings.iter().any(|warning| {
        warning.code == WarningCode::AmbiguousReadingOrder && warning.message.contains("too few")
    }));
}

#[test]
fn plausible_column_split_with_weak_edge_clusters_warns_with_reason() {
    let output = reconstruct(document(vec![page(
        1,
        &[
            ("Left one", 20.0, 20.0, 10.0, 400),
            ("Left two", 50.0, 60.0, 10.0, 400),
            ("Right one", 180.0, 25.0, 10.0, 400),
            ("Right two", 210.0, 65.0, 10.0, 400),
        ],
    )]))
    .unwrap();
    assert!(output.warnings.iter().any(|warning| {
        warning.code == WarningCode::AmbiguousReadingOrder && warning.message.contains("cluster")
    }));
}

#[test]
fn plausible_column_split_with_occupied_gutter_warns_with_reason() {
    let config = HeuristicConfig {
        column_ambiguity_span_ratio: 0.8,
        ..HeuristicConfig::default()
    };
    let output = reconstruct_with_config(
        document(vec![page(
            1,
            &[
                ("Left reaches the gutter", 20.0, 20.0, 10.0, 400),
                ("Left short", 20.0, 60.0, 10.0, 400),
                ("Right one", 180.0, 25.0, 10.0, 400),
                ("Right two", 180.0, 65.0, 10.0, 400),
            ],
        )]),
        &config,
    )
    .unwrap();
    assert!(output.warnings.iter().any(|warning| {
        warning.code == WarningCode::AmbiguousReadingOrder && warning.message.contains("gutter")
    }));
}

#[test]
fn semantic_config_domains_reject_invalid_unit_ratios_and_heading_order() {
    let invalid = [
        HeuristicConfig {
            chrome_edge_ratio: 2.0,
            ..HeuristicConfig::default()
        },
        HeuristicConfig {
            link_intersection_ratio: 1.1,
            ..HeuristicConfig::default()
        },
        HeuristicConfig {
            heading_level_1_size_ratio: 1.2,
            heading_level_2_size_ratio: 1.5,
            ..HeuristicConfig::default()
        },
        HeuristicConfig {
            heading_level_1_size_ratio: 1.5,
            heading_level_2_size_ratio: 1.5,
            ..HeuristicConfig::default()
        },
        HeuristicConfig {
            line_vertical_overlap_ratio: 1.01,
            ..HeuristicConfig::default()
        },
        HeuristicConfig {
            heading_bold_weight: 0,
            ..HeuristicConfig::default()
        },
        HeuristicConfig {
            table_min_rows: 1,
            ..HeuristicConfig::default()
        },
        HeuristicConfig {
            borderless_table_min_rows: 2,
            ..HeuristicConfig::default()
        },
    ];
    for config in invalid {
        assert!(matches!(
            reconstruct_with_config(document(vec![]), &config),
            Err(ConversionError::ConversionFailed { .. })
        ));
    }
}

#[test]
fn extreme_table_minimums_return_typed_errors_without_overflow() {
    for config in [
        HeuristicConfig {
            table_min_rows: usize::MAX,
            ..HeuristicConfig::default()
        },
        HeuristicConfig {
            table_min_columns: usize::MAX,
            ..HeuristicConfig::default()
        },
        HeuristicConfig {
            borderless_table_min_rows: usize::MAX,
            ..HeuristicConfig::default()
        },
    ] {
        assert!(matches!(
            reconstruct_with_config(document(vec![page(1, &[])]), &config),
            Err(ConversionError::ConversionFailed { .. })
        ));
    }
}

#[test]
fn text_straddling_an_internal_rule_degrades_without_duplication() {
    let mut source = page(
        1,
        &[
            ("A", 20.0, 15.0, 10.0, 700),
            ("Wide", 70.0, 15.0, 10.0, 700),
            ("B", 20.0, 35.0, 10.0, 400),
            ("Y", 90.0, 35.0, 10.0, 400),
        ],
    );
    for x in [10.0, 80.0, 150.0] {
        source.rules.push(RawRule {
            kind: RuleKind::Line,
            bounds: rect(x, 10.0, x, 50.0),
            stroke_width: 1.0,
        });
    }
    for y in [10.0, 30.0, 50.0] {
        source.rules.push(RawRule {
            kind: RuleKind::Line,
            bounds: rect(10.0, y, 150.0, y),
            stroke_width: 1.0,
        });
    }

    let output = reconstruct(document(vec![source])).unwrap();
    assert!(
        !output
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    let text = all_text(&output.blocks);
    for expected in ["A", "Wide", "B", "Y"] {
        assert_eq!(text.matches(expected).count(), 1, "{text}");
    }
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::TableDegraded)
    );
}

#[test]
fn text_outside_a_ruled_grid_does_not_create_a_false_boundary_ambiguity() {
    let mut source = page(
        1,
        &[
            ("A", 20.0, 15.0, 10.0, 700),
            ("X", 90.0, 15.0, 10.0, 700),
            ("B", 20.0, 35.0, 10.0, 400),
            ("Y", 90.0, 35.0, 10.0, 400),
            ("Outside straddles", 70.0, 70.0, 10.0, 400),
        ],
    );
    for x in [10.0, 80.0, 150.0] {
        source.rules.push(RawRule {
            kind: RuleKind::Line,
            bounds: rect(x, 10.0, x, 50.0),
            stroke_width: 1.0,
        });
    }
    for y in [10.0, 30.0, 50.0] {
        source.rules.push(RawRule {
            kind: RuleKind::Line,
            bounds: rect(10.0, y, 150.0, y),
            stroke_width: 1.0,
        });
    }

    let output = reconstruct(document(vec![source])).unwrap();
    assert!(
        output
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    assert_eq!(
        all_text(&output.blocks)
            .matches("Outside straddles")
            .count(),
        1
    );
    assert!(
        output
            .warnings
            .iter()
            .all(|warning| warning.code != WarningCode::TableDegraded)
    );
}
