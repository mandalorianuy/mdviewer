use std::{collections::BTreeMap, path::PathBuf};

use mdconvert_core::{
    Alignment, Asset, AssetId, Block, ConversionError, ConversionLimits, ConversionRequest,
    ConversionWarning, Converter, Document, DocumentMetadata, Inline, ListItem, ModelError,
    WarningCode,
};

#[test]
fn document_json_round_trip_preserves_the_complete_model() {
    let asset_id = AssetId::new("figure-1").expect("asset ID should be valid");
    let document = Document {
        metadata: DocumentMetadata {
            title: Some("Portable document".into()),
            author: Some("Ada".into()),
            subject: Some("Contract".into()),
            source_format: Some("pdf".into()),
            page_count: Some(3),
            properties: BTreeMap::from([("language".into(), "es-UY".into())]),
        },
        blocks: vec![
            Block::heading(2, vec![Inline::Text("Heading".into())])
                .expect("heading level should be valid"),
            Block::Paragraph {
                content: vec![
                    Inline::Text("Text".into()),
                    Inline::Emphasis(vec![Inline::Text("emphasis".into())]),
                    Inline::Strong(vec![Inline::Text("strong".into())]),
                    Inline::Code("code".into()),
                    Inline::Link {
                        url: "https://example.com".into(),
                        title: Some("Example".into()),
                        content: vec![Inline::Text("link".into())],
                    },
                    Inline::LineBreak,
                ],
            },
            Block::List {
                ordered: true,
                start: Some(3),
                items: vec![ListItem {
                    blocks: vec![Block::Paragraph {
                        content: vec![Inline::Text("item".into())],
                    }],
                }],
            },
            Block::Table {
                alignments: vec![
                    Alignment::None,
                    Alignment::Left,
                    Alignment::Center,
                    Alignment::Right,
                ],
                rows: vec![vec![vec![Inline::Text("cell".into())]]],
            },
            Block::Code {
                language: Some("rust".into()),
                text: "fn main() {}".into(),
            },
            Block::Quote {
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text("quote".into())],
                }],
            },
            Block::Image {
                asset_id: asset_id.clone(),
                alt: "A diagram".into(),
            },
            Block::ThematicBreak,
        ],
        assets: vec![Asset {
            id: asset_id,
            file_name: "figure.png".into(),
            media_type: "image/png".into(),
            data: vec![137, 80, 78, 71],
        }],
        warnings: vec![ConversionWarning {
            code: WarningCode::MissingImageAlt,
            message: "Image alternative text was synthesized".into(),
            page: Some(1),
        }],
    };

    let json = serde_json::to_string(&document).expect("document should serialize");
    let decoded: Document = serde_json::from_str(&json).expect("document should deserialize");

    assert_eq!(decoded, document);
}

#[test]
fn block_and_inline_use_exact_adjacent_tag_json() {
    let block = Block::Paragraph {
        content: vec![Inline::Link {
            url: "https://example.com".into(),
            title: None,
            content: vec![Inline::Text("Example".into())],
        }],
    };

    assert_eq!(
        serde_json::to_value(block).expect("block should serialize"),
        serde_json::json!({
            "type": "paragraph",
            "value": {
                "content": [{
                    "type": "link",
                    "value": {
                        "url": "https://example.com",
                        "title": null,
                        "content": [{"type": "text", "value": "Example"}]
                    }
                }]
            }
        })
    );
}

