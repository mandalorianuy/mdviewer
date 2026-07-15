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

fn convert_html(temp: &TempDir, html: &str) -> mdconvert_core::Document {
    let source = temp.path().join("source.html");
    fs::write(&source, html).unwrap();
    HtmlConverter.convert(&request_for(source)).unwrap()
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
fn table_and_code_extraction_exclude_invisible_descendants() {
    let temp = TempDir::new().unwrap();
    let document = convert_html(
        &temp,
        "<table><tr><td>visible</td><td hidden>hidden cell</td></tr><tr hidden><td>hidden row</td></tr><tr><td><span hidden>hidden span</span>last</td></tr></table><pre>before<script>script in pre</script><span hidden>hidden in pre</span>after</pre><p><code>one<script>script in code</script><span hidden>hidden in code</span>two</code></p>",
    );

    assert_eq!(
        document.blocks,
        vec![
            Block::Table {
                alignments: vec![Alignment::None],
                rows: vec![
                    vec![vec![Inline::Text("visible".into())]],
                    vec![vec![Inline::Text("last".into())]],
                ],
            },
            Block::Code {
                language: None,
                text: "beforeafter".into(),
            },
            Block::Paragraph {
                content: vec![Inline::Code("onetwo".into())],
            },
        ]
    );
}

#[test]
fn hidden_descendant_image_does_not_break_strong_inline_flow() {
    let temp = TempDir::new().unwrap();
    let document = convert_html(
        &temp,
        "<p><strong>A<span hidden><img src='x'></span>B</strong></p>",
    );

    assert_eq!(document.assets, vec![]);
    assert_eq!(document.warnings, vec![]);
    assert_eq!(
        document.blocks,
        vec![Block::Paragraph {
            content: vec![Inline::Strong(vec![
                Inline::Text("A".into()),
                Inline::Text("B".into()),
            ])],
        }]
    );
    assert_eq!(emitted(&document), "**AB**\n");
}

#[test]
fn aria_hidden_descendant_image_does_not_break_link_inline_flow() {
    let temp = TempDir::new().unwrap();
    let document = convert_html(
        &temp,
        "<p><a href='/x'>A<span aria-hidden='true'><img src='x'></span>B</a></p>",
    );

    assert_eq!(document.assets, vec![]);
    assert_eq!(document.warnings, vec![]);
    assert_eq!(
        document.blocks,
        vec![Block::Paragraph {
            content: vec![Inline::Link {
                url: "/x".into(),
                title: None,
                content: vec![Inline::Text("A".into()), Inline::Text("B".into())],
            }],
        }]
    );
    assert_eq!(emitted(&document), "[AB](/x)\n");
}

#[test]
fn style_hidden_descendant_image_does_not_break_emphasis_inline_flow() {
    let temp = TempDir::new().unwrap();
    let document = convert_html(
        &temp,
        "<p><em>A<span style='visibility: hidden'><img src='x'></span>B</em></p>",
    );

    assert_eq!(document.assets, vec![]);
    assert_eq!(document.warnings, vec![]);
    assert_eq!(emitted(&document), "*AB*\n");
    assert!(matches!(
        document.blocks.as_slice(),
        [Block::Paragraph { content }]
            if matches!(content.as_slice(), [Inline::Emphasis(_)])
    ));
}

#[test]
fn excessive_dom_depth_returns_an_exact_typed_limit() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("deep.html");
    let html = format!("{}text{}", "<div>".repeat(300), "</div>".repeat(300));
    fs::write(&source, html).unwrap();

    assert!(matches!(
        HtmlConverter.convert(&request_for(source)),
        Err(ConversionError::LimitExceeded {
            limit: "html_dom_depth",
            actual: 257,
            maximum: 256,
        })
    ));
}

#[test]
fn effective_base_uses_first_nonempty_href_and_resolves_relative_to_request() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source.html");
    fs::write(
        &source,
        "<base><base href='../assets/'><base href='https://ignored.test/'><p><a href='guide'>Guide</a></p>",
    )
    .unwrap();
    let mut request = request_for(source);
    request.source_url = Some(Url::parse("https://request.test/docs/page/").unwrap());

    let document = HtmlConverter.convert(&request).unwrap();
    assert_eq!(
        emitted(&document),
        "[Guide](https://request.test/docs/assets/guide)\n"
    );
}

