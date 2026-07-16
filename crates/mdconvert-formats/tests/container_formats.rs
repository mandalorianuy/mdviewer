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
    let mut entries = entries.to_vec();
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("docx") => entries.extend(package_envelope(
            "word/document.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        )),
        Some("pptx") => entries.extend(package_envelope(
            "ppt/presentation.xml",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
        )),
        Some("xlsx") => entries.extend(package_envelope(
            "xl/workbook.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
        )),
        _ => {}
    }
    fs::write(path, stored_zip(&entries)).unwrap();
}

fn package_envelope(
    main_part: &'static str,
    main_content_type: &'static str,
) -> [TestEntry<'static>; 2] {
    let content_types = match main_content_type {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml" => {
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="png" ContentType="image/png"/><Default Extension="jpg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.as_slice()
        }
        "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml" => {
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="png" ContentType="image/png"/><Default Extension="jpg" ContentType="image/jpeg"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/></Types>"#.as_slice()
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml" => {
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="png" ContentType="image/png"/><Default Extension="jpg" ContentType="image/jpeg"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#.as_slice()
        }
        _ => unreachable!(),
    };
    let root_rels = match main_part {
        "word/document.xml" => br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="root" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.as_slice(),
        "ppt/presentation.xml" => br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="root" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#.as_slice(),
        "xl/workbook.xml" => br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="root" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.as_slice(),
        _ => unreachable!(),
    };
    [
        TestEntry::file("[Content_Types].xml", content_types),
        TestEntry::file("_rels/.rels", root_rels),
    ]
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

