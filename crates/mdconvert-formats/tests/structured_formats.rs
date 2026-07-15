use std::{fs, path::PathBuf};

use mdconvert_core::{
    Block, ConversionError, ConversionRequest, Converter, Document, GfmOptions, Inline,
    WarningCode, emit_gfm,
};
use mdconvert_formats::{
    CsvConverter, DelimiterDetectionError, DetectionError, JsonConverter, StructuredFormat,
    StructuredLimits, XmlConverter, detect_delimiter, detect_format,
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

fn convert_temporary(
    directory: &tempfile::TempDir,
    name: &str,
    bytes: &[u8],
    converter: &dyn Converter,
) -> Result<Document, ConversionError> {
    let path = directory.path().join(name);
    fs::write(&path, bytes).expect("write temporary input");
    converter.convert(&ConversionRequest::new(path).expect("valid request"))
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
            signatures,
        }) if signatures == vec![StructuredFormat::Json]
    ));
    assert!(matches!(
        detect_format(PathBuf::from("report.data").as_path(), b"a,b;c\n1,2;3\n"),
        Err(DetectionError::Ambiguous { .. })
    ));

    for input in [
        b"{name,age\nAlice,30\n".as_slice(),
        b"<name,age\nAlice,30\n".as_slice(),
    ] {
        assert_eq!(
            detect_format(PathBuf::from("report.data").as_path(), input),
            Ok(StructuredFormat::Csv)
        );
        assert_eq!(
            detect_format(PathBuf::from("report.csv").as_path(), input),
            Ok(StructuredFormat::Csv)
        );
    }
}

#[test]
fn detection_validates_candidates_before_reporting_conflict_or_ambiguity() {
    assert!(matches!(
        detect_format(PathBuf::from("report.json").as_path(), b"a,b\n1,2\n"),
        Err(DetectionError::Conflict {
            extension: StructuredFormat::Json,
            signatures,
        }) if signatures == vec![StructuredFormat::Csv]
    ));
    assert!(matches!(
        detect_format(PathBuf::from("report.data").as_path(), b"a,b;c\n1,2;3\n"),
        Err(DetectionError::Ambiguous { .. })
    ));
    assert!(matches!(
        detect_format(PathBuf::from("report.data").as_path(), b"{not json"),
        Err(DetectionError::Unsupported)
    ));
}

#[test]
fn matching_extension_resolves_valid_polyglots_without_hiding_real_conflicts() {
    let cases = [
        ("array.json", b"[1,2]".as_slice(), StructuredFormat::Json),
        (
            "entity.xml",
            b"<root>&amp;</root>".as_slice(),
            StructuredFormat::Xml,
        ),
        (
            "comma.xml",
            b"<root>a,b</root>".as_slice(),
            StructuredFormat::Xml,
        ),
    ];
    for (name, input, expected) in cases {
        assert_eq!(
            detect_format(PathBuf::from(name).as_path(), input),
            Ok(expected)
        );
    }

    for input in [b"[1,2]".as_slice(), b"<root>a,b</root>".as_slice()] {
        assert!(matches!(
            detect_format(PathBuf::from("polyglot.data").as_path(), input),
            Err(DetectionError::Ambiguous { .. })
        ));
    }
    assert!(matches!(
        detect_format(
            PathBuf::from("mismatch.xml").as_path(),
            br#"{"only":"json"}"#,
        ),
        Err(DetectionError::Conflict { .. })
    ));
    assert!(matches!(
        detect_format(PathBuf::from("mismatch.xml").as_path(), b"[1,2]"),
        Err(DetectionError::Conflict {
            extension: StructuredFormat::Xml,
            signatures,
        }) if signatures == vec![StructuredFormat::Csv, StructuredFormat::Json]
    ));

    let directory = tempfile::tempdir().expect("temporary directory");
    convert_temporary(&directory, "array.json", b"[1,2]", &JsonConverter)
        .expect("JSON converter resolves JSON/CSV polyglot by extension");
    convert_temporary(
        &directory,
        "entity.xml",
        b"<root>&amp;</root>",
        &XmlConverter,
    )
    .expect("XML converter accepts a predefined entity");
    convert_temporary(&directory, "comma.xml", b"<root>a,b</root>", &XmlConverter)
        .expect("XML converter resolves XML/CSV polyglot by extension");
    convert_temporary(&directory, "array.csv", b"[1,2]", &CsvConverter)
        .expect("CSV converter resolves JSON/CSV polyglot by extension");
}

