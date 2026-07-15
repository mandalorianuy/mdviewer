use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use flate2::{Compression, write::DeflateEncoder};
use mdconvert_core::{
    Block, ConversionError, ConversionRequest, Converter, GfmOptions, Inline, WarningCode, emit_gfm,
};
use mdconvert_formats::{
    ArchiveLimits, DocxConverter, EpubConverter, ImageConverter, LocalFormat, PptxConverter,
    XlsxConverter, ZipConverter, local_v1_formats,
};
use tempfile::TempDir;

fn request(path: impl Into<PathBuf>) -> ConversionRequest {
    ConversionRequest::new(path).unwrap()
}

fn write_zip(path: &Path, entries: &[TestEntry<'_>]) {
    fs::write(path, stored_zip(entries)).unwrap();
}

#[derive(Clone, Copy)]
struct TestEntry<'a> {
    name: &'a str,
    bytes: &'a [u8],
    unix_mode: u32,
}

impl<'a> TestEntry<'a> {
    fn file(name: &'a str, bytes: &'a [u8]) -> Self {
        Self {
            name,
            bytes,
            unix_mode: 0o100644,
        }
    }

    fn symlink(name: &'a str, target: &'a [u8]) -> Self {
        Self {
            name,
            bytes: target,
            unix_mode: 0o120777,
        }
    }

    fn directory(name: &'a str) -> Self {
        Self {
            name,
            bytes: b"",
            unix_mode: 0o040755,
        }
    }
}

fn stored_zip(entries: &[TestEntry<'_>]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = u32::try_from(output.len()).unwrap();
        let crc = crc32(entry.bytes);
        let name = entry.name.as_bytes();
        push_u32(&mut output, 0x0403_4b50);
        push_u16(&mut output, 20);
        push_u16(&mut output, 1 << 11);
        push_u16(&mut output, 0);
        push_u16(&mut output, 0);
        push_u16(&mut output, 0);
        push_u32(&mut output, crc);
        push_u32(&mut output, u32::try_from(entry.bytes.len()).unwrap());
        push_u32(&mut output, u32::try_from(entry.bytes.len()).unwrap());
        push_u16(&mut output, u16::try_from(name.len()).unwrap());
        push_u16(&mut output, 0);
        output.extend_from_slice(name);
        output.extend_from_slice(entry.bytes);

        push_u32(&mut central, 0x0201_4b50);
        push_u16(&mut central, (3 << 8) | 20);
        push_u16(&mut central, 20);
        push_u16(&mut central, 1 << 11);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, crc);
        push_u32(&mut central, u32::try_from(entry.bytes.len()).unwrap());
        push_u32(&mut central, u32::try_from(entry.bytes.len()).unwrap());
        push_u16(&mut central, u16::try_from(name.len()).unwrap());
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, entry.unix_mode << 16);
        push_u32(&mut central, offset);
        central.extend_from_slice(name);
    }
    let central_offset = u32::try_from(output.len()).unwrap();
    let central_size = u32::try_from(central.len()).unwrap();
    output.extend_from_slice(&central);
    push_u32(&mut output, 0x0605_4b50);
    push_u16(&mut output, 0);
    push_u16(&mut output, 0);
    push_u16(&mut output, u16::try_from(entries.len()).unwrap());
    push_u16(&mut output, u16::try_from(entries.len()).unwrap());
    push_u32(&mut output, central_size);
    push_u32(&mut output, central_offset);
    push_u16(&mut output, 0);
    output
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn deflated_zip_with_trailing_data(name: &str, data: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    let mut compressed = encoder.finish().unwrap();
    compressed.extend_from_slice(b"junk");
    let mut output = Vec::new();
    let name = name.as_bytes();
    let crc = crc32(data);
    push_u32(&mut output, 0x0403_4b50);
    push_u16(&mut output, 20);
    push_u16(&mut output, 1 << 11);
    push_u16(&mut output, 8);
    push_u16(&mut output, 0);
    push_u16(&mut output, 0);
    push_u32(&mut output, crc);
    push_u32(&mut output, u32::try_from(compressed.len()).unwrap());
    push_u32(&mut output, u32::try_from(data.len()).unwrap());
    push_u16(&mut output, u16::try_from(name.len()).unwrap());
    push_u16(&mut output, 0);
    output.extend_from_slice(name);
    output.extend_from_slice(&compressed);
    let central_offset = u32::try_from(output.len()).unwrap();
    push_u32(&mut output, 0x0201_4b50);
    push_u16(&mut output, (3 << 8) | 20);
    push_u16(&mut output, 20);
    push_u16(&mut output, 1 << 11);
    push_u16(&mut output, 8);
    push_u16(&mut output, 0);
    push_u16(&mut output, 0);
    push_u32(&mut output, crc);
    push_u32(&mut output, u32::try_from(compressed.len()).unwrap());
    push_u32(&mut output, u32::try_from(data.len()).unwrap());
    push_u16(&mut output, u16::try_from(name.len()).unwrap());
    push_u16(&mut output, 0);
    push_u16(&mut output, 0);
    push_u16(&mut output, 0);
    push_u16(&mut output, 0);
    push_u32(&mut output, 0o100644 << 16);
    push_u32(&mut output, 0);
    output.extend_from_slice(name);
    let central_size = u32::try_from(output.len()).unwrap() - central_offset;
    push_u32(&mut output, 0x0605_4b50);
    push_u16(&mut output, 0);
    push_u16(&mut output, 0);
    push_u16(&mut output, 1);
    push_u16(&mut output, 1);
    push_u32(&mut output, central_size);
    push_u32(&mut output, central_offset);
    push_u16(&mut output, 0);
    output
}