#[test]
fn remaining_block_variants_use_exact_adjacent_tag_json() {
    let asset_id = AssetId::new("figure-1").expect("asset ID should be valid");
    let cases = [
        (
            Block::heading(1, vec![Inline::Text("Heading".into())])
                .expect("heading level should be valid"),
            serde_json::json!({
                "type": "heading",
                "value": {
                    "level": 1,
                    "content": [{"type": "text", "value": "Heading"}]
                }
            }),
        ),
        (
            Block::List {
                ordered: true,
                start: Some(4),
                items: vec![ListItem {
                    blocks: vec![Block::Paragraph {
                        content: vec![Inline::Text("Item".into())],
                    }],
                }],
            },
            serde_json::json!({
                "type": "list",
                "value": {
                    "ordered": true,
                    "start": 4,
                    "items": [{
                        "blocks": [{
                            "type": "paragraph",
                            "value": {
                                "content": [{"type": "text", "value": "Item"}]
                            }
                        }]
                    }]
                }
            }),
        ),
        (
            Block::Table {
                alignments: vec![
                    Alignment::None,
                    Alignment::Left,
                    Alignment::Center,
                    Alignment::Right,
                ],
                rows: vec![vec![vec![Inline::Code("cell".into())]]],
            },
            serde_json::json!({
                "type": "table",
                "value": {
                    "alignments": ["none", "left", "center", "right"],
                    "rows": [[[{"type": "code", "value": "cell"}]]]
                }
            }),
        ),
        (
            Block::Code {
                language: Some("rust".into()),
                text: "fn main() {}".into(),
            },
            serde_json::json!({
                "type": "code",
                "value": {"language": "rust", "text": "fn main() {}"}
            }),
        ),
        (
            Block::Quote {
                blocks: vec![Block::ThematicBreak],
            },
            serde_json::json!({
                "type": "quote",
                "value": {"blocks": [{"type": "thematic_break"}]}
            }),
        ),
        (
            Block::Image {
                asset_id,
                alt: "Diagram".into(),
            },
            serde_json::json!({
                "type": "image",
                "value": {"asset_id": "figure-1", "alt": "Diagram"}
            }),
        ),
        (
            Block::ThematicBreak,
            serde_json::json!({"type": "thematic_break"}),
        ),
    ];

    for (block, expected) in cases {
        assert_eq!(
            serde_json::to_value(block).expect("block should serialize"),
            expected
        );
    }
}

#[test]
fn remaining_inline_variants_use_exact_adjacent_tag_json() {
    let cases = [
        (
            Inline::Emphasis(vec![Inline::Text("emphasis".into())]),
            serde_json::json!({
                "type": "emphasis",
                "value": [{"type": "text", "value": "emphasis"}]
            }),
        ),
        (
            Inline::Strong(vec![Inline::Text("strong".into())]),
            serde_json::json!({
                "type": "strong",
                "value": [{"type": "text", "value": "strong"}]
            }),
        ),
        (
            Inline::Code("code".into()),
            serde_json::json!({"type": "code", "value": "code"}),
        ),
        (Inline::LineBreak, serde_json::json!({"type": "line_break"})),
    ];

    for (inline, expected) in cases {
        assert_eq!(
            serde_json::to_value(inline).expect("inline should serialize"),
            expected
        );
    }
}

#[test]
fn every_alignment_uses_its_exact_snake_case_json_value() {
    assert_eq!(
        serde_json::to_value([
            Alignment::None,
            Alignment::Left,
            Alignment::Center,
            Alignment::Right,
        ])
        .expect("alignments should serialize"),
        serde_json::json!(["none", "left", "center", "right"])
    );
}

#[test]
fn heading_levels_are_restricted_to_one_through_six() {
    for level in 1..=6 {
        assert!(Block::heading(level, vec![Inline::Text("Title".into())]).is_ok());
    }

    assert!(matches!(
        Block::heading(0, vec![]),
        Err(ModelError::InvalidHeadingLevel(0))
    ));
    assert!(matches!(
        Block::heading(7, vec![Inline::Text("Título".into())]),
        Err(ModelError::InvalidHeadingLevel(7))
    ));
}

#[test]
fn asset_ids_reject_empty_or_whitespace_only_values_and_preserve_valid_input() {
    assert!(matches!(AssetId::new(""), Err(ModelError::EmptyAssetId)));
    assert!(matches!(
        AssetId::new(" \t\n"),
        Err(ModelError::EmptyAssetId)
    ));

    let id = AssetId::new("  figure-1  ").expect("trim-nonempty ID should be valid");
    assert_eq!(id.as_str(), "  figure-1  ");
    assert_eq!(
        serde_json::to_value(id).expect("asset ID should serialize transparently"),
        serde_json::json!("  figure-1  ")
    );
}

#[test]
fn asset_id_deserialization_rejects_empty_and_whitespace_only_strings() {
    for json in [r#""""#, r#"" \t\n""#] {
        assert!(
            serde_json::from_str::<AssetId>(json).is_err(),
            "invalid asset ID JSON should be rejected: {json}"
        );
    }
}

#[test]
fn heading_deserialization_rejects_levels_outside_one_through_six() {
    for level in [0, 7] {
        let json = serde_json::json!({
            "type": "heading",
            "value": {"level": level, "content": []}
        });

        assert!(
            serde_json::from_value::<Block>(json).is_err(),
            "invalid heading level should be rejected: {level}"
        );
    }
}