#[test]
fn csv_quote_and_record_grammar_is_strict() {
    let directory = tempfile::tempdir().expect("temporary directory");
    for (name, input) in [
        ("unterminated.csv", b"a,b\n\"unterminated,2".as_slice()),
        ("misplaced.csv", b"a,b\nx\"oops,2".as_slice()),
        ("after-quote.csv", b"a,b\n\"x\"oops,2".as_slice()),
        ("bad-double.csv", b"a,b\n\"a\"\"\"b,2".as_slice()),
        ("lone-cr.csv", b"a,b\r1,2".as_slice()),
        ("quoted-lone-cr.csv", b"a,b\n\"x\ry\",2".as_slice()),
    ] {
        assert!(matches!(
            convert_temporary(&directory, name, input, &CsvConverter),
            Err(ConversionError::CorruptInput { .. })
        ));
    }

    let document = convert_temporary(
        &directory,
        "doubled.csv",
        b"a,b\r\n\"a\"\"b\",2\r\n",
        &CsvConverter,
    )
    .expect("valid doubled quote and CRLF convert");
    let rendered = emit_gfm(
        &document,
        &GfmOptions {
            final_newline: false,
        },
    )
    .expect("emit valid CSV");
    assert!(rendered.contains("| a\"b | 2 |"));
}

#[test]
fn csv_empty_blank_header_only_and_trailing_policy_is_explicit() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let empty =
        convert_temporary(&directory, "empty.csv", b"", &CsvConverter).expect("empty CSV converts");
    assert!(empty.blocks.is_empty());
    assert_eq!(empty.warnings.len(), 1);

    let header = convert_temporary(&directory, "header.csv", b"a,b\n", &CsvConverter)
        .expect("header-only CSV converts");
    let Block::Table { rows, .. } = &header.blocks[0] else {
        panic!("CSV is a table")
    };
    assert_eq!(rows.len(), 1);

    let blank = convert_temporary(&directory, "blank.csv", b"a,b\n\n1,2,\n", &CsvConverter)
        .expect("blank records and trailing empty fields convert");
    let Block::Table { rows, .. } = &blank.blocks[0] else {
        panic!("CSV is a table")
    };
    assert_eq!(rows.len(), 3, "interior blank record must be preserved");
    assert_eq!(rows[1].len(), 1);
    assert_eq!(rows[2].len(), 3);
    assert!(blank.warnings.iter().any(|warning| {
        warning.code == WarningCode::TableDegraded && warning.message.contains("ragged")
    }));
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
fn xml_enforces_document_prolog_root_and_epilog_grammar() {
    let directory = tempfile::tempdir().expect("temporary directory");
    for (name, input) in [
        (
            "whitespace-before-decl.xml",
            b" \n<?xml version=\"1.0\"?><root/>".as_slice(),
        ),
        (
            "duplicate-decl.xml",
            b"<?xml version=\"1.0\"?><?xml version=\"1.0\"?><root/>".as_slice(),
        ),
        (
            "trailing-decl.xml",
            b"<root/><?xml version=\"1.0\"?>".as_slice(),
        ),
        (
            "unsupported-version.xml",
            b"<?xml version=\"1.2\"?><root/>".as_slice(),
        ),
        (
            "unsupported-encoding.xml",
            b"<?xml version=\"1.0\" encoding=\"UTF-16\"?><root/>".as_slice(),
        ),
        (
            "invalid-comment.xml",
            b"<root><!-- a--b --></root>".as_slice(),
        ),
        (
            "cdata-before-root.xml",
            b"<![CDATA[text]]><root/>".as_slice(),
        ),
        ("entity-before-root.xml", b"&amp;<root/>".as_slice()),
        ("text-after-root.xml", b"<root/>tail".as_slice()),
        ("reserved-pi.xml", b"<?XML value?><root/>".as_slice()),
    ] {
        assert!(
            matches!(
                convert_temporary(&directory, name, input, &XmlConverter),
                Err(ConversionError::CorruptInput { .. })
            ),
            "{name} must fail"
        );
    }

    convert_temporary(
        &directory,
        "legal.xml",
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!--before--><?work ok?><root/><!--after-->\n",
        &XmlConverter,
    )
    .expect("legal prolog and epilog convert");
}

