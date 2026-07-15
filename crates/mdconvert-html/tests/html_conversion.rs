use std::{fs, path::PathBuf};

use mdconvert_core::{
    Alignment, AssetId, Block, ConversionError, ConversionLimits, ConversionRequest, Converter,
    GfmOptions, Inline, WarningCode, emit_gfm,
};
use mdconvert_html::HtmlConverter;
use tempfile::TempDir;
use url::Url;

fn workspace_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn request_for(path: impl Into<PathBuf>) -> ConversionRequest {
    ConversionRequest::new(path).expect("fixture path should be valid")
}

fn convert_fixture(name: &str) -> mdconvert_core::Document {
    HtmlConverter
        .convert(&request_for(workspace_path(&format!(
            "tests/fixtures/html/{name}.html"
        ))))
        .expect("fixture should convert")
}

fn emitted(document: &mdconvert_core::Document) -> String {
    emit_gfm(
        document,
        &GfmOptions {
            final_newline: true,
        },
    )
    .expect("shared emitter should render converted document")
}

#[test]
fn semantic_fixture_maps_dom_structure_and_uses_shared_golden() {
    let document = convert_fixture("semantic");

    assert_eq!(
        document.metadata.title.as_deref(),
        Some("Semantic document")
    );
    assert_eq!(document.metadata.source_format.as_deref(), Some("html"));
    assert!(matches!(
        document.blocks.first(),
        Some(Block::Heading { level: 1, .. })
    ));
    assert!(document.blocks.iter().any(|block| matches!(
        block,
        Block::List { ordered: false, items, .. }
            if items.iter().any(|item| item.blocks.iter().any(|nested| matches!(
                nested,
                Block::List { ordered: true, start: Some(3), .. }
            )))
    )));
    assert!(document.blocks.iter().any(|block| matches!(
        block,
        Block::Table { alignments, rows }
            if alignments == &[Alignment::Left, Alignment::Right] && rows.len() == 3
    )));
    assert!(document.blocks.iter().any(|block| matches!(
        block,
        Block::Code { language: Some(language), text }
            if language == "rust" && text.contains("println!")
    )));
    assert!(
        document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Quote { .. }))
    );

    let markdown = emitted(&document);
    assert_eq!(
        markdown,
        fs::read_to_string(workspace_path("tests/golden/html/semantic.md"))
            .expect("semantic golden should exist")
    );
    for invisible in [
        "script text",
        "hidden attribute text",
        "aria hidden text",
        "display hidden text",
        "important hidden text",
        "visibility hidden text",
        "template text",
        "noscript text",
        "onclick",
    ] {
        assert!(
            !markdown.contains(invisible),
            "leaked invisible text: {invisible}"
        );
    }
    assert!(markdown.contains("[a guide](https://example.test/docs/guide/start)"));
    assert!(markdown.contains("Before image"));
    assert!(markdown.contains("Remote photo"));
    assert!(markdown.contains("after image"));
    assert!(document.warnings.iter().any(|warning| {
        warning.code == WarningCode::ExternalAssetSkipped && warning.page.is_none()
    }));
}

#[test]
fn malformed_fixture_recovers_and_skips_unsafe_link() {
    let document = convert_fixture("malformed");
    let markdown = emitted(&document);

    assert_eq!(
        markdown,
        fs::read_to_string(workspace_path("tests/golden/html/malformed.md"))
            .expect("malformed golden should exist")
    );
    assert!(markdown.contains("unsafe destination"));
    assert!(markdown.contains("unsafe data"));
    assert!(markdown.contains("obfuscated unsafe"));
    assert!(!markdown.contains("javascript:"));
    assert!(!markdown.contains("data:text"));
    assert!(!markdown.contains("java\nscript"));
    assert!(!markdown.contains("script must disappear"));
    assert!(document.warnings.iter().any(|warning| {
        warning.code == WarningCode::InvalidLinkSkipped && warning.page.is_none()
    }));
}

#[test]
fn request_source_url_resolves_relative_link_without_base() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source.html");
    fs::write(&source, "<p><a href='child'>Child</a></p>").unwrap();
    let mut request = request_for(source);
    request.source_url = Some(Url::parse("https://request.example/root/").unwrap());

    let document = HtmlConverter.convert(&request).unwrap();
    assert_eq!(
        document.blocks,
        vec![Block::Paragraph {
            content: vec![Inline::Link {
                url: "https://request.example/root/child".into(),
                title: None,
                content: vec![Inline::Text("Child".into())],
            }],
        }]
    );
}

#[test]
fn invalid_document_base_falls_back_to_request_source_url() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source.html");
    fs::write(
        &source,
        "<base href='data:text/plain,not-a-base'><p><a href='child'>Child</a></p>",
    )
    .unwrap();
    let mut request = request_for(source);
    request.source_url = Some(Url::parse("https://request.example/root/").unwrap());

    let document = HtmlConverter.convert(&request).unwrap();
    assert_eq!(
        emitted(&document),
        "[Child](https://request.example/root/child)\n"
    );
}