fn emitted(document: &mdconvert_core::Document) -> String {
    emit_gfm(
        document,
        &GfmOptions {
            final_newline: true,
        },
    )
    .unwrap()
}

fn workspace_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

#[test]
fn portable_container_and_image_fixtures_match_shared_emitter_goldens() {
    let cases: [(&str, &dyn Converter); 7] = [
        ("bounded.zip", &ZipConverter),
        ("semantic.docx", &DocxConverter),
        ("ordered.pptx", &PptxConverter),
        ("displayed.xlsx", &XlsxConverter),
        ("spine.epub", &EpubConverter),
        ("metadata.png", &ImageConverter),
        ("metadata.jpg", &ImageConverter),
    ];
    for (name, converter) in cases {
        let document = converter
            .convert(&request(workspace_path(&format!(
                "tests/fixtures/formats/{name}"
            ))))
            .unwrap();
        assert_eq!(
            emitted(&document),
            fs::read_to_string(workspace_path(&format!("tests/golden/formats/{name}.md"))).unwrap(),
            "golden mismatch for {name}"
        );
    }
}

#[test]
fn local_v1_registry_is_explicit_and_network_free() {
    assert_eq!(
        local_v1_formats(),
        &[
            LocalFormat::Pdf,
            LocalFormat::Html,
            LocalFormat::Csv,
            LocalFormat::Json,
            LocalFormat::Xml,
            LocalFormat::Zip,
            LocalFormat::Epub,
            LocalFormat::Docx,
            LocalFormat::Pptx,
            LocalFormat::Xlsx,
            LocalFormat::Png,
            LocalFormat::Jpeg,
        ]
    );
    assert!(
        local_v1_formats()
            .iter()
            .all(|format| !format.extensions().contains(&"youtube"))
    );
}

#[test]
fn zip_rejects_unsafe_names_duplicates_and_special_entries() {
    let temp = TempDir::new().unwrap();
    let cases = [
        vec![TestEntry::file("../escape.txt", b"x")],
        vec![TestEntry::file("safe\\..\\escape.txt", b"x")],
        vec![TestEntry::file("/absolute.txt", b"x")],
        vec![TestEntry::file("C:/drive.txt", b"x")],
        vec![TestEntry::file("//server/share.txt", b"x")],
        vec![TestEntry::file("nul\0name.txt", b"x")],
        vec![
            TestEntry::file("same/name.txt", b"a"),
            TestEntry::file("same\\name.txt", b"b"),
        ],
        vec![TestEntry::symlink("link", b"target")],
    ];

    for (index, entries) in cases.iter().enumerate() {
        let path = temp.path().join(format!("unsafe-{index}.zip"));
        write_zip(&path, entries);
        assert!(
            matches!(
                ZipConverter.convert(&request(path)),
                Err(ConversionError::CorruptInput { .. })
            ),
            "unsafe case {index} was accepted"
        );
    }
}