#[test]
fn xml_enforces_declaration_and_qname_grammar() {
    let directory = tempfile::tempdir().expect("temporary directory");
    for (name, input) in [
        (
            "duplicate-version.xml",
            br#"<?xml version="1.0" version="1.0"?><root/>"#.as_slice(),
        ),
        (
            "unknown-declaration.xml",
            br#"<?xml version="1.0" foo="bar"?><root/>"#.as_slice(),
        ),
        (
            "out-of-order-declaration.xml",
            br#"<?xml version="1.0" standalone="yes" encoding="UTF-8"?><root/>"#.as_slice(),
        ),
        (
            "trailing-declaration-token.xml",
            br#"<?xml version="1.0" junk?><root/>"#.as_slice(),
        ),
        (
            "invalid-standalone.xml",
            br#"<?xml version="1.0" standalone="maybe"?><root/>"#.as_slice(),
        ),
        (
            "invalid-encoding-name.xml",
            br#"<?xml version="1.0" encoding="1UTF"?><root/>"#.as_slice(),
        ),
        (
            "duplicate-encoding.xml",
            br#"<?xml version="1.0" encoding="UTF-8" encoding="UTF-8"?><root/>"#.as_slice(),
        ),
        ("digit-name.xml", b"<1root/>".as_slice()),
        ("empty-prefix.xml", b"<:root/>".as_slice()),
        ("multiple-colons.xml", b"<a:b:c/>".as_slice()),
        (
            "invalid-attribute-qname.xml",
            br#"<root a:b:c="value"/>"#.as_slice(),
        ),
        ("reserved-element-prefix.xml", b"<xmlns:root/>".as_slice()),
    ] {
        assert!(
            matches!(
                convert_temporary(&directory, name, input, &XmlConverter),
                Err(ConversionError::CorruptInput { .. })
            ),
            "{name} must fail"
        );
    }

    for (name, input) in [
        (
            "legal-full-declaration.xml",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="no"?><root/>"#.as_slice(),
        ),
        (
            "legal-qnames.xml",
            br#"<ns:root xmlns:ns="urn:test" xml:lang="en"><ns:item/></ns:root>"#.as_slice(),
        ),
    ] {
        convert_temporary(&directory, name, input, &XmlConverter)
            .unwrap_or_else(|error| panic!("{name} should convert: {error}"));
    }
}

#[test]
fn xml_rejects_illegal_xml_1_0_characters_after_decoding() {
    let directory = tempfile::tempdir().expect("temporary directory");
    for (name, input) in [
        ("control-reference.xml", b"<root>&#1;</root>".as_slice()),
        (
            "hex-control-reference.xml",
            b"<root>&#x1;</root>".as_slice(),
        ),
        (
            "attribute-control-reference.xml",
            br#"<root value="&#1;"/>"#.as_slice(),
        ),
        ("literal-control.xml", b"<root>\x01</root>".as_slice()),
        (
            "cdata-control.xml",
            b"<root><![CDATA[\x01]]></root>".as_slice(),
        ),
    ] {
        assert!(
            matches!(
                convert_temporary(&directory, name, input, &XmlConverter),
                Err(ConversionError::CorruptInput { .. })
            ),
            "{name} must fail"
        );
    }

    convert_temporary(
        &directory,
        "legal-chars.xml",
        "<root a=\"&#9;&#10;&#13;😀\">&#9;&#10;&#13;&#x1F600;😀</root>".as_bytes(),
        &XmlConverter,
    )
    .expect("XML 1.0 whitespace and supplementary characters convert");
}