#[test]
fn effective_base_controls_external_and_file_based_images() {
    let external = TempDir::new().unwrap();
    fs::write(external.path().join("same.png"), b"must not load").unwrap();
    let external_document = convert_html(
        &external,
        "<base href='https://cdn.test/assets/'><img src='same.png' alt='Remote'>",
    );
    assert!(external_document.assets.is_empty());
    assert_eq!(
        external_document.blocks,
        vec![Block::Paragraph {
            content: vec![Inline::Text("Remote".into())],
        }]
    );
    assert_eq!(
        external_document
            .warnings
            .iter()
            .map(|warning| warning.code.clone())
            .collect::<Vec<_>>(),
        vec![WarningCode::ExternalAssetSkipped]
    );

    let local = TempDir::new().unwrap();
    fs::create_dir(local.path().join("assets")).unwrap();
    fs::write(local.path().join("assets/local.png"), b"local").unwrap();
    let local_document = convert_html(
        &local,
        "<base href='assets/'><img src='local.png' alt='Local'>",
    );
    assert_eq!(local_document.assets.len(), 1);
    assert_eq!(local_document.assets[0].data, b"local");
}

#[test]
fn inline_whitespace_is_normalized_across_nested_boundaries() {
    let temp = TempDir::new().unwrap();
    let document = convert_html(
        &temp,
        "<p>  Hello<strong> world</strong> and <em>friends </em><a href='x'> link</a>  </p>",
    );

    assert_eq!(
        document.blocks,
        vec![Block::Paragraph {
            content: vec![
                Inline::Text("Hello".into()),
                Inline::Text(" ".into()),
                Inline::Strong(vec![Inline::Text("world".into())]),
                Inline::Text(" ".into()),
                Inline::Text("and".into()),
                Inline::Text(" ".into()),
                Inline::Emphasis(vec![Inline::Text("friends".into())]),
                Inline::Text(" ".into()),
                Inline::Link {
                    url: "x".into(),
                    title: None,
                    content: vec![Inline::Text("link".into())],
                },
            ],
        }]
    );
    assert_eq!(
        emitted(&document),
        "Hello **world** and *friends* [link](x)\n"
    );
}

#[test]
fn invalid_and_external_images_use_exact_warning_taxonomy() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("note.txt"), b"not an image").unwrap();
    fs::create_dir(temp.path().join("folder.png")).unwrap();
    let outside = TempDir::new().unwrap();
    let outside_path = outside.path().join("outside.png");
    fs::write(&outside_path, b"outside").unwrap();
    let outside_url = Url::from_file_path(&outside_path).unwrap();
    let html = format!(
        "<img alt='Missing'><img src='data:image/png;base64,%%%' alt='Malformed'><img src='data:text/plain,hello' alt='Unsupported data'><img src='note.txt' alt='Unsupported local'><img src='gone.png' alt='Gone'><img src='folder.png' alt='Folder'><img src='https://cdn.test/a.png' alt='Remote'><img src='{outside_url}' alt='Outside'><img>"
    );
    let document = convert_html(&temp, &html);

    assert_eq!(
        document
            .warnings
            .iter()
            .map(|warning| warning.code.clone())
            .collect::<Vec<_>>(),
        vec![
            WarningCode::InvalidAssetSkipped,
            WarningCode::InvalidAssetSkipped,
            WarningCode::InvalidAssetSkipped,
            WarningCode::InvalidAssetSkipped,
            WarningCode::InvalidAssetSkipped,
            WarningCode::InvalidAssetSkipped,
            WarningCode::ExternalAssetSkipped,
            WarningCode::ExternalAssetSkipped,
            WarningCode::MissingImageAlt,
            WarningCode::InvalidAssetSkipped,
        ]
    );
    assert!(
        document
            .warnings
            .iter()
            .all(|warning| warning.page.is_none())
    );
    assert_eq!(document.blocks.len(), 8);
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