#[test]
fn zip_checks_limits_crc_and_nested_archives_before_conversion() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("large.zip");
    write_zip(&path, &[TestEntry::file("large.txt", b"12345")]);
    let limits = ArchiveLimits {
        max_entry_uncompressed_bytes: 4,
        ..ArchiveLimits::default()
    };
    assert!(matches!(
        ZipConverter::with_limits(limits).convert(&request(&path)),
        Err(ConversionError::LimitExceeded {
            limit: "archive_entry_uncompressed_bytes",
            ..
        })
    ));

    let corrupt = temp.path().join("crc.zip");
    let mut bytes = stored_zip(&[TestEntry::file("file.txt", b"payload")]);
    let payload = bytes
        .windows(b"payload".len())
        .position(|window| window == b"payload")
        .unwrap();
    bytes[payload] ^= 1;
    fs::write(&corrupt, bytes).unwrap();
    assert!(matches!(
        ZipConverter.convert(&request(corrupt)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let nested = temp.path().join("nested.zip");
    let inner = stored_zip(&[TestEntry::file("inner.txt", b"x")]);
    write_zip(&nested, &[TestEntry::file("inner.zip", &inner)]);
    assert!(matches!(
        ZipConverter.convert(&request(nested)),
        Err(ConversionError::UnsupportedFormat { .. })
    ));
}

#[test]
fn zip_rejects_local_header_size_disagreement() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("local-size.zip");
    let mut bytes = stored_zip(&[TestEntry::file("file.txt", b"payload")]);
    bytes[22..26].copy_from_slice(&99u32.to_le_bytes());
    fs::write(&path, bytes).unwrap();

    assert!(matches!(
        ZipConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn zip_rejects_trailing_bytes_inside_a_deflate_entry() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("deflate-trailing.zip");
    fs::write(
        &path,
        deflated_zip_with_trailing_data("file.txt", b"payload"),
    )
    .unwrap();

    assert!(matches!(
        ZipConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn zip_listing_order_is_normalized_and_deterministic() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("order.zip");
    write_zip(
        &path,
        &[
            TestEntry::file("z-last.txt", b"z"),
            TestEntry::file("a-first.txt", b"a"),
        ],
    );

    let markdown = emitted(&ZipConverter.convert(&request(path)).unwrap());
    assert!(markdown.find("a-first.txt").unwrap() < markdown.find("z-last.txt").unwrap());
}

#[test]
fn zip_converts_the_first_supported_entry_in_normalized_order_without_extraction() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("documents.zip");
    write_zip(
        &path,
        &[
            TestEntry::file("z.json", br#"{"later":true}"#),
            TestEntry::file("a.csv", b"name,value\nfirst,1\n"),
            TestEntry::file("notes.txt", b"inventory only"),
        ],
    );

    let document = ZipConverter.convert(&request(path)).unwrap();
    assert_eq!(
        document.metadata.source_format.as_deref(),
        Some("zip (csv)")
    );
    assert_eq!(
        document.metadata.properties.get("selected_entry"),
        Some(&"a.csv".into())
    );
    assert!(matches!(document.blocks.as_slice(), [Block::Table { .. }]));
    assert!(document.warnings.iter().any(|warning| {
        warning.code == WarningCode::AdditionalArchiveEntriesSkipped
            && warning.message.contains("z.json")
    }));
}

#[test]
fn zip_rejects_encryption_unsupported_compression_truncation_and_entry_limits() {
    let temp = TempDir::new().unwrap();
    let base = stored_zip(&[TestEntry::file("file.txt", b"payload")]);
    let central = base
        .windows(4)
        .position(|window| window == 0x0201_4b50u32.to_le_bytes())
        .unwrap();

    let encrypted = temp.path().join("encrypted.zip");
    let mut bytes = base.clone();
    bytes[6..8].copy_from_slice(&((1 << 11) | 1u16).to_le_bytes());
    bytes[central + 8..central + 10].copy_from_slice(&((1 << 11) | 1u16).to_le_bytes());
    fs::write(&encrypted, bytes).unwrap();
    assert!(matches!(
        ZipConverter.convert(&request(encrypted)),
        Err(ConversionError::EncryptedInput)
    ));

    let unsupported = temp.path().join("unsupported.zip");
    let mut bytes = base.clone();
    bytes[8..10].copy_from_slice(&99u16.to_le_bytes());
    bytes[central + 10..central + 12].copy_from_slice(&99u16.to_le_bytes());
    fs::write(&unsupported, bytes).unwrap();
    assert!(matches!(
        ZipConverter.convert(&request(unsupported)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let truncated = temp.path().join("truncated.zip");
    fs::write(&truncated, &base[..base.len() - 8]).unwrap();
    assert!(matches!(
        ZipConverter.convert(&request(truncated)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let entries = temp.path().join("entries.zip");
    write_zip(
        &entries,
        &[
            TestEntry::file("one.txt", b"1"),
            TestEntry::file("two.txt", b"2"),
        ],
    );
    assert!(matches!(
        ZipConverter::with_limits(ArchiveLimits {
            max_entries: 1,
            ..ArchiveLimits::default()
        })
        .convert(&request(entries)),
        Err(ConversionError::LimitExceeded {
            limit: "archive_entries",
            actual: 2,
            maximum: 1,
        })
    ));

    assert!(matches!(
        ZipConverter::with_limits(ArchiveLimits {
            max_entries: 0,
            ..ArchiveLimits::default()
        })
        .convert(&request(temp.path().join("missing.zip"))),
        Err(ConversionError::ConversionFailed { .. })
    ));

    let ratio = temp.path().join("ratio.zip");
    let mut bytes = stored_zip(&[TestEntry::file("bomb.txt", &vec![0; 2_048])]);
    let central = bytes
        .windows(4)
        .position(|window| window == 0x0201_4b50u32.to_le_bytes())
        .unwrap();
    bytes[18..22].copy_from_slice(&1u32.to_le_bytes());
    bytes[central + 20..central + 24].copy_from_slice(&1u32.to_le_bytes());
    fs::write(&ratio, bytes).unwrap();
    assert!(matches!(
        ZipConverter::with_limits(ArchiveLimits {
            max_expansion_ratio: 10,
            ..ArchiveLimits::default()
        })
        .convert(&request(ratio)),
        Err(ConversionError::LimitExceeded {
            limit: "archive_expansion_ratio",
            ..
        })
    ));

    let total = temp.path().join("total.zip");
    write_zip(
        &total,
        &[
            TestEntry::file("one.txt", b"123"),
            TestEntry::file("two.txt", b"456"),
        ],
    );
    assert!(matches!(
        ZipConverter::with_limits(ArchiveLimits {
            max_total_uncompressed_bytes: 5,
            ..ArchiveLimits::default()
        })
        .convert(&request(total)),
        Err(ConversionError::LimitExceeded {
            limit: "archive_total_uncompressed_bytes",
            actual: 6,
            maximum: 5,
        })
    ));
}

#[test]
fn docx_preserves_headings_lists_tables_images_and_safe_links() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("semantic.docx");
    let entries = [
        TestEntry::file(
            "docProps/core.xml",
            br#"<cp:coreProperties xmlns:cp="urn:cp" xmlns:dc="urn:dc"><dc:title>Contract</dc:title><dc:creator>Ada</dc:creator></cp:coreProperties>"#,
        ),
        TestEntry::file(
            "word/styles.xml",
            br#"<w:styles xmlns:w="urn:w"><w:style w:styleId="CustomHeading"><w:name w:val="Section title"/><w:pPr><w:outlineLvl w:val="1"/></w:pPr></w:style></w:styles>"#,
        ),
        TestEntry::file(
            "word/numbering.xml",
            br#"<w:numbering xmlns:w="urn:w"><w:abstractNum w:abstractNumId="5"><w:lvl w:ilvl="0"><w:start w:val="3"/><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="9"><w:abstractNumId w:val="5"/></w:num></w:numbering>"#,
        ),
        TestEntry::file(
            "word/_rels/document.xml.rels",
            br##"<Relationships xmlns="urn:rels"><Relationship Id="rImg" Type="image" Target="media/pixel.png"/><Relationship Id="rSafe" Type="hyperlink" Target="#details"/><Relationship Id="rExternal" Type="hyperlink" Target="https://example.invalid/" TargetMode="External"/></Relationships>"##,
        ),
        TestEntry::file(
            "word/document.xml",
            br#"<w:document xmlns:w="urn:w" xmlns:r="urn:r" xmlns:a="urn:a"><w:body>
              <w:p><w:pPr><w:pStyle w:val="CustomHeading"/></w:pPr><w:r><w:t>Overview</w:t></w:r></w:p>
              <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="9"/></w:numPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>
              <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="9"/></w:numPr></w:pPr><w:r><w:t>Second</w:t></w:r></w:p>
              <w:p><w:hyperlink r:id="rSafe"><w:r><w:t>Details</w:t></w:r></w:hyperlink><w:r><w:drawing><a:blip r:embed="rImg"/></w:drawing></w:r></w:p>
              <w:p><w:hyperlink r:id="rExternal"><w:r><w:t>External text</w:t></w:r></w:hyperlink></w:p>
              <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Value</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
            </w:body></w:document>"#,
        ),
        TestEntry::file("word/media/pixel.png", b"png-bytes"),
    ];
    write_zip(&path, &entries);

    let document = DocxConverter.convert(&request(path)).unwrap();
    assert_eq!(document.metadata.title.as_deref(), Some("Contract"));
    assert_eq!(document.metadata.author.as_deref(), Some("Ada"));
    assert!(matches!(
        document.blocks.first(),
        Some(Block::Heading { level: 2, content })
            if content == &[Inline::Text("Overview".into())]
    ));
    assert!(document.blocks.iter().any(|block| matches!(
        block,
        Block::List { ordered: true, start: Some(3), items } if items.len() == 2
    )));
    assert!(document.blocks.iter().any(|block| matches!(
        block,
        Block::Table { rows, .. } if rows.len() == 2 && rows[0].len() == 2
    )));
    assert!(
        document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Image { .. }))
    );
    assert_eq!(document.assets.len(), 1);
    let markdown = emitted(&document);
    assert!(markdown.contains("[Details](#details)"));
    assert!(markdown.contains("External text"));
    assert!(!markdown.contains("example.invalid"));
    assert!(document.warnings.iter().any(|warning| {
        warning.code == WarningCode::ExternalAssetSkipped && warning.page.is_none()
    }));
}

#[test]
fn docx_resolves_inherited_heading_styles_without_guessing_from_display_name() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("styles.docx");
    write_zip(
        &path,
        &[
            TestEntry::file(
                "word/styles.xml",
                br#"<w:styles xmlns:w="urn:w"><w:style w:styleId="Base"><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style><w:style w:styleId="Derived"><w:basedOn w:val="Base"/><w:name w:val="Custom display"/></w:style></w:styles>"#,
            ),
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="urn:w"><w:body><w:p><w:pPr><w:pStyle w:val="Derived"/></w:pPr><w:r><w:t>Inherited</w:t></w:r></w:p></w:body></w:document>"#,
            ),
        ],
    );

    let document = DocxConverter.convert(&request(path)).unwrap();
    assert!(matches!(
        document.blocks.as_slice(),
        [Block::Heading { level: 3, .. }]
    ));
}

#[test]
fn pptx_uses_presentation_relationship_order_and_attaches_notes() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("ordered.pptx");
    let entries = [
        TestEntry::file(
            "ppt/presentation.xml",
            br#"<p:presentation xmlns:p="urn:p" xmlns:r="urn:r"><p:sldIdLst><p:sldId r:id="rSecond"/><p:sldId r:id="rFirst"/></p:sldIdLst></p:presentation>"#,
        ),
        TestEntry::file(
            "ppt/_rels/presentation.xml.rels",
            br#"<Relationships><Relationship Id="rFirst" Type="slide" Target="slides/slide1.xml"/><Relationship Id="rSecond" Type="slide" Target="slides/slide2.xml"/></Relationships>"#,
        ),
        TestEntry::file(
            "ppt/slides/slide1.xml",
            br#"<p:sld xmlns:p="urn:p" xmlns:a="urn:a"><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>First title</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#,
        ),
        TestEntry::file(
            "ppt/slides/slide2.xml",
            br#"<p:sld xmlns:p="urn:p" xmlns:a="urn:a"><p:sp><p:txBody><a:p><a:r><a:t>Second body</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#,
        ),
        TestEntry::file(
            "ppt/slides/_rels/slide2.xml.rels",
            br#"<Relationships><Relationship Id="rNotes" Type="notesSlide" Target="../notesSlides/notesSlide2.xml"/></Relationships>"#,
        ),
        TestEntry::file(
            "ppt/notesSlides/notesSlide2.xml",
            br#"<p:notes xmlns:p="urn:p" xmlns:a="urn:a"><p:sp><p:txBody><a:p><a:r><a:t>Presenter note</a:t></a:r></a:p></p:txBody></p:sp></p:notes>"#,
        ),
    ];
    write_zip(&path, &entries);

    let document = PptxConverter.convert(&request(path)).unwrap();
    let markdown = emitted(&document);
    assert!(markdown.find("Second body").unwrap() < markdown.find("First title").unwrap());
    assert!(markdown.contains("### Notes"));
    assert!(markdown.contains("Presenter note"));
    assert_eq!(document.metadata.page_count, Some(2));
}

#[test]
fn pptx_preserves_safe_local_run_links() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("links.pptx");
    write_zip(
        &path,
        &[
            TestEntry::file(
                "ppt/presentation.xml",
                br#"<p:presentation xmlns:p="urn:p" xmlns:r="urn:r"><p:sldIdLst><p:sldId id="256" r:id="rSlide"/></p:sldIdLst></p:presentation>"#,
            ),
            TestEntry::file(
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships><Relationship Id="rSlide" Type="slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            TestEntry::file(
                "ppt/slides/slide1.xml",
                br#"<p:sld xmlns:p="urn:p" xmlns:a="urn:a" xmlns:r="urn:r"><p:sp><p:txBody><a:p><a:r><a:rPr><a:hlinkClick r:id="rLink"/></a:rPr><a:t>Guide</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#,
            ),
            TestEntry::file(
                "ppt/slides/_rels/slide1.xml.rels",
                br##"<Relationships><Relationship Id="rLink" Type="hyperlink" Target="#section"/></Relationships>"##,
            ),
        ],
    );

    let document = PptxConverter.convert(&request(path)).unwrap();
    assert!(emitted(&document).contains("[Guide](#section)"));
}

#[test]
fn pptx_preserves_slide_tables_and_local_images() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("media.pptx");
    write_zip(
        &path,
        &[
            TestEntry::file(
                "ppt/presentation.xml",
                br#"<p:presentation xmlns:p="urn:p" xmlns:r="urn:r"><p:sldIdLst><p:sldId r:id="slide"/></p:sldIdLst></p:presentation>"#,
            ),
            TestEntry::file(
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships><Relationship Id="slide" Type="slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            TestEntry::file(
                "ppt/slides/slide1.xml",
                br#"<p:sld xmlns:p="urn:p" xmlns:a="urn:a" xmlns:r="urn:r"><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>Name</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>Value</a:t></a:r></a:p></a:txBody></a:tc></a:tr><a:tr><a:tc><a:txBody><a:p><a:r><a:t>A</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>1</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl><a:blip r:embed="image"/></p:sld>"#,
            ),
            TestEntry::file(
                "ppt/slides/_rels/slide1.xml.rels",
                br#"<Relationships><Relationship Id="image" Type="image" Target="../media/pixel.png"/></Relationships>"#,
            ),
            TestEntry::file("ppt/media/pixel.png", b"local-image"),
        ],
    );

    let document = PptxConverter.convert(&request(path)).unwrap();
    assert!(document.blocks.iter().any(|block| matches!(
        block,
        Block::Table { rows, .. } if rows.len() == 2 && rows[0].len() == 2
    )));
    assert!(
        document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Image { .. }))
    );
    assert_eq!(document.assets.len(), 1);
}

#[test]
fn xlsx_preserves_workbook_order_formulas_and_cached_display_values() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("book.xlsx");
    let entries = [
        TestEntry::file(
            "xl/workbook.xml",
            br#"<workbook xmlns:r="urn:r"><sheets><sheet name="Summary" r:id="r2"/><sheet name="Raw" r:id="r1"/></sheets></workbook>"#,
        ),
        TestEntry::file(
            "xl/_rels/workbook.xml.rels",
            br#"<Relationships><Relationship Id="r1" Type="worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="r2" Type="worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#,
        ),
        TestEntry::file(
            "xl/sharedStrings.xml",
            br#"<sst><si><t>Label</t></si><si><t>Rate</t></si></sst>"#,
        ),
        TestEntry::file(
            "xl/styles.xml",
            br#"<styleSheet><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="10"/></cellXfs></styleSheet>"#,
        ),
        TestEntry::file(
            "xl/worksheets/sheet1.xml",
            br#"<worksheet><sheetData><row r="1"><c r="A1"><v>raw</v></c></row></sheetData></worksheet>"#,
        ),
        TestEntry::file(
            "xl/worksheets/sheet2.xml",
            br#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2"><f>SUM(1,2)</f><v>3</v></c><c r="B2" s="1"><v>0.125</v></c></row></sheetData></worksheet>"#,
        ),
    ];
    write_zip(&path, &entries);

    let document = XlsxConverter.convert(&request(path)).unwrap();
    let markdown = emitted(&document);
    assert!(markdown.find("## Summary").unwrap() < markdown.find("## Raw").unwrap());
    assert!(markdown.contains("`=SUM(1,2)` (3)"));
    assert!(markdown.contains("12.50%"));
    assert_eq!(document.metadata.page_count, Some(2));
}

