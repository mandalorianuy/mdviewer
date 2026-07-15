use std::collections::BTreeMap;

use mdconvert_core::{
    Alignment, Asset, AssetId, Block, Document, DocumentMetadata, EmitError, GfmOptions, Inline,
    ListItem, emit_gfm,
};

fn empty_document(blocks: Vec<Block>) -> Document {
    Document {
        metadata: DocumentMetadata::default(),
        blocks,
        assets: vec![],
        warnings: vec![],
    }
}

fn emit(document: &Document) -> String {
    emit_gfm(
        document,
        &GfmOptions {
            final_newline: true,
        },
    )
    .expect("document should emit")
}

fn text(value: &str) -> Vec<Inline> {
    vec![Inline::Text(value.into())]
}

#[test]
fn emits_every_variant_as_the_checked_in_golden_file() {
    let asset_id = AssetId::new("figure-1").expect("asset ID should be valid");
    let document = Document {
        metadata: DocumentMetadata {
            title: Some("metadata must not become frontmatter".into()),
            properties: BTreeMap::from([("source".into(), "fixture".into())]),
            ..DocumentMetadata::default()
        },
        blocks: vec![
            Block::heading(1, text("Título & café")).expect("heading should be valid"),
            Block::Paragraph {
                content: vec![
                    Inline::Text("Plain *text* ".into()),
                    Inline::Emphasis(text("emphasis")),
                    Inline::Text(" ".into()),
                    Inline::Strong(text("strong")),
                    Inline::Text(" ".into()),
                    Inline::Code("code ` tick".into()),
                    Inline::Text(" ".into()),
                    Inline::Link {
                        url: "https://example.test/a(b)".into(),
                        title: Some("A \"title\"".into()),
                        content: text("link"),
                    },
                    Inline::LineBreak,
                    Inline::Text("next".into()),
                ],
            },
            Block::List {
                ordered: true,
                start: Some(3),
                items: vec![ListItem {
                    blocks: vec![
                        Block::Paragraph {
                            content: text("ordered start"),
                        },
                        Block::List {
                            ordered: false,
                            start: None,
                            items: vec![ListItem {
                                blocks: vec![Block::Paragraph {
                                    content: text("nested item"),
                                }],
                            }],
                        },
                    ],
                }],
            },
            Block::Table {
                alignments: vec![Alignment::None, Alignment::Center],
                rows: vec![
                    vec![text("Name"), text("Value")],
                    vec![text("a|b\r\nc"), text("UTF-8 ñ")],
                ],
            },
            Block::Code {
                language: Some("rust".into()),
                text: "let marker = \"```\";\r\n".into(),
            },
            Block::Quote {
                blocks: vec![
                    Block::Paragraph {
                        content: text("quoted"),
                    },
                    Block::ThematicBreak,
                ],
            },
            Block::Image {
                asset_id: asset_id.clone(),
                alt: "Alt [image]".into(),
            },
            Block::ThematicBreak,
        ],
        assets: vec![Asset {
            id: asset_id,
            file_name: "figure.png".into(),
            media_type: "image/png".into(),
            data: vec![],
        }],
        warnings: vec![],
    };

    let markdown = emit(&document);

    assert_eq!(
        markdown,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/golden/core/all-blocks.md"
        ))
    );
    assert!(!markdown.as_bytes().starts_with(b"---"));
    assert!(!markdown.contains('\r'));
    assert!(std::str::from_utf8(markdown.as_bytes()).is_ok());
}

#[test]
fn final_newline_is_controlled_only_by_the_option() {
    let document = empty_document(vec![Block::Paragraph {
        content: text("line\r\nending\rnormalized"),
    }]);

    let with_newline = emit(&document);
    let without_newline = emit_gfm(
        &document,
        &GfmOptions {
            final_newline: false,
        },
    )
    .expect("document should emit");

    assert_eq!(with_newline, "line\nending\nnormalized\n");
    assert_eq!(without_newline, "line\nending\nnormalized");
}

#[test]
fn escapes_table_cells_without_breaking_gfm_and_pads_short_rows() {
    let document = empty_document(vec![Block::Table {
        alignments: vec![],
        rows: vec![
            vec![text("a|b\nc"), text("header 2")],
            vec![text("only one")],
        ],
    }]);

    assert_eq!(
        emit(&document),
        "| a\\|b<br>c | header 2 |\n| --- | --- |\n| only one |  |\n"
    );
}

#[test]
fn rejects_a_nonempty_alignment_vector_with_the_wrong_width() {
    let document = empty_document(vec![Block::Table {
        alignments: vec![Alignment::Left],
        rows: vec![vec![text("one"), text("two")]],
    }]);

    assert!(matches!(
        emit_gfm(
            &document,
            &GfmOptions {
                final_newline: true
            }
        ),
        Err(EmitError::TableAlignmentWidthMismatch {
            expected: 2,
            actual: 1
        })
    ));
}

#[test]
fn chooses_code_delimiters_longer_than_contained_backtick_runs() {
    let document = empty_document(vec![
        Block::Paragraph {
            content: vec![Inline::Code("`edge` and ``middle``".into())],
        },
        Block::Code {
            language: None,
            text: "before ```` after".into(),
        },
    ]);

    assert_eq!(
        emit(&document),
        "``` `edge` and ``middle`` ```\n\n`````\nbefore ```` after\n`````\n"
    );
}