#[test]
fn warning_codes_have_stable_snake_case_values() {
    let cases = [
        (
            WarningCode::AmbiguousReadingOrder,
            "ambiguous_reading_order",
        ),
        (WarningCode::TableDegraded, "table_degraded"),
        (
            WarningCode::FontMetadataInsufficient,
            "font_metadata_insufficient",
        ),
        (WarningCode::MissingImageAlt, "missing_image_alt"),
        (WarningCode::InvalidLinkSkipped, "invalid_link_skipped"),
        (WarningCode::InvalidAssetSkipped, "invalid_asset_skipped"),
        (WarningCode::ExternalAssetSkipped, "external_asset_skipped"),
        (WarningCode::OcrDeferred, "ocr_deferred"),
    ];

    for (code, expected) in cases {
        assert_eq!(
            serde_json::to_value(code).expect("warning code should serialize"),
            serde_json::json!(expected)
        );
    }
}

#[test]
fn conversion_limits_use_the_portable_defaults() {
    assert_eq!(
        ConversionLimits::default(),
        ConversionLimits {
            max_input_bytes: 500 * 1024 * 1024,
            max_pages: 2_000,
            max_assets: 10_000,
        }
    );
}

#[test]
fn conversion_requests_reject_only_empty_source_paths() {
    assert!(matches!(
        ConversionRequest::new(PathBuf::new()),
        Err(ModelError::EmptySourcePath)
    ));

    let request = ConversionRequest::new("input.pdf").expect("source should be valid");
    assert_eq!(request.source, PathBuf::from("input.pdf"));
    assert_eq!(request.source_url, None);
    assert_eq!(request.limits, ConversionLimits::default());
}

#[test]
fn conversion_request_deserialization_rejects_an_empty_source_path() {
    let json = serde_json::json!({
        "source": "",
        "source_url": null,
        "limits": {
            "max_input_bytes": 500 * 1024 * 1024,
            "max_pages": 2_000,
            "max_assets": 10_000
        }
    });

    assert!(serde_json::from_value::<ConversionRequest>(json).is_err());
}

#[test]
fn conversion_errors_expose_stable_codes() {
    let errors = [
        (
            ConversionError::from(ModelError::EmptySourcePath),
            "invalid_request",
        ),
        (
            ConversionError::Io {
                path: PathBuf::from("input.pdf"),
                source: std::io::Error::other("read failed"),
            },
            "io",
        ),
        (
            ConversionError::UnsupportedFormat {
                format: "doc".into(),
            },
            "unsupported_format",
        ),
        (
            ConversionError::CorruptInput {
                message: "invalid header".into(),
            },
            "corrupt_input",
        ),
        (ConversionError::EncryptedInput, "encrypted_input"),
        (
            ConversionError::LimitExceeded {
                limit: "max_pages",
                actual: 2_001,
                maximum: 2_000,
            },
            "limit_exceeded",
        ),
        (ConversionError::OcrRequired, "ocr_required"),
        (
            ConversionError::ConversionFailed {
                message: "parser stopped".into(),
            },
            "conversion_failed",
        ),
    ];

    for (error, expected) in errors {
        assert_eq!(error.code(), expected);
    }
}

#[test]
fn conversion_errors_preserve_invalid_request_and_io_sources() {
    let invalid_request = ConversionError::from(ModelError::EmptySourcePath);
    assert!(std::error::Error::source(&invalid_request).is_some());

    let io = ConversionError::Io {
        path: PathBuf::from("input.pdf"),
        source: std::io::Error::other("read failed"),
    };
    assert!(std::error::Error::source(&io).is_some());
}

#[test]
fn converter_contract_accepts_a_borrowed_request_and_returns_a_document() {
    struct EmptyConverter;

    impl Converter for EmptyConverter {
        fn convert(&self, _request: &ConversionRequest) -> Result<Document, ConversionError> {
            Ok(Document {
                metadata: DocumentMetadata::default(),
                blocks: vec![],
                assets: vec![],
                warnings: vec![],
            })
        }
    }

    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EmptyConverter>();

    let request = ConversionRequest::new("input.pdf").expect("source should be valid");
    let document = EmptyConverter
        .convert(&request)
        .expect("converter should succeed");
    assert_eq!(document.metadata, DocumentMetadata::default());
}