#[test]
fn xlsx_rejects_external_link_parts_without_resolving_them() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("external.xlsx");
    write_zip(
        &path,
        &[
            TestEntry::file(
                "xl/workbook.xml",
                br#"<workbook xmlns:r="urn:r"><sheets><sheet name="Sheet" r:id="r1"/></sheets></workbook>"#,
            ),
            TestEntry::file(
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships><Relationship Id="r1" Type="worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            TestEntry::file(
                "xl/worksheets/sheet1.xml",
                br#"<worksheet><sheetData><row r="1"><c r="A1"><v>safe</v></c></row></sheetData></worksheet>"#,
            ),
            TestEntry::file(
                "xl/externalLinks/externalLink1.xml",
                br#"<externalLink><externalBook r:id="network" xmlns:r="urn:r"/></externalLink>"#,
            ),
        ],
    );

    assert!(matches!(
        XlsxConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn epub_validates_container_and_uses_spine_nav_and_local_images() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("book.epub");
    let image = b"local-image";
    let entries = [
        TestEntry::file("mimetype", b"application/epub+zip"),
        TestEntry::file(
            "META-INF/container.xml",
            br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        ),
        TestEntry::file(
            "OEBPS/content.opf",
            br#"<package xmlns:dc="urn:dc"><metadata><dc:title>Local book</dc:title><dc:creator>Ada</dc:creator></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="pic" href="images/pic.png" media-type="image/png"/></manifest><spine><itemref idref="two"/><itemref idref="one"/></spine></package>"#,
        ),
        TestEntry::file(
            "OEBPS/nav.xhtml",
            br#"<html><body><nav><ol><li><a href="one.xhtml">First</a></li></ol></nav></body></html>"#,
        ),
        TestEntry::file(
            "OEBPS/one.xhtml",
            br#"<html><body><h1>One</h1><p><img src="images/pic.png" alt="Local"/></p><script>fetch('https://invalid')</script></body></html>"#,
        ),
        TestEntry::file(
            "OEBPS/two.xhtml",
            br#"<html><body><h1>Two</h1><p><a href="https://example.invalid">external label</a></p></body></html>"#,
        ),
        TestEntry::file("OEBPS/images/pic.png", image),
    ];
    write_zip(&path, &entries);

    let document = EpubConverter.convert(&request(path)).unwrap();
    assert_eq!(document.metadata.title.as_deref(), Some("Local book"));
    assert_eq!(document.metadata.author.as_deref(), Some("Ada"));
    let markdown = emitted(&document);
    assert!(markdown.contains("## Navigation"));
    assert!(markdown.find("# Two").unwrap() < markdown.find("# One").unwrap());
    assert!(markdown.contains("external label"));
    assert!(!markdown.contains("example.invalid"));
    assert!(!markdown.contains("fetch"));
    assert_eq!(document.assets.len(), 1);
    assert_eq!(document.assets[0].data, image);
    assert!(
        document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Image { .. }))
    );
}