#[test]
fn escapes_text_links_and_image_destinations_by_context() {
    let asset_id = AssetId::new("image").expect("asset ID should be valid");
    let mut document = empty_document(vec![
        Block::Paragraph {
            content: vec![Inline::Link {
                url: r"https://example.test/a(b)\c".into(),
                title: Some("quoted \"title\" \\ path".into()),
                content: text("[label] *literal*"),
            }],
        },
        Block::Image {
            asset_id: asset_id.clone(),
            alt: "[alt] *literal*".into(),
        },
    ]);
    document.assets.push(Asset {
        id: asset_id,
        file_name: "asset (1)\\draft.png".into(),
        media_type: "image/png".into(),
        data: vec![],
    });

    assert_eq!(
        emit(&document),
        "[\\[label\\] \\*literal\\*](https://example.test/a\\(b\\)\\\\c \"quoted \\\"title\\\" \\\\ path\")\n\n![\\[alt\\] \\*literal\\*](asset%20\\(1\\)\\\\draft.png)\n"
    );
}

#[test]
fn percent_encodes_unsafe_destination_bytes_without_reencoding_percent_sequences() {
    let asset_id = AssetId::new("unsafe-destination").expect("asset ID should be valid");
    let mut document = empty_document(vec![
        Block::Paragraph {
            content: vec![Inline::Link {
                url: "https://example.test/already%20ok space\tline\r\nnext(1)\\end\u{1}\u{7f}"
                    .into(),
                title: None,
                content: text("link"),
            }],
        },
        Block::Image {
            asset_id: asset_id.clone(),
            alt: "image".into(),
        },
    ]);
    document.assets.push(Asset {
        id: asset_id,
        file_name: "asset name\trow\r\n(1)\\draft\0.png".into(),
        media_type: "image/png".into(),
        data: vec![],
    });

    assert_eq!(
        emit(&document),
        "[link](https://example.test/already%20ok%20space%09line%0D%0Anext\\(1\\)\\\\end%01%7F)\n\n![image](asset%20name%09row%0D%0A\\(1\\)\\\\draft%00.png)\n"
    );
}

#[test]
fn escapes_plain_text_that_would_be_reinterpreted_as_block_markup() {
    let document = empty_document(vec![Block::Paragraph {
        content: text("# heading\n- bullet\n+ bullet\n1. ordered\n> quote"),
    }]);

    assert_eq!(
        emit(&document),
        "\\# heading\n\\- bullet\n\\+ bullet\n1\\. ordered\n\\> quote\n"
    );
}

#[test]
fn preserves_literal_gfm_punctuation_entities_and_indented_markers() {
    let document = empty_document(vec![Block::Paragraph {
        content: text("~~text~~\n&copy;\n  - item\n   + item\n1) item\n  12. item\n    13. code"),
    }]);

    assert_eq!(
        emit(&document),
        "\\~\\~text\\~\\~\n&amp;copy;\n  \\- item\n   \\+ item\n1\\) item\n  12\\. item\n    13. code\n"
    );
}

#[test]
fn escapes_literal_hyphen_rules_after_zero_through_three_leading_spaces() {
    let document = empty_document(vec![Block::Paragraph {
        content: text("---\n---\n ---\n  ---\n   ---\n    ---"),
    }]);

    assert_eq!(
        emit(&document),
        "\\---\n\\---\n \\---\n  \\---\n   \\---\n    ---\n"
    );
}

#[test]
fn escapes_literal_setext_rules_after_zero_through_three_leading_spaces() {
    let document = empty_document(vec![Block::Paragraph {
        content: text("paragraph\n===\n ===\n  ===\n   ===\n    ==="),
    }]);

    assert_eq!(
        emit(&document),
        "paragraph\n\\===\n \\===\n  \\===\n   \\===\n    ===\n"
    );
}

#[test]
fn uses_a_long_enough_tilde_fence_when_the_language_contains_a_backtick() {
    let document = empty_document(vec![Block::Code {
        language: Some("rust`edition".into()),
        text: "before ~~~~ after".into(),
    }]);

    assert_eq!(
        emit(&document),
        "~~~~~rust`edition\nbefore ~~~~ after\n~~~~~\n"
    );
}

#[test]
fn rejects_code_languages_containing_raw_line_endings() {
    for language in ["rust\r2024", "rust\n2024"] {
        let document = empty_document(vec![Block::Code {
            language: Some(language.into()),
            text: "fn main() {}".into(),
        }]);

        assert!(matches!(
            emit_gfm(
                &document,
                &GfmOptions {
                    final_newline: true
                }
            ),
            Err(EmitError::InvalidCodeLanguage)
        ));
    }
}

#[test]
fn emits_an_empty_inline_code_value_as_an_html_code_element() {
    let document = empty_document(vec![Block::Paragraph {
        content: vec![Inline::Code(String::new())],
    }]);

    assert_eq!(emit(&document), "<code></code>\n");
}

#[test]
fn missing_and_duplicate_asset_ids_are_typed_errors() {
    let id = AssetId::new("figure").expect("asset ID should be valid");
    let image = Block::Image {
        asset_id: id.clone(),
        alt: "figure".into(),
    };
    let missing = empty_document(vec![image.clone()]);
    assert!(matches!(
        emit_gfm(
            &missing,
            &GfmOptions {
                final_newline: true
            }
        ),
        Err(EmitError::MissingAsset { asset_id }) if asset_id == "figure"
    ));

    let asset = Asset {
        id,
        file_name: "figure.png".into(),
        media_type: "image/png".into(),
        data: vec![],
    };
    let mut duplicate = empty_document(vec![image]);
    duplicate.assets = vec![asset.clone(), asset];
    assert!(matches!(
        emit_gfm(
            &duplicate,
            &GfmOptions {
                final_newline: true
            }
        ),
        Err(EmitError::DuplicateAssetId { asset_id }) if asset_id == "figure"
    ));
}