fn png_chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    output.extend_from_slice(kind);
    output.extend_from_slice(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    output.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn truecolor_png_with_palette_after_transparency() -> Vec<u8> {
    let mut output = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    png_chunk(&mut output, b"IHDR", &ihdr);
    png_chunk(&mut output, b"tRNS", &[0, 0, 0, 0, 0, 0]);
    png_chunk(&mut output, b"PLTE", &[0, 0, 0]);
    png_chunk(
        &mut output,
        b"IDAT",
        &[0x78, 0x9c, 0x63, 0x60, 0x60, 0x60, 0, 0, 0, 4, 0, 1],
    );
    png_chunk(&mut output, b"IEND", &[]);
    output
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

fn descriptor_zip(name: &str, data: &[u8], descriptor: Option<(bool, u32, u32, u32)>) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut output = Vec::new();
    let name = name.as_bytes();
    let crc = crc32(data);
    push_u32(&mut output, 0x0403_4b50);
    push_u16(&mut output, 20);
    push_u16(&mut output, (1 << 11) | (1 << 3));
    push_u16(&mut output, 8);
    push_u16(&mut output, 0);
    push_u16(&mut output, 0);
    push_u32(&mut output, 0);
    push_u32(&mut output, 0);
    push_u32(&mut output, 0);
    push_u16(&mut output, u16::try_from(name.len()).unwrap());
    push_u16(&mut output, 0);
    output.extend_from_slice(name);
    output.extend_from_slice(&compressed);
    if let Some((signature, descriptor_crc, descriptor_compressed, descriptor_uncompressed)) =
        descriptor
    {
        if signature {
            push_u32(&mut output, 0x0807_4b50);
        }
        push_u32(&mut output, descriptor_crc);
        push_u32(&mut output, descriptor_compressed);
        push_u32(&mut output, descriptor_uncompressed);
    }
    let central_offset = u32::try_from(output.len()).unwrap();
    push_u32(&mut output, 0x0201_4b50);
    push_u16(&mut output, (3 << 8) | 20);
    push_u16(&mut output, 20);
    push_u16(&mut output, (1 << 11) | (1 << 3));
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

fn eocd_offset(bytes: &[u8]) -> usize {
    bytes
        .windows(4)
        .rposition(|window| window == 0x0605_4b50u32.to_le_bytes())
        .unwrap()
}

fn central_offset(bytes: &[u8]) -> usize {
    let eocd = eocd_offset(bytes);
    usize::try_from(u32::from_le_bytes(
        bytes[eocd + 16..eocd + 20].try_into().unwrap(),
    ))
    .unwrap()
}

fn prepend_valid_zip_preamble(bytes: &[u8], prefix: &[u8]) -> Vec<u8> {
    let old_central = central_offset(bytes);
    let old_eocd = eocd_offset(bytes);
    let mut output = prefix.to_vec();
    output.extend_from_slice(bytes);
    let delta = u32::try_from(prefix.len()).unwrap();
    let mut cursor = old_central + prefix.len();
    while cursor < old_eocd + prefix.len() {
        let local = u32::from_le_bytes(output[cursor + 42..cursor + 46].try_into().unwrap());
        output[cursor + 42..cursor + 46].copy_from_slice(&(local + delta).to_le_bytes());
        let name = usize::from(u16::from_le_bytes(
            output[cursor + 28..cursor + 30].try_into().unwrap(),
        ));
        let extra = usize::from(u16::from_le_bytes(
            output[cursor + 30..cursor + 32].try_into().unwrap(),
        ));
        let comment = usize::from(u16::from_le_bytes(
            output[cursor + 32..cursor + 34].try_into().unwrap(),
        ));
        cursor += 46 + name + extra + comment;
    }
    let new_eocd = old_eocd + prefix.len();
    output[new_eocd + 16..new_eocd + 20]
        .copy_from_slice(&(u32::try_from(old_central).unwrap() + delta).to_le_bytes());
    output
}

fn insert_valid_local_gap(bytes: &[u8], gap: &[u8]) -> Vec<u8> {
    let old_central = central_offset(bytes);
    let old_eocd = eocd_offset(bytes);
    let first_name = usize::from(u16::from_le_bytes(bytes[26..28].try_into().unwrap()));
    let first_extra = usize::from(u16::from_le_bytes(bytes[28..30].try_into().unwrap()));
    let first_compressed =
        usize::try_from(u32::from_le_bytes(bytes[18..22].try_into().unwrap())).unwrap();
    let insertion = 30 + first_name + first_extra + first_compressed;
    let mut output = bytes.to_vec();
    output.splice(insertion..insertion, gap.iter().copied());
    let delta = u32::try_from(gap.len()).unwrap();
    let mut cursor = old_central + gap.len();
    while cursor < old_eocd + gap.len() {
        let local = u32::from_le_bytes(output[cursor + 42..cursor + 46].try_into().unwrap());
        if usize::try_from(local).unwrap() >= insertion {
            output[cursor + 42..cursor + 46].copy_from_slice(&(local + delta).to_le_bytes());
        }
        let name = usize::from(u16::from_le_bytes(
            output[cursor + 28..cursor + 30].try_into().unwrap(),
        ));
        let extra = usize::from(u16::from_le_bytes(
            output[cursor + 30..cursor + 32].try_into().unwrap(),
        ));
        let comment = usize::from(u16::from_le_bytes(
            output[cursor + 32..cursor + 34].try_into().unwrap(),
        ));
        cursor += 46 + name + extra + comment;
    }
    let new_eocd = old_eocd + gap.len();
    output[new_eocd + 16..new_eocd + 20]
        .copy_from_slice(&(u32::try_from(old_central).unwrap() + delta).to_le_bytes());
    output
}

fn reorder_central_records(bytes: &[u8], order: &[usize]) -> Vec<u8> {
    let central = central_offset(bytes);
    let eocd = eocd_offset(bytes);
    let mut cursor = central;
    let mut records = Vec::new();
    while cursor < eocd {
        assert_eq!(&bytes[cursor..cursor + 4], &0x0201_4b50u32.to_le_bytes());
        let name = usize::from(u16::from_le_bytes(
            bytes[cursor + 28..cursor + 30].try_into().unwrap(),
        ));
        let extra = usize::from(u16::from_le_bytes(
            bytes[cursor + 30..cursor + 32].try_into().unwrap(),
        ));
        let comment = usize::from(u16::from_le_bytes(
            bytes[cursor + 32..cursor + 34].try_into().unwrap(),
        ));
        let end = cursor + 46 + name + extra + comment;
        records.push(bytes[cursor..end].to_vec());
        cursor = end;
    }
    assert_eq!(order.len(), records.len());
    let mut output = bytes.to_vec();
    let mut cursor = central;
    for index in order {
        let record = &records[*index];
        output[cursor..cursor + record.len()].copy_from_slice(record);
        cursor += record.len();
    }
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
        if matches!(name, "semantic.docx" | "ordered.pptx" | "displayed.xlsx") {
            assert_eq!(
                document
                    .metadata
                    .properties
                    .get("ooxml_profile")
                    .map(String::as_str),
                Some("transitional_only")
            );
        }
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
fn archive_limits_are_capped_by_the_request_input_budget() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("request-budget.zip");
    let data = vec![0; 4_096];
    let bytes = descriptor_zip(
        "large.txt",
        &data,
        Some((true, crc32(&data), 20, u32::try_from(data.len()).unwrap())),
    );
    // Correct the compressed length in the descriptor from the central record.
    let central = central_offset(&bytes);
    let compressed = u32::from_le_bytes(bytes[central + 20..central + 24].try_into().unwrap());
    let mut bytes = bytes;
    let descriptor = central - 16;
    bytes[descriptor + 8..descriptor + 12].copy_from_slice(&compressed.to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    let mut request = request(path);
    request.limits.max_input_bytes = u64::try_from(bytes.len()).unwrap();

    assert!(matches!(
        ZipConverter.convert(&request),
        Err(ConversionError::LimitExceeded {
            limit: "archive_entry_uncompressed_bytes" | "archive_total_uncompressed_bytes",
            ..
        })
    ));
}

#[test]
fn zip_requires_complete_ordered_local_records_and_valid_data_descriptors() {
    let temp = TempDir::new().unwrap();
    let data = b"payload";
    let valid = descriptor_zip(
        "file.txt",
        data,
        Some((true, crc32(data), 9, u32::try_from(data.len()).unwrap())),
    );
    let central = central_offset(&valid);
    let compressed = u32::from_le_bytes(valid[central + 20..central + 24].try_into().unwrap());
    let mut valid = valid;
    let descriptor = central - 16;
    valid[descriptor + 8..descriptor + 12].copy_from_slice(&compressed.to_le_bytes());

    let valid_path = temp.path().join("descriptor.zip");
    fs::write(&valid_path, &valid).unwrap();
    ZipConverter.convert(&request(valid_path)).unwrap();

    let no_signature = descriptor_zip(
        "file.txt",
        data,
        Some((
            false,
            crc32(data),
            compressed,
            u32::try_from(data.len()).unwrap(),
        )),
    );
    let no_signature_path = temp.path().join("descriptor-no-signature.zip");
    fs::write(&no_signature_path, no_signature).unwrap();
    ZipConverter.convert(&request(no_signature_path)).unwrap();

    let missing = temp.path().join("missing-descriptor.zip");
    fs::write(&missing, descriptor_zip("file.txt", data, None)).unwrap();
    assert!(matches!(
        ZipConverter.convert(&request(missing)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let corrupt_descriptor = temp.path().join("corrupt-descriptor.zip");
    fs::write(
        &corrupt_descriptor,
        descriptor_zip(
            "file.txt",
            data,
            Some((true, 0, compressed, u32::try_from(data.len()).unwrap())),
        ),
    )
    .unwrap();
    assert!(matches!(
        ZipConverter.convert(&request(corrupt_descriptor)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let preamble = temp.path().join("preamble.zip");
    fs::write(
        &preamble,
        prepend_valid_zip_preamble(&valid, b"polyglot-prefix"),
    )
    .unwrap();
    assert!(matches!(
        ZipConverter.convert(&request(preamble)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let gap = temp.path().join("gap.zip");
    let two = stored_zip(&[
        TestEntry::file("one.txt", b"one"),
        TestEntry::file("two.txt", b"two"),
    ]);
    fs::write(&gap, insert_valid_local_gap(&two, b"unexplained-gap")).unwrap();
    assert!(matches!(
        ZipConverter.convert(&request(gap)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn zip_accepts_signatureless_descriptor_when_crc_equals_the_signature_magic() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("descriptor-magic-crc.zip");
    let data = [0xac, 0x0a, 0x7a, 0xd5];
    assert_eq!(crc32(&data), 0x0807_4b50);
    let mut bytes = descriptor_zip(
        "data.bin",
        &data,
        Some((false, 0x0807_4b50, 0, u32::try_from(data.len()).unwrap())),
    );
    let central = central_offset(&bytes);
    let compressed = u32::from_le_bytes(bytes[central + 20..central + 24].try_into().unwrap());
    let descriptor = central - 12;
    bytes[descriptor + 4..descriptor + 8].copy_from_slice(&compressed.to_le_bytes());
    fs::write(&path, bytes).unwrap();

    assert!(ZipConverter.convert(&request(path)).is_ok());
}

#[test]
fn epub_mimetype_order_is_authenticated_from_physical_local_records() {
    let temp = TempDir::new().unwrap();
    let entries = [
        TestEntry::file("mimetype", b"application/epub+zip"),
        TestEntry::file(
            "META-INF/container.xml",
            br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        ),
        TestEntry::file(
            "OPS/content.opf",
            br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="c" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#,
        ),
        TestEntry::file("OPS/chapter.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Body</p></body></html>"#),
    ];
    let physical_first = reorder_central_records(&stored_zip(&entries), &[1, 2, 3, 0]);
    let path = temp.path().join("physical-first.epub");
    fs::write(&path, physical_first).unwrap();
    EpubConverter.convert(&request(path)).unwrap();

    let physical_second_entries = [entries[1], entries[0], entries[2], entries[3]];
    let central_first =
        reorder_central_records(&stored_zip(&physical_second_entries), &[1, 0, 2, 3]);
    let path = temp.path().join("central-first.epub");
    fs::write(&path, central_first).unwrap();
    assert!(matches!(
        EpubConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn package_targets_normalize_backslashes_before_rejecting_unc_and_drive_paths() {
    let temp = TempDir::new().unwrap();
    let epub = temp.path().join("unc-root.epub");
    write_zip(
        &epub,
        &[
            TestEntry::file("mimetype", b"application/epub+zip"),
            TestEntry::file(
                "META-INF/container.xml",
                br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="\\server\content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            ),
            TestEntry::file(
                "server/content.opf",
                br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="c" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#,
            ),
            TestEntry::file("server/chapter.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Body</p></body></html>"#),
        ],
    );
    assert!(matches!(
        EpubConverter.convert(&request(epub)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let docx = temp.path().join("unc-relationship.docx");
    write_zip(
        &docx,
        &[
            TestEntry::file(
                "word/_rels/document.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="\\media\pixel.png"/></Relationships>"#,
            ),
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body><w:p><w:r><w:drawing><a:blip r:embed="img"/></w:drawing></w:r></w:p></w:body></w:document>"#,
            ),
            TestEntry::file("word/media/pixel.png", b"png"),
        ],
    );
    assert!(matches!(
        DocxConverter.convert(&request(docx)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn package_queries_require_the_expected_expanded_namespaces() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("foreign-namespaces.pptx");
    write_zip(
        &path,
        &[
            TestEntry::file(
                "ppt/presentation.xml",
                br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:evil="urn:evil"><p:sldIdLst><p:sldId evil:id="evilSlide" r:id="missingSlide"/></p:sldIdLst></p:presentation>"#,
            ),
            TestEntry::file(
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships" xmlns:evil="urn:evil"><evil:Relationship Id="evilSlide" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            TestEntry::file(
                "ppt/slides/slide1.xml",
                br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Injected</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            ),
        ],
    );

    assert!(matches!(
        PptxConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn relationships_reject_unknown_target_modes() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("unknown-target-mode.docx");
    let entries = [
        TestEntry::file(
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        TestEntry::file(
            "_rels/.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="root" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml" TargetMode="RemoteMaybe"/></Relationships>"#,
        ),
        TestEntry::file(
            "word/document.xml",
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Body</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ];
    fs::write(&path, stored_zip(&entries)).unwrap();
    assert!(matches!(
        DocxConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn ooxml_strict_is_explicitly_typed_unsupported_not_corrupt() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("strict.docx");
    let entries = [
        TestEntry::file(
            "[Content_Types].xml",
            br#"<Types xmlns="http://purl.oclc.org/ooxml/package/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        TestEntry::file(
            "_rels/.rels",
            br#"<Relationships xmlns="http://purl.oclc.org/ooxml/package/relationships"><Relationship Id="root" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        TestEntry::file(
            "word/document.xml",
            br#"<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body><w:p><w:r><w:t>Strict</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ];
    fs::write(&path, stored_zip(&entries)).unwrap();
    assert!(matches!(
        DocxConverter.convert(&request(path)),
        Err(ConversionError::UnsupportedInput { .. })
    ));

    let escaped = temp.path().join("escaped-strict.docx");
    let entries = [
        TestEntry::file(
            "[Content_Types].xml",
            br#"<Types xmlns="http://p&#117;rl.oclc.org/ooxml/package/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        TestEntry::file(
            "_rels/.rels",
            br#"<Relationships xmlns="http://p&#117;rl.oclc.org/ooxml/package/relationships"><Relationship Id="root" Type="http://p&#117;rl.oclc.org/ooxml/officeDocument/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        TestEntry::file(
            "word/document.xml",
            br#"<w:document xmlns:w="http://p&#117;rl.oclc.org/ooxml/wordprocessingml/main"><w:body/></w:document>"#,
        ),
    ];
    fs::write(&escaped, stored_zip(&entries)).unwrap();
    assert!(matches!(
        DocxConverter.convert(&request(escaped)),
        Err(ConversionError::UnsupportedInput { .. })
    ));

    let literal = temp.path().join("transitional-literal.docx");
    write_zip(
        &literal,
        &[
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>http://purl.oclc.org/ooxml/ is ordinary document text</w:t></w:r></w:p></w:body></w:document>"#,
            ),
            TestEntry::file("custom/binary.bin", b"http://purl.oclc.org/ooxml/"),
        ],
    );
    assert!(DocxConverter.convert(&request(literal)).is_ok());
}

#[test]
fn missing_and_unsafe_ooxml_hyperlinks_preserve_text_and_warn_once() {
    let temp = TempDir::new().unwrap();
    let docx = temp.path().join("missing-links.docx");
    write_zip(
        &docx,
        &[
            TestEntry::file(
                "word/_rels/document.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="unc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="\\server\share"/></Relationships>"#,
            ),
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:hyperlink r:id="missing"><w:r><w:t>Missing</w:t></w:r></w:hyperlink><w:hyperlink r:id="unc"><w:r><w:t>UNC</w:t></w:r></w:hyperlink></w:p></w:body></w:document>"#,
            ),
        ],
    );
    let document = DocxConverter.convert(&request(docx)).unwrap();
    let markdown = emitted(&document);
    assert!(markdown.contains("Missing"));
    assert!(markdown.contains("UNC"));
    assert!(!markdown.contains("server"));
    assert_eq!(
        document
            .warnings
            .iter()
            .filter(|warning| warning.code == WarningCode::InvalidLinkSkipped)
            .count(),
        1
    );

    let pptx = temp.path().join("missing-links.pptx");
    write_zip(
        &pptx,
        &[
            TestEntry::file(
                "ppt/presentation.xml",
                br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId r:id="slide"/></p:sldIdLst></p:presentation>"#,
            ),
            TestEntry::file(
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="slide" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            TestEntry::file(
                "ppt/slides/slide1.xml",
                br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:rPr><a:hlinkClick r:id="missing"/></a:rPr><a:t>Missing</a:t></a:r><a:r><a:rPr><a:hlinkClick r:id="unc"/></a:rPr><a:t>UNC</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            ),
            TestEntry::file(
                "ppt/slides/_rels/slide1.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="unc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="\\server\share"/></Relationships>"#,
            ),
        ],
    );
    let document = PptxConverter.convert(&request(pptx)).unwrap();
    assert!(emitted(&document).contains("MissingUNC"));
    let warnings = document
        .warnings
        .iter()
        .filter(|warning| warning.code == WarningCode::InvalidLinkSkipped)
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].page, Some(1));
}

#[test]
fn ooxml_requires_authenticated_package_envelope_and_real_image_parts() {
    let temp = TempDir::new().unwrap();
    let missing_envelope = temp.path().join("missing-envelope.docx");
    fs::write(
        &missing_envelope,
        stored_zip(&[TestEntry::file(
            "word/document.xml",
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Body</w:t></w:r></w:p></w:body></w:document>"#,
        )]),
    )
    .unwrap();
    assert!(matches!(
        DocxConverter.convert(&request(missing_envelope)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let bogus_image = temp.path().join("bogus-image.docx");
    write_zip(
        &bogus_image,
        &[
            TestEntry::file(
                "word/_rels/document.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pixel.png"/></Relationships>"#,
            ),
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body><w:p><w:r><w:drawing><a:blip r:embed="img"/></w:drawing></w:r></w:p></w:body></w:document>"#,
            ),
            TestEntry::file("word/media/pixel.png", b"not a png"),
        ],
    );
    assert!(matches!(
        DocxConverter.convert(&request(bogus_image)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let header_only_image = temp.path().join("header-only-image.docx");
    write_zip(
        &header_only_image,
        &[
            TestEntry::file(
                "word/_rels/document.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pixel.png"/></Relationships>"#,
            ),
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body><w:p><w:r><w:drawing><a:blip r:embed="img"/></w:drawing></w:r></w:p></w:body></w:document>"#,
            ),
            TestEntry::file("word/media/pixel.png", b"\x89PNG\r\n\x1a\nheader-only"),
        ],
    );
    assert!(matches!(
        DocxConverter.convert(&request(header_only_image)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let bad_filter_image = temp.path().join("bad-filter-image.docx");
    let bad_filter_png: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0,
        0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0b, 0x49, 0x44,
        0x41, 0x54, 0x78, 0x9c, 0x63, 0x65, 0, 2, 0, 0, 0x1e, 0, 6, 0xbc, 0xa9, 0x7c, 0x69, 0, 0,
        0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    write_zip(
        &bad_filter_image,
        &[
            TestEntry::file(
                "word/_rels/document.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pixel.png"/></Relationships>"#,
            ),
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body><w:p><w:r><w:drawing><a:blip r:embed="img"/></w:drawing></w:r></w:p></w:body></w:document>"#,
            ),
            TestEntry::file("word/media/pixel.png", bad_filter_png),
        ],
    );
    assert!(matches!(
        DocxConverter.convert(&request(bad_filter_image)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let bad_order_image = temp.path().join("bad-png-order.docx");
    let bad_order_png = truecolor_png_with_palette_after_transparency();
    write_zip(
        &bad_order_image,
        &[
            TestEntry::file(
                "word/_rels/document.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pixel.png"/></Relationships>"#,
            ),
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body><w:p><w:r><w:drawing><a:blip r:embed="img"/></w:drawing></w:r></w:p></w:body></w:document>"#,
            ),
            TestEntry::file("word/media/pixel.png", &bad_order_png),
        ],
    );
    assert!(matches!(
        DocxConverter.convert(&request(bad_order_image)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let wrong_image_type = temp.path().join("wrong-image-type.docx");
    write_zip(
        &wrong_image_type,
        &[
            TestEntry::file(
                "word/_rels/document.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="media/pixel.png"/></Relationships>"#,
            ),
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body><w:p><w:r><w:drawing><a:blip r:embed="img"/></w:drawing></w:r></w:p></w:body></w:document>"#,
            ),
            TestEntry::file(
                "word/media/pixel.png",
                include_bytes!("../../../tests/fixtures/formats/metadata.png"),
            ),
        ],
    );
    assert!(matches!(
        DocxConverter.convert(&request(wrong_image_type)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let external_image = temp.path().join("external-image.pptx");
    write_zip(
        &external_image,
        &[
            TestEntry::file(
                "ppt/presentation.xml",
                br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId r:id="slide"/></p:sldIdLst></p:presentation>"#,
            ),
            TestEntry::file(
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="slide" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            TestEntry::file(
                "ppt/slides/slide1.xml",
                br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:pic><p:blipFill><a:blip r:embed="image"/></p:blipFill></p:pic></p:spTree></p:cSld></p:sld>"#,
            ),
            TestEntry::file(
                "ppt/slides/_rels/slide1.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="image" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="https://example.invalid/image.png" TargetMode="External"/></Relationships>"#,
            ),
        ],
    );
    assert!(matches!(
        PptxConverter.convert(&request(external_image)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let active_svg = temp.path().join("active-svg.docx");
    let svg_entries = vec![
        TestEntry::file(
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="svg" ContentType="image/svg+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        package_envelope(
            "word/document.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        )[1],
        TestEntry::file(
            "word/_rels/document.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/active.svg"/></Relationships>"#,
        ),
        TestEntry::file(
            "word/document.xml",
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body><w:p><w:r><w:drawing><a:blip r:embed="img"/></w:drawing></w:r></w:p></w:body></w:document>"#,
        ),
        TestEntry::file(
            "word/media/active.svg",
            br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#,
        ),
    ];
    fs::write(&active_svg, stored_zip(&svg_entries)).unwrap();
    assert!(matches!(
        DocxConverter.convert(&request(active_svg)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn docx_preserves_headings_lists_tables_images_and_safe_links() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("semantic.docx");
    let entries = [
        TestEntry::file(
            "docProps/core.xml",
            br#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Contract</dc:title><dc:creator>Ada</dc:creator></cp:coreProperties>"#,
        ),
        TestEntry::file(
            "word/styles.xml",
            br#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:styleId="CustomHeading"><w:name w:val="Section title"/><w:pPr><w:outlineLvl w:val="1"/></w:pPr></w:style></w:styles>"#,
        ),
        TestEntry::file(
            "word/numbering.xml",
            br#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="5"><w:lvl w:ilvl="0"><w:start w:val="3"/><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="9"><w:abstractNumId w:val="5"/></w:num></w:numbering>"#,
        ),
        TestEntry::file(
            "word/_rels/document.xml.rels",
            br##"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rImg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pixel.png"/><Relationship Id="rSafe" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="#details"/><Relationship Id="rExternal" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.invalid/" TargetMode="External"/></Relationships>"##,
        ),
        TestEntry::file(
            "word/document.xml",
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body>
              <w:p><w:pPr><w:pStyle w:val="CustomHeading"/></w:pPr><w:r><w:t>Overview</w:t></w:r></w:p>
              <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="9"/></w:numPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>
              <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="9"/></w:numPr></w:pPr><w:r><w:t>Second</w:t></w:r></w:p>
              <w:p><w:hyperlink r:id="rSafe"><w:r><w:t>Details</w:t></w:r></w:hyperlink><w:r><w:drawing><a:blip r:embed="rImg"/></w:drawing></w:r></w:p>
              <w:p><w:hyperlink r:id="rExternal"><w:r><w:t>External text</w:t></w:r></w:hyperlink></w:p>
              <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Value</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
            </w:body></w:document>"#,
        ),
        TestEntry::file(
            "word/media/pixel.png",
            include_bytes!("../../../tests/fixtures/formats/metadata.png"),
        ),
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
        warning.code == WarningCode::ExternalLinkSkipped && warning.page.is_none()
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
                br#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:styleId="Base"><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style><w:style w:styleId="Derived"><w:basedOn w:val="Base"/><w:name w:val="Custom display"/></w:style></w:styles>"#,
            ),
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Derived"/></w:pPr><w:r><w:t>Inherited</w:t></w:r></w:p></w:body></w:document>"#,
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
fn docx_emits_every_successive_list_run_across_kind_and_level_transitions() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("list-transitions.docx");
    write_zip(
        &path,
        &[
            TestEntry::file(
                "word/numbering.xml",
                br#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="2"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="2"/></w:num></w:numbering>"#,
            ),
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
                <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Ordered one</w:t></w:r></w:p>
                <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>Bullet one</w:t></w:r></w:p>
                <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Ordered two</w:t></w:r></w:p>
                <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Nested</w:t></w:r></w:p>
                <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Ordered three</w:t></w:r></w:p>
                </w:body></w:document>"#,
            ),
        ],
    );

    let markdown = emitted(&DocxConverter.convert(&request(path)).unwrap());
    for text in [
        "Ordered one",
        "Bullet one",
        "Ordered two",
        "Nested",
        "Ordered three",
    ] {
        assert_eq!(markdown.matches(text).count(), 1, "{text} must appear once");
    }
}

#[test]
fn docx_style_inheritance_has_an_iterative_depth_budget() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("deep-styles.docx");
    let mut styles = String::from(
        r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:styleId="S0"><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style>"#,
    );
    for index in 1..=129 {
        styles.push_str(&format!(
            r#"<w:style w:styleId="S{index}"><w:basedOn w:val="S{}"/></w:style>"#,
            index - 1
        ));
    }
    styles.push_str("</w:styles>");
    write_zip(
        &path,
        &[
            TestEntry::file("word/styles.xml", styles.as_bytes()),
            TestEntry::file(
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="S129"/></w:pPr><w:r><w:t>Deep</w:t></w:r></w:p></w:body></w:document>"#,
            ),
        ],
    );

    assert!(matches!(
        DocxConverter.convert(&request(path)),
        Err(ConversionError::LimitExceeded {
            limit: "docx_style_inheritance_depth",
            ..
        })
    ));
}

#[test]
fn pptx_uses_presentation_relationship_order_and_attaches_notes() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("ordered.pptx");
    let entries = [
        TestEntry::file(
            "ppt/presentation.xml",
            br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId r:id="rSecond"/><p:sldId r:id="rFirst"/></p:sldIdLst></p:presentation>"#,
        ),
        TestEntry::file(
            "ppt/_rels/presentation.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rFirst" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/><Relationship Id="rSecond" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide2.xml"/></Relationships>"#,
        ),
        TestEntry::file(
            "ppt/slides/slide1.xml",
            br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>First title</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#,
        ),
        TestEntry::file(
            "ppt/slides/slide2.xml",
            br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:sp><p:txBody><a:p><a:r><a:t>Second body</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#,
        ),
        TestEntry::file(
            "ppt/slides/_rels/slide2.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rNotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide2.xml"/></Relationships>"#,
        ),
        TestEntry::file(
            "ppt/notesSlides/notesSlide2.xml",
            br#"<p:notes xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:sp><p:txBody><a:p><a:r><a:t>Presenter note</a:t></a:r></a:p></p:txBody></p:sp></p:notes>"#,
        ),
    ];
    write_zip(&path, &entries);

    let document = PptxConverter.convert(&request(path)).unwrap();
    let markdown = emitted(&document);
    assert!(markdown.find("Second body").unwrap() < markdown.find("First title").unwrap());
    assert!(markdown.contains("### Notes"));
    assert!(markdown.contains("Presenter note"));
    assert_eq!(document.metadata.page_count, Some(2));

    let ambiguous_path = temp.path().join("ambiguous-notes.pptx");
    let mut ambiguous_entries = entries;
    ambiguous_entries[4] = TestEntry::file(
        "ppt/slides/_rels/slide2.xml.rels",
        br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rNotes1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide2.xml"/><Relationship Id="rNotes2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide2.xml"/></Relationships>"#,
    );
    write_zip(&ambiguous_path, &ambiguous_entries);
    assert!(matches!(
        PptxConverter.convert(&request(ambiguous_path)),
        Err(ConversionError::CorruptInput { .. })
    ));
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
                br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rSlide"/></p:sldIdLst></p:presentation>"#,
            ),
            TestEntry::file(
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rSlide" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            TestEntry::file(
                "ppt/slides/slide1.xml",
                br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sp><p:txBody><a:p><a:r><a:rPr><a:hlinkClick r:id="rLink"/></a:rPr><a:t>Guide</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#,
            ),
            TestEntry::file(
                "ppt/slides/_rels/slide1.xml.rels",
                br##"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="#section"/></Relationships>"##,
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
                br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId r:id="slide"/></p:sldIdLst></p:presentation>"#,
            ),
            TestEntry::file(
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="slide" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            TestEntry::file(
                "ppt/slides/slide1.xml",
                br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>Name</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>Value</a:t></a:r></a:p></a:txBody></a:tc></a:tr><a:tr><a:tc><a:txBody><a:p><a:r><a:t>A</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>1</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl><a:blip r:embed="image"/></p:sld>"#,
            ),
            TestEntry::file(
                "ppt/slides/_rels/slide1.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="image" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/pixel.png"/></Relationships>"#,
            ),
            TestEntry::file(
                "ppt/media/pixel.png",
                include_bytes!("../../../tests/fixtures/formats/metadata.png"),
            ),
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
fn pptx_preserves_shape_tree_order_and_dedupes_page_scoped_link_warnings() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("shape-order.pptx");
    write_zip(
        &path,
        &[
            TestEntry::file(
                "ppt/presentation.xml",
                br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId r:id="slide"/></p:sldIdLst></p:presentation>"#,
            ),
            TestEntry::file(
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="slide" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            TestEntry::file(
                "ppt/slides/slide1.xml",
                br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree>
                <p:sp><p:txBody><a:p><a:r><a:rPr><a:hlinkClick r:id="external"/></a:rPr><a:t>Before</a:t></a:r><a:r><a:rPr><a:hlinkClick r:id="external"/></a:rPr><a:t> again</a:t></a:r></a:p></p:txBody></p:sp>
                <p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>Cell</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>
                <p:pic><p:blipFill><a:blip r:embed="image"/></p:blipFill></p:pic>
                <p:sp><p:txBody><a:p><a:r><a:t>After</a:t></a:r></a:p></p:txBody></p:sp>
                </p:spTree></p:cSld></p:sld>"#,
            ),
            TestEntry::file(
                "ppt/slides/_rels/slide1.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="external" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.invalid" TargetMode="External"/><Relationship Id="image" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/pixel.png"/></Relationships>"#,
            ),
            TestEntry::file(
                "ppt/media/pixel.png",
                include_bytes!("../../../tests/fixtures/formats/metadata.png"),
            ),
        ],
    );

    let document = PptxConverter.convert(&request(path)).unwrap();
    assert!(matches!(
        document.blocks.as_slice(),
        [
            Block::Heading { .. },
            Block::Paragraph { .. },
            Block::Table { .. },
            Block::Image { .. },
            Block::Paragraph { .. }
        ]
    ));
    let link_warnings = document
        .warnings
        .iter()
        .filter(|warning| warning.code == WarningCode::ExternalLinkSkipped)
        .collect::<Vec<_>>();
    assert_eq!(link_warnings.len(), 1);
    assert_eq!(link_warnings[0].page, Some(1));
}

#[test]
fn xlsx_preserves_workbook_order_formulas_and_cached_display_values() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("book.xlsx");
    let entries = [
        TestEntry::file(
            "xl/workbook.xml",
            br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Summary" r:id="r2"/><sheet name="Raw" r:id="r1"/></sheets></workbook>"#,
        ),
        TestEntry::file(
            "xl/_rels/workbook.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="r2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#,
        ),
        TestEntry::file(
            "xl/sharedStrings.xml",
            br#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Label</t></si><si><t>Rate</t></si></sst>"#,
        ),
        TestEntry::file(
            "xl/styles.xml",
            br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="10"/></cellXfs></styleSheet>"#,
        ),
        TestEntry::file(
            "xl/worksheets/sheet1.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>raw</v></c></row></sheetData></worksheet>"#,
        ),
        TestEntry::file(
            "xl/worksheets/sheet2.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2"><f>SUM(1,2)</f><v>3</v></c><c r="B2" s="1"><v>0.125</v></c></row></sheetData></worksheet>"#,
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
                br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet" r:id="r1"/></sheets></workbook>"#,
            ),
            TestEntry::file(
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            TestEntry::file(
                "xl/worksheets/sheet1.xml",
                br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>safe</v></c></row></sheetData></worksheet>"#,
            ),
            TestEntry::file(
                "xl/externalLinks/externalLink1.xml",
                br#"<externalLink><externalBook r:id="network" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/></externalLink>"#,
            ),
        ],
    );

    assert!(matches!(
        XlsxConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn xlsx_requires_strict_ordered_a1_references_and_bounds_sparse_rows() {
    let temp = TempDir::new().unwrap();
    let workbook = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet" r:id="r1"/></sheets></workbook>"#;
    let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let cases = [
        (
            "zero",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A0"><v>x</v></c></row></sheetData></worksheet>"#.as_slice(),
            false,
        ),
        (
            "wrong-row",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A2"><v>x</v></c></row></sheetData></worksheet>"#.as_slice(),
            false,
        ),
        (
            "cell-order",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="B1"><v>b</v></c><c r="A1"><v>a</v></c></row></sheetData></worksheet>"#.as_slice(),
            false,
        ),
        (
            "sparse-limit",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1048576"><c r="A1048576"><v>x</v></c></row></sheetData></worksheet>"#.as_slice(),
            true,
        ),
        (
            "dimension-limit",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:XFD1048576"/><sheetData/></worksheet>"#.as_slice(),
            true,
        ),
    ];
    for (name, worksheet, limit_expected) in cases {
        let path = temp.path().join(format!("{name}.xlsx"));
        write_zip(
            &path,
            &[
                TestEntry::file("xl/workbook.xml", workbook),
                TestEntry::file("xl/_rels/workbook.xml.rels", rels),
                TestEntry::file("xl/worksheets/sheet1.xml", worksheet),
            ],
        );
        let result = XlsxConverter.convert(&request(path));
        if limit_expected {
            assert!(matches!(
                result,
                Err(ConversionError::LimitExceeded {
                    limit: "worksheet_rows",
                    ..
                })
            ));
        } else {
            assert!(matches!(result, Err(ConversionError::CorruptInput { .. })));
        }
    }
}

#[test]
fn xlsx_bounds_the_materialized_rectangle_without_trusting_dimension() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("wide-sparse.xlsx");
    write_zip(
        &path,
        &[
            TestEntry::file(
                "xl/workbook.xml",
                br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet" r:id="r1"/></sheets></workbook>"#,
            ),
            TestEntry::file(
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            TestEntry::file(
                "xl/worksheets/sheet1.xml",
                br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:A1"/><sheetData><row r="100000"><c r="XFD100000"><v>one</v></c></row></sheetData></worksheet>"#,
            ),
        ],
    );

    assert!(matches!(
        XlsxConverter.convert(&request(path)),
        Err(ConversionError::LimitExceeded {
            limit: "worksheet_cells",
            actual: 1_638_400_000,
            maximum: 1_000_000,
        })
    ));
}

#[test]
fn xlsx_rejects_external_workbook_and_dde_formulas_but_keeps_local_references() {
    let temp = TempDir::new().unwrap();
    let workbook = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet" r:id="r1"/></sheets></workbook>"#;
    let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let cases = [
        ("external-book", "[Book.xlsx]Sheet1!A1", true),
        ("quoted-external-book", "'[Book.xlsx]Sheet 1'!A1", true),
        (
            "quoted-external-apostrophe",
            "'[Book.xlsx]O''Brien'!A1",
            true,
        ),
        ("dde", "cmd|' /C calc'!A0", true),
        ("webservice", "WEBSERVICE ( \"https://invalid\" )", true),
        ("rtd", "rTd(\"server\",,\"topic\")", true),
        ("image", "ImAgE ( \"https://invalid\" )", true),
        ("filterxml", "FILTERXML(A1,\"/root\")", false),
        ("quoted-local-sheet", "'Sheet 2'!A1", false),
        ("quoted-local-apostrophe", "'O''Brien'!A1", false),
        ("quoted-local-pipe", "'A|B'!A1", false),
        ("function-looking-sheet", "'WEBSERVICE('!A1", false),
        ("structured-local", "Table1[Column]", false),
        ("local-pipe-string", "\"A|B\"", false),
        (
            "external-looking-string",
            "\"WEBSERVICE([Book.xlsx]Sheet!A1)\"",
            false,
        ),
    ];
    for (name, formula, rejected) in cases {
        let path = temp.path().join(format!("{name}.xlsx"));
        let worksheet = format!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><f>{formula}</f><v>1</v></c></row></sheetData></worksheet>"#
        );
        write_zip(
            &path,
            &[
                TestEntry::file("xl/workbook.xml", workbook),
                TestEntry::file("xl/_rels/workbook.xml.rels", rels),
                TestEntry::file("xl/worksheets/sheet1.xml", worksheet.as_bytes()),
            ],
        );
        let result = XlsxConverter.convert(&request(path));
        if rejected {
            assert!(
                matches!(result, Err(ConversionError::CorruptInput { .. })),
                "{name}"
            );
        } else {
            let markdown = emitted(&result.unwrap());
            let emitted_formula = formula.replace('|', "\\|");
            assert!(markdown.contains(&emitted_formula), "{name}");
        }
    }

    let named = temp.path().join("external-defined-name.xlsx");
    write_zip(
        &named,
        &[
            TestEntry::file(
                "xl/workbook.xml",
                br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><definedNames><definedName name="Bad">[Book.xlsx]Sheet1!A1</definedName></definedNames><sheets><sheet name="Sheet" r:id="r1"/></sheets></workbook>"#,
            ),
            TestEntry::file("xl/_rels/workbook.xml.rels", rels),
            TestEntry::file(
                "xl/worksheets/sheet1.xml",
                br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#,
            ),
        ],
    );
    assert!(matches!(
        XlsxConverter.convert(&request(named)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn xlsx_rejects_connections_query_and_any_external_relationships() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("connections.xlsx");
    write_zip(
        &path,
        &[
            TestEntry::file(
                "xl/workbook.xml",
                br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet" r:id="sheet"/></sheets></workbook>"#,
            ),
            TestEntry::file(
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="sheet" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="connection" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/connections" Target="connections.xml"/><Relationship Id="network" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.invalid" TargetMode="External"/></Relationships>"#,
            ),
            TestEntry::file(
                "xl/worksheets/sheet1.xml",
                br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>safe</v></c></row></sheetData></worksheet>"#,
            ),
            TestEntry::file(
                "xl/connections.xml",
                br#"<connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#,
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
    let image = include_bytes!("../../../tests/fixtures/formats/metadata.png");
    let entries = [
        TestEntry::file("mimetype", b"application/epub+zip"),
        TestEntry::file(
            "META-INF/container.xml",
            br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        ),
        TestEntry::file(
            "OEBPS/content.opf",
            br#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>Local book</dc:title><dc:creator>Ada</dc:creator></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="pic" href="images/pic.png" media-type="image/png"/></manifest><spine><itemref idref="two"/><itemref idref="one"/></spine></package>"#,
        ),
        TestEntry::file(
            "OEBPS/nav.xhtml",
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><nav><ol><li><a href="one.xhtml">First</a></li></ol></nav></body></html>"#,
        ),
        TestEntry::file(
            "OEBPS/one.xhtml",
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>One</h1><p><img src="images/pic.png" alt="Local"/></p><script>fetch('https://invalid')</script></body></html>"#,
        ),
        TestEntry::file(
            "OEBPS/two.xhtml",
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Two</h1><p><a href="https://example.invalid">external label</a></p></body></html>"#,
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
                br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
            ),
            TestEntry::file(
                "content.opf",
                br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="bad" href="../escape.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="bad"/></spine></package>"#,
            ),
        ],
    );
    assert!(matches!(
        EpubConverter.convert(&request(escaping)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn epub_images_accept_only_authenticated_package_parts_and_private_refs() {
    let temp = TempDir::new().unwrap();
    let image = include_bytes!("../../../tests/fixtures/formats/metadata.png");
    let sources = [
        "data:image/png;base64,iVBORw0KGgo=",
        "mdconvert-asset:epub-999",
        "http://example.invalid/pixel.png",
        "file:///tmp/pixel.png",
    ];
    for (index, source) in sources.into_iter().enumerate() {
        let path = temp.path().join(format!("scheme-{index}.epub"));
        let chapter = format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="{source}" alt="probe"/></body></html>"#
        );
        write_zip(
            &path,
            &[
                TestEntry::file("mimetype", b"application/epub+zip"),
                TestEntry::file(
                    "META-INF/container.xml",
                    br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
                ),
                TestEntry::file(
                    "content.opf",
                    br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="pic" href="pixel.png" media-type="image/png"/></manifest><spine><itemref idref="chapter"/></spine></package>"#,
                ),
                TestEntry::file("chapter.xhtml", chapter.as_bytes()),
                TestEntry::file("pixel.png", image),
            ],
        );
        assert!(matches!(
            EpubConverter.convert(&request(path)),
            Err(ConversionError::CorruptInput { .. })
        ));
    }

    let forged = temp.path().join("predictable-alias.epub");
    write_zip(
        &forged,
        &[
            TestEntry::file("mimetype", b"application/epub+zip"),
            TestEntry::file(
                "META-INF/container.xml",
                br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
            ),
            TestEntry::file(
                "content.opf",
                br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="pic" href="pixel.png" media-type="image/png"/></manifest><spine><itemref idref="chapter"/></spine></package>"#,
            ),
            TestEntry::file(
                "chapter.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="pixel.png" alt="real"/><IMG sRc="mdconvert-asset:epub-001" alt="forged"/></body></html>"#,
            ),
            TestEntry::file("pixel.png", image),
        ],
    );
    assert!(matches!(
        EpubConverter.convert(&request(forged)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn epub_manifest_rejects_duplicate_normalized_part_aliases() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("duplicate-part.epub");
    write_zip(
        &path,
        &[
            TestEntry::file("mimetype", b"application/epub+zip"),
            TestEntry::file(
                "META-INF/container.xml",
                br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
            ),
            TestEntry::file(
                "content.opf",
                br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="a" href="images/./pic.png" media-type="image/png"/><item id="b" href="images/pic.png" media-type="image/jpeg"/></manifest><spine><itemref idref="chapter"/></spine></package>"#,
            ),
            TestEntry::file(
                "chapter.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body>chapter</body></html>"#,
            ),
            TestEntry::file(
                "images/pic.png",
                include_bytes!("../../../tests/fixtures/formats/metadata.png"),
            ),
        ],
    );
    assert!(matches!(
        EpubConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn epub_embedded_images_require_full_bounded_structural_validation() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("header-only.epub");
    write_zip(
        &path,
        &[
            TestEntry::file("mimetype", b"application/epub+zip"),
            TestEntry::file(
                "META-INF/container.xml",
                br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
            ),
            TestEntry::file(
                "content.opf",
                br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="pic" href="pixel.png" media-type="image/png"/></manifest><spine><itemref idref="chapter"/></spine></package>"#,
            ),
            TestEntry::file(
                "chapter.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="pixel.png" alt="probe"/></body></html>"#,
            ),
            TestEntry::file("pixel.png", b"\x89PNG\r\n\x1a\nheader-only"),
        ],
    );
    assert!(matches!(
        EpubConverter.convert(&request(path)),
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
                br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
            ),
            TestEntry::file(
                "content.opf",
                br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/></spine></package>"#,
            ),
            TestEntry::file(
                "one.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body>one</body></html>"#,
            ),
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
                br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
            ),
            TestEntry::file(
                "content.opf",
                br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="a" href="a.png" media-type="image/png"/><item id="b" href="b.png" media-type="image/png"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#,
            ),
            TestEntry::file("one.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="a.png"/></body></html>"#),
            TestEntry::file("two.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="b.png"/></body></html>"#),
            TestEntry::file(
                "a.png",
                include_bytes!("../../../tests/fixtures/formats/metadata.png"),
            ),
            TestEntry::file(
                "b.png",
                include_bytes!("../../../tests/fixtures/formats/metadata.png"),
            ),
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

#[test]
fn epub_dedupes_repeated_part_references_but_not_distinct_equal_assets() {
    let temp = TempDir::new().unwrap();
    let build = |path: &Path, second_source: &str| {
        write_zip(
            path,
            &[
                TestEntry::file("mimetype", b"application/epub+zip"),
                TestEntry::file(
                    "META-INF/container.xml",
                    br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
                ),
                TestEntry::file(
                    "content.opf",
                    br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="a" href="a.png" media-type="image/png"/><item id="b" href="b.png" media-type="image/png"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#,
                ),
                TestEntry::file(
                    "one.xhtml",
                    br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="a.png"/></body></html>"#,
                ),
                TestEntry::file(
                    "two.xhtml",
                    format!(
                        r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="{second_source}"/></body></html>"#
                    )
                    .as_bytes(),
                ),
                TestEntry::file(
                    "a.png",
                    include_bytes!("../../../tests/fixtures/formats/metadata.png"),
                ),
                TestEntry::file(
                    "b.png",
                    include_bytes!("../../../tests/fixtures/formats/metadata.png"),
                ),
            ],
        );
    };

    let repeated = temp.path().join("repeated.epub");
    build(&repeated, "a.png");
    let mut repeated_request = request(repeated);
    repeated_request.limits.max_assets = 1;
    let document = EpubConverter.convert(&repeated_request).unwrap();
    assert_eq!(document.assets.len(), 1);

    let distinct = temp.path().join("distinct.epub");
    build(&distinct, "b.png");
    let mut distinct_request = request(distinct);
    distinct_request.limits.max_assets = 1;
    assert!(matches!(
        EpubConverter.convert(&distinct_request),
        Err(ConversionError::LimitExceeded {
            limit: "assets",
            actual: 2,
            maximum: 1,
        })
    ));
}