#[test]
fn epub_rejects_invalid_mimetype_and_escaping_manifest_targets() {
    let temp = TempDir::new().unwrap();
    let bad_mimetype = temp.path().join("bad-mimetype.epub");
    write_zip(
        &bad_mimetype,
        &[
            TestEntry::file("mimetype", b"application/zip"),
            TestEntry::file("META-INF/container.xml", b"<container/>"),
        ],
    );
    assert!(matches!(
        EpubConverter.convert(&request(bad_mimetype)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let escaping = temp.path().join("escaping.epub");
    write_zip(
        &escaping,
        &[
            TestEntry::file("mimetype", b"application/epub+zip"),
            TestEntry::file(
                "META-INF/container.xml",
                br#"<container><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
            ),
            TestEntry::file(
                "content.opf",
                br#"<package><manifest><item id="bad" href="../escape.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="bad"/></spine></package>"#,
            ),
        ],
    );
    assert!(matches!(
        EpubConverter.convert(&request(escaping)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn epub_requires_mimetype_to_be_the_literal_first_archive_entry() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("directory-first.epub");
    write_zip(
        &path,
        &[
            TestEntry::directory("META-INF/"),
            TestEntry::file("mimetype", b"application/epub+zip"),
            TestEntry::file(
                "META-INF/container.xml",
                br#"<container><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
            ),
            TestEntry::file(
                "content.opf",
                br#"<package><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/></spine></package>"#,
            ),
            TestEntry::file("one.xhtml", b"<html><body>one</body></html>"),
        ],
    );

    assert!(matches!(
        EpubConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn epub_enforces_the_cumulative_asset_budget_across_spine_items() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("assets.epub");
    write_zip(
        &path,
        &[
            TestEntry::file("mimetype", b"application/epub+zip"),
            TestEntry::file(
                "META-INF/container.xml",
                br#"<container><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
            ),
            TestEntry::file(
                "content.opf",
                br#"<package><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="a" href="a.png" media-type="image/png"/><item id="b" href="b.png" media-type="image/png"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#,
            ),
            TestEntry::file("one.xhtml", br#"<html><body><img src="a.png"/></body></html>"#),
            TestEntry::file("two.xhtml", br#"<html><body><img src="b.png"/></body></html>"#),
            TestEntry::file("a.png", b"a"),
            TestEntry::file("b.png", b"b"),
        ],
    );
    let mut request = request(path);
    request.limits.max_assets = 1;

    assert!(matches!(
        EpubConverter.convert(&request),
        Err(ConversionError::LimitExceeded {
            limit: "assets",
            actual: 2,
            maximum: 1,
        })
    ));
}