#[test]
fn bounded_data_and_in_root_local_images_become_deterministic_assets() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source.html");
    fs::write(temp.path().join("local.jpg"), [0xff, 0xd8, 0xff, 0xd9]).unwrap();
    fs::write(
        &source,
        "<p>before<img src='data:image/png;base64,iVBORw0KGgo=' alt='Data'>middle<img src='local.jpg' alt='Local'>after</p>",
    )
    .unwrap();

    let document = HtmlConverter.convert(&request_for(source)).unwrap();
    assert_eq!(document.assets.len(), 2);
    assert_eq!(
        document.assets[0].id,
        AssetId::new("html-image-001").unwrap()
    );
    assert_eq!(document.assets[0].file_name, "image-001.png");
    assert_eq!(document.assets[0].media_type, "image/png");
    assert_eq!(
        document.assets[1].id,
        AssetId::new("html-image-002").unwrap()
    );
    assert_eq!(document.assets[1].file_name, "image-002.jpg");
    assert_eq!(document.assets[1].data, [0xff, 0xd8, 0xff, 0xd9]);
    assert_eq!(
        document.blocks,
        vec![
            Block::Paragraph {
                content: vec![Inline::Text("before".into())]
            },
            Block::Image {
                asset_id: AssetId::new("html-image-001").unwrap(),
                alt: "Data".into()
            },
            Block::Paragraph {
                content: vec![Inline::Text("middle".into())]
            },
            Block::Image {
                asset_id: AssetId::new("html-image-002").unwrap(),
                alt: "Local".into()
            },
            Block::Paragraph {
                content: vec![Inline::Text("after".into())]
            },
        ]
    );
}

#[cfg(unix)]
#[test]
fn external_and_symlink_escape_images_preserve_alt_and_warn() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let source_dir = temp.path().join("input");
    fs::create_dir(&source_dir).unwrap();
    let outside = temp.path().join("outside.png");
    fs::write(&outside, b"outside").unwrap();
    symlink(&outside, source_dir.join("escape.png")).unwrap();
    let source = source_dir.join("source.html");
    fs::write(
        &source,
        "<img src='https://example.test/remote.png' alt='Remote'><img src='escape.png' alt='Escaped'><img src='https://example.test/empty.png'>",
    )
    .unwrap();

    let document = HtmlConverter.convert(&request_for(source)).unwrap();
    assert!(document.assets.is_empty());
    assert_eq!(
        document.blocks,
        vec![
            Block::Paragraph {
                content: vec![Inline::Text("Remote".into())]
            },
            Block::Paragraph {
                content: vec![Inline::Text("Escaped".into())]
            },
        ]
    );
    assert_eq!(
        document
            .warnings
            .iter()
            .filter(|warning| warning.code == WarningCode::ExternalAssetSkipped)
            .count(),
        3
    );
    assert!(
        document.warnings.iter().any(|warning| {
            warning.code == WarningCode::MissingImageAlt && warning.page.is_none()
        })
    );
}

#[test]
fn max_assets_and_total_decoded_image_bytes_are_enforced() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source.html");
    fs::write(
        &source,
        "<img src='data:image/png;base64,AQID' alt='One'><img src='data:image/png;base64,BAUG' alt='Two'>",
    )
    .unwrap();

    let mut asset_limited = request_for(&source);
    asset_limited.limits.max_assets = 1;
    assert!(matches!(
        HtmlConverter.convert(&asset_limited),
        Err(ConversionError::LimitExceeded {
            limit: "assets",
            actual: 2,
            maximum: 1
        })
    ));

    let mut byte_limited = request_for(source);
    byte_limited.limits.max_input_bytes = 5;
    assert!(matches!(
        HtmlConverter.convert(&byte_limited),
        Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            ..
        })
    ));

    let local_source = temp.path().join("local.html");
    fs::write(temp.path().join("large.png"), vec![0_u8; 128]).unwrap();
    fs::write(&local_source, "<img src='large.png' alt='Large'>").unwrap();
    let mut decoded_limited = request_for(local_source);
    decoded_limited.limits.max_input_bytes = 64;
    assert!(matches!(
        HtmlConverter.convert(&decoded_limited),
        Err(ConversionError::LimitExceeded {
            limit: "asset_bytes",
            actual: 128,
            maximum: 64
        })
    ));
}

#[test]
fn missing_directory_and_oversize_inputs_return_typed_errors() {
    let temp = TempDir::new().unwrap();
    assert!(matches!(
        HtmlConverter.convert(&request_for(temp.path().join("missing.html"))),
        Err(ConversionError::Io { .. })
    ));
    assert!(matches!(
        HtmlConverter.convert(&request_for(temp.path())),
        Err(ConversionError::CorruptInput { .. })
    ));

    let source = temp.path().join("large.html");
    fs::write(&source, "<p>larger than ten bytes</p>").unwrap();
    let mut request = request_for(source);
    request.limits = ConversionLimits {
        max_input_bytes: 10,
        ..ConversionLimits::default()
    };
    assert!(matches!(
        HtmlConverter.convert(&request),
        Err(ConversionError::LimitExceeded { limit: "input_bytes", actual, maximum: 10 }) if actual > 10
    ));

    let corrupt = temp.path().join("corrupt.html");
    fs::write(&corrupt, [0xff, 0xfe, 0xfd]).unwrap();
    assert!(matches!(
        HtmlConverter.convert(&request_for(corrupt)),
        Err(ConversionError::CorruptInput { .. })
    ));
}