#[test]
fn xml_mixed_content_preserves_spaces_around_children_without_fake_warning() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let document = convert_temporary(&directory, "spaces.xml", b"<p>a <b/> b</p>", &XmlConverter)
        .expect("mixed content converts");
    assert!(document.warnings.is_empty());
    let Block::List { items, .. } = &document.blocks[0] else {
        panic!("XML root is represented as a list")
    };
    assert_eq!(items.len(), 1);
    let Block::Paragraph { content } = &items[0].blocks[1] else {
        panic!("leading mixed text is a paragraph")
    };
    assert_eq!(content, &vec![Inline::Text("a ".into())]);
    let Block::Paragraph { content } = &items[0].blocks[3] else {
        panic!("trailing mixed text is a paragraph")
    };
    assert_eq!(content, &vec![Inline::Text(" b".into())]);
    let rendered = emit_gfm(
        &document,
        &GfmOptions {
            final_newline: false,
        },
    )
    .expect("emit mixed XML");
    assert!(rendered.contains("a \n\n  - **b**\n\n   b"));
}

#[test]
fn structural_budgets_fail_typed_before_unbounded_growth() {
    let directory = tempfile::tempdir().expect("temporary directory");

    let wide_csv = format!("{}\n", vec!["x"; 4_097].join(","));
    let long_csv = format!("{}\n", "x".repeat(65_537));
    let many_records_csv = "x\n".repeat(10_001);
    let row = vec!["x"; 1_000].join(",");
    let many_cells_csv = format!("{}\n", vec![row; 101].join("\n"));
    for (name, input, limit) in [
        ("wide.csv", wide_csv, "csv_fields_per_record"),
        ("long.csv", long_csv, "csv_field_bytes"),
        ("records.csv", many_records_csv, "csv_records"),
        ("cells.csv", many_cells_csv, "csv_cells"),
    ] {
        assert!(matches!(
            convert_temporary(&directory, name, input.as_bytes(), &CsvConverter),
            Err(ConversionError::LimitExceeded { limit: actual, .. }) if actual == limit
        ));
    }

    let array = format!("[{}]", vec!["0"; 20_001].join(","));
    let object = format!(
        "{{{}}}",
        (0..10_001)
            .map(|index| format!("\"k{index}\":0"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let inner = format!("[{}]", vec!["0"; 600].join(","));
    let nodes = format!("[{}]", vec![inner; 100].join(","));
    for (name, input, limit) in [
        ("array.json", array, "json_array_entries"),
        ("object.json", object, "json_object_entries"),
        ("nodes.json", nodes, "json_nodes"),
    ] {
        assert!(matches!(
            convert_temporary(&directory, name, input.as_bytes(), &JsonConverter),
            Err(ConversionError::LimitExceeded { limit: actual, .. }) if actual == limit
        ));
    }

    let nodes = format!("<root>{}</root>", "<n/>".repeat(10_000));
    let attributes = format!(
        "<root {}/>",
        (0..4_097)
            .map(|index| format!("a{index}=\"x\""))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let text = format!("<root>{}</root>", "x".repeat(1_048_577));
    for (name, input, limit) in [
        ("nodes.xml", nodes, "xml_nodes"),
        ("attributes.xml", attributes, "xml_attributes_per_element"),
        ("text.xml", text, "xml_text_bytes"),
    ] {
        assert!(matches!(
            convert_temporary(&directory, name, input.as_bytes(), &XmlConverter),
            Err(ConversionError::LimitExceeded { limit: actual, .. }) if actual == limit
        ));
    }
}

#[test]
fn structural_budgets_are_configurable_and_validated() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("custom.csv");
    fs::write(&path, b"a,b\n1,2\n").expect("write CSV");
    let request = ConversionRequest::new(path).expect("valid request");

    let limits = StructuredLimits {
        max_csv_records: 1,
        ..StructuredLimits::default()
    };
    assert!(matches!(
        CsvConverter.convert_with_limits(&request, &limits),
        Err(ConversionError::LimitExceeded {
            limit: "csv_records",
            maximum: 1,
            ..
        })
    ));

    let limits = StructuredLimits {
        max_csv_records: 0,
        ..StructuredLimits::default()
    };
    assert!(matches!(
        CsvConverter.convert_with_limits(&request, &limits),
        Err(ConversionError::ConversionFailed { .. })
    ));
}

#[test]
fn container_entry_budgets_win_before_deserializing_excess_children() {
    let directory = tempfile::tempdir().expect("temporary directory");

    let array_path = directory.path().join("precheck-array.json");
    fs::write(&array_path, b"[0,[0,0]]").expect("write JSON array");
    let limits = StructuredLimits {
        max_json_nodes: 2,
        max_json_array_entries: 1,
        ..StructuredLimits::default()
    };
    assert!(matches!(
        JsonConverter.convert_with_limits(
            &ConversionRequest::new(array_path).expect("valid request"),
            &limits,
        ),
        Err(ConversionError::LimitExceeded {
            limit: "json_array_entries",
            actual: 2,
            maximum: 1,
        })
    ));

    let object_path = directory.path().join("precheck-object.json");
    fs::write(&object_path, br#"{"a":0,"b":[0,0]}"#).expect("write JSON object");
    let limits = StructuredLimits {
        max_json_nodes: 2,
        max_json_object_entries: 1,
        ..StructuredLimits::default()
    };
    assert!(matches!(
        JsonConverter.convert_with_limits(
            &ConversionRequest::new(object_path).expect("valid request"),
            &limits,
        ),
        Err(ConversionError::LimitExceeded {
            limit: "json_object_entries",
            actual: 2,
            maximum: 1,
        })
    ));
}

#[test]
fn xml_raw_content_budgets_are_checked_before_decoding() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let limits = StructuredLimits {
        max_xml_text_bytes: 1,
        ..StructuredLimits::default()
    };

    for (name, input) in [
        ("raw-crlf.xml", b"<root>\r\n</root>".as_slice()),
        (
            "raw-cdata-crlf.xml",
            b"<root><![CDATA[\r\n]]></root>".as_slice(),
        ),
        (
            "raw-attribute-reference.xml",
            br#"<root value="&#9;"/>"#.as_slice(),
        ),
    ] {
        let path = directory.path().join(name);
        fs::write(&path, input).expect("write XML budget fixture");
        assert!(matches!(
            XmlConverter.convert_with_limits(
                &ConversionRequest::new(path).expect("valid request"),
                &limits,
            ),
            Err(ConversionError::LimitExceeded {
                limit: "xml_text_bytes",
                maximum: 1,
                ..
            })
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
    for depth in [127, 128] {
        let path = directory.path().join(format!("depth-{depth}.json"));
        let nested = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        fs::write(&path, nested).expect("write boundary JSON");
        JsonConverter
            .convert(&ConversionRequest::new(path).expect("valid request"))
            .unwrap_or_else(|error| panic!("depth {depth} should succeed: {error}"));
    }

    let path = directory.path().join("depth-129.json");
    let nested = format!("{}0{}", "[".repeat(129), "]".repeat(129));
    fs::write(&path, nested).expect("write over-depth JSON");
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
