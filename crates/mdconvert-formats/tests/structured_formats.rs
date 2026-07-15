use std::{fs, path::PathBuf};

use mdconvert_core::{
    Block, ConversionError, ConversionRequest, Converter, GfmOptions, Inline, WarningCode, emit_gfm,
};
use mdconvert_formats::{
    CsvConverter, DelimiterDetectionError, DetectionError, JsonConverter, StructuredFormat,
    XmlConverter, detect_delimiter, detect_format,
};

fn repository_file(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn request(relative: &str) -> ConversionRequest {
    ConversionRequest::new(repository_file(relative)).expect("valid fixture path")
}

fn assert_golden(converter: &dyn Converter, fixture: &str, golden: &str) {
    let document = converter
        .convert(&request(fixture))
        .expect("conversion succeeds");
    let actual = emit_gfm(
        &document,
        &GfmOptions {
            final_newline: true,
        },
    )
    .expect("GFM emission succeeds");
    let expected = fs::read_to_string(repository_file(golden)).expect("golden is readable");
    assert_eq!(actual, expected);
}

#[test]
fn legacy_structured_fixtures_match_shared_gfm_goldens() {
    assert_golden(
        &CsvConverter,
        "tests/fixtures/formats/sample.csv",
        "tests/golden/formats/sample.csv.md",
    );
    assert_golden(
        &JsonConverter,
        "tests/fixtures/formats/sample.json",
        "tests/golden/formats/sample.json.md",
    );
    assert_golden(
        &XmlConverter,
        "tests/fixtures/formats/sample.xml",
        "tests/golden/formats/sample.xml.md",
    );
}

#[test]
fn detects_all_supported_csv_delimiters_and_rejects_ties() {
    for (delimiter, input) in [
        (b',', b"a,b\n1,2\n".as_slice()),
        (b';', b"a;b\n1;2\n".as_slice()),
        (b'\t', b"a\tb\n1\t2\n".as_slice()),
        (b'|', b"a|b\n1|2\n".as_slice()),
    ] {
        assert_eq!(detect_delimiter(input), Ok(delimiter));
    }

    assert!(matches!(
        detect_delimiter(b"a,b;c\n1,2;3\n"),
        Err(DelimiterDetectionError::Ambiguous { .. })
    ));
}

#[test]
fn csv_preserves_quotes_newlines_empty_cells_and_warns_on_ragged_rows() {
    assert_golden(
        &CsvConverter,
        "tests/fixtures/formats/quoted-ragged.csv",
        "tests/golden/formats/quoted-ragged.csv.md",
    );
    let document = CsvConverter
        .convert(&request("tests/fixtures/formats/quoted-ragged.csv"))
        .expect("conversion succeeds");
    assert_eq!(document.metadata.source_format.as_deref(), Some("csv"));
    assert!(document.warnings.iter().any(|warning| {
        warning.code == WarningCode::TableDegraded && warning.message.contains("ragged")
    }));
}

#[test]
fn json_preserves_source_order_types_arrays_and_nested_objects() {
    let document = JsonConverter
        .convert(&request("tests/fixtures/formats/nested.json"))
        .expect("conversion succeeds");
    assert_eq!(document.metadata.source_format.as_deref(), Some("json"));
    assert_golden(
        &JsonConverter,
        "tests/fixtures/formats/nested.json",
        "tests/golden/formats/nested.json.md",
    );

    let Block::List { items, .. } = &document.blocks[0] else {
        panic!("top-level JSON object must be a list")
    };
    let first_key = match &items[0].blocks[0] {
        Block::Paragraph { content } => match &content[0] {
            Inline::Strong(key) => &key[0],
            other => panic!("expected strong key, got {other:?}"),
        },
        other => panic!("expected paragraph, got {other:?}"),
    };
    assert_eq!(first_key, &Inline::Text("zeta".into()));
}

#[test]
fn xml_preserves_namespaces_attributes_repetition_mixed_content_and_entities() {
    assert_golden(
        &XmlConverter,
        "tests/fixtures/formats/mixed.xml",
        "tests/golden/formats/mixed.xml.md",
    );
    let document = XmlConverter
        .convert(&request("tests/fixtures/formats/mixed.xml"))
        .expect("conversion succeeds");
    assert_eq!(document.metadata.source_format.as_deref(), Some("xml"));
    let rendered = emit_gfm(
        &document,
        &GfmOptions {
            final_newline: false,
        },
    )
    .expect("GFM emission succeeds");
    assert_eq!(rendered.matches("**x:item**").count(), 2);
    assert!(rendered.contains("before"));
    assert!(rendered.contains("after"));
    assert!(rendered.contains("A &amp; B"));
}

#[test]
fn detection_combines_extension_and_content_without_guessing() {
    assert_eq!(
        detect_format(PathBuf::from("report.JSON").as_path(), br#" {"ok":true} "#),
        Ok(StructuredFormat::Json)
    );
    assert_eq!(
        detect_format(PathBuf::from("report.data").as_path(), b"a;b\n1;2\n"),
        Ok(StructuredFormat::Csv)
    );
    assert!(matches!(
        detect_format(PathBuf::from("report.csv").as_path(), br#"{"ok":true}"#),
        Err(DetectionError::Conflict {
            extension: StructuredFormat::Csv,
            signature: StructuredFormat::Json
        })
    ));
    assert!(matches!(
        detect_format(PathBuf::from("report.data").as_path(), b"a,b;c\n1,2;3\n"),
        Err(DetectionError::Ambiguous { .. })
    ));
}

#[test]
fn converters_reject_incompatible_extension_and_signature_instead_of_guessing() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("claims-to-be.csv");
    fs::write(&path, br#"{"actually":"json"}"#).expect("write conflicting input");
    assert!(matches!(
        CsvConverter.convert(&ConversionRequest::new(path).expect("valid request")),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn json_large_numbers_are_not_rounded() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("large.json");
    fs::write(&path, br#"{"large":123456789012345678901234567890}"#)
        .expect("write large number input");
    let document = JsonConverter
        .convert(&ConversionRequest::new(path).expect("valid request"))
        .expect("large finite JSON number converts");
    let rendered = emit_gfm(
        &document,
        &GfmOptions {
            final_newline: false,
        },
    )
    .expect("GFM emission succeeds");
    assert_eq!(rendered, "- **large**: `123456789012345678901234567890`");
}

#[test]
fn invalid_utf8_corrupt_and_trailing_inputs_fail_typed() {
    let directory = tempfile::tempdir().expect("temporary directory");
    for (name, bytes, converter) in [
        (
            "bad.csv",
            b"a,b\n\xff,2".as_slice(),
            &CsvConverter as &dyn Converter,
        ),
        ("bad.json", b"{\"a\":1} trailing".as_slice(), &JsonConverter),
        ("bad.xml", b"<root><open></root>".as_slice(), &XmlConverter),
    ] {
        let path = directory.path().join(name);
        fs::write(&path, bytes).expect("write hostile fixture");
        let error = converter
            .convert(&ConversionRequest::new(path).expect("valid request"))
            .expect_err("input must fail");
        assert!(matches!(error, ConversionError::CorruptInput { .. }));
    }
}

#[test]
fn utf8_bom_is_accepted_without_becoming_document_content() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let cases = [
        (
            "bom.csv",
            b"\xef\xbb\xbfa,b\n1,2\n".as_slice(),
            &CsvConverter as &dyn Converter,
            "| a | b |",
        ),
        (
            "bom.json",
            b"\xef\xbb\xbf{\"a\":1}".as_slice(),
            &JsonConverter as &dyn Converter,
            "- **a**: `1`",
        ),
        (
            "bom.xml",
            b"\xef\xbb\xbf<root>value</root>".as_slice(),
            &XmlConverter as &dyn Converter,
            "- **root**: value",
        ),
    ];
    for (name, bytes, converter, expected) in cases {
        let path = directory.path().join(name);
        fs::write(&path, bytes).expect("write BOM fixture");
        let document = converter
            .convert(&ConversionRequest::new(path).expect("valid request"))
            .expect("UTF-8 BOM is supported");
        let rendered = emit_gfm(
            &document,
            &GfmOptions {
                final_newline: false,
            },
        )
        .expect("GFM emission succeeds");
        assert!(rendered.starts_with(expected), "rendered: {rendered:?}");
        assert!(!rendered.contains('\u{feff}'));
    }
}

#[test]
fn unsafe_xml_and_duplicate_json_keys_fail_closed() {
    let directory = tempfile::tempdir().expect("temporary directory");
    for (name, bytes, converter) in [
        (
            "entity.xml",
            br#"<!DOCTYPE root [<!ENTITY boom "expanded">]><root>&boom;</root>"#.as_slice(),
            &XmlConverter as &dyn Converter,
        ),
        (
            "duplicate.json",
            br#"{"same":1,"same":2}"#.as_slice(),
            &JsonConverter,
        ),
        (
            "reserved-number-key.json",
            br#"{"$serde_json::private::Number":"1"}"#.as_slice(),
            &JsonConverter,
        ),
    ] {
        let path = directory.path().join(name);
        fs::write(&path, bytes).expect("write hostile fixture");
        assert!(matches!(
            converter.convert(&ConversionRequest::new(path).expect("valid request")),
            Err(ConversionError::CorruptInput { .. })
        ));
    }
}

#[test]
fn input_and_recursion_limits_fail_typed() {
    let mut request = request("tests/fixtures/formats/sample.json");
    request.limits.max_input_bytes = 4;
    assert!(matches!(
        JsonConverter.convert(&request),
        Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            ..
        })
    ));

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("deep.json");
    let nested = format!("{}0{}", "[".repeat(129), "]".repeat(129));
    fs::write(&path, nested).expect("write deep JSON");
    assert!(matches!(
        JsonConverter.convert(&ConversionRequest::new(path).expect("valid request")),
        Err(ConversionError::LimitExceeded {
            limit: "json_nesting_depth",
            ..
        })
    ));

    let path = directory.path().join("deep.xml");
    let nested = format!("{}value{}", "<n>".repeat(129), "</n>".repeat(129));
    fs::write(&path, nested).expect("write deep XML");
    assert!(matches!(
        XmlConverter.convert(&ConversionRequest::new(path).expect("valid request")),
        Err(ConversionError::LimitExceeded {
            limit: "xml_nesting_depth",
            ..
        })
    ));
}
