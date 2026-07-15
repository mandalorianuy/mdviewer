use std::{fs, path::PathBuf};

use mdconvert_core::{ConversionError, ConversionRequest};
use mdconvert_pdf::{RawRect, RuleKind, extract_pdf};
use tempfile::tempdir;

const TOLERANCE: f32 = 0.25;

fn workspace_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn request_for(path: impl Into<PathBuf>) -> ConversionRequest {
    ConversionRequest::new(path).expect("fixture path should be valid")
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= TOLERANCE,
        "expected {actual} to be within {TOLERANCE} of {expected}"
    );
}

fn assert_rect_close(actual: &RawRect, expected: [f32; 4]) {
    assert_close(actual.left, expected[0]);
    assert_close(actual.top, expected[1]);
    assert_close(actual.right, expected[2]);
    assert_close(actual.bottom, expected[3]);
}

#[test]
fn extracts_digital_fixture_in_pdf_text_and_object_order() {
    let document = extract_pdf(&request_for(workspace_path(
        "tests/fixtures/pdf/digital-basic.pdf",
    )))
    .expect("digital fixture should extract");

    assert_eq!(document.metadata.title.as_deref(), Some("Digital Basic"));
    assert_eq!(document.metadata.author.as_deref(), Some("MDViewer Tests"));
    assert_eq!(
        document.metadata.subject.as_deref(),
        Some("Raw PDF geometry")
    );
    assert_eq!(document.metadata.source_format.as_deref(), Some("pdf"));
    assert_eq!(document.metadata.page_count, Some(1));
    assert_eq!(
        document.metadata.properties,
        [
            ("creation_date".into(), "D:20260715120000Z".into()),
            (
                "creator".into(),
                "deterministic Python stdlib fixture".into()
            ),
            ("producer".into(), "MDViewer fixture builder".into()),
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(document.pages.len(), 1);

    let page = &document.pages[0];
    assert_eq!(page.number, 1);
    assert_close(page.width, 300.0);
    assert_close(page.height, 400.0);
    assert_eq!(page.rotation_degrees, 0);

    let glyph_text: String = page
        .glyphs
        .iter()
        .map(|glyph| glyph.text.as_str())
        .collect();
    assert_eq!(glyph_text, "Bold Title\r\nAlpha Beta");
    assert_eq!(page.glyphs.len(), 22);
    assert!(page.glyphs.iter().all(|glyph| {
        glyph.bounds.left.is_finite()
            && glyph.bounds.top.is_finite()
            && glyph.bounds.right.is_finite()
            && glyph.bounds.bottom.is_finite()
            && glyph.bounds.left <= glyph.bounds.right
            && glyph.bounds.top <= glyph.bounds.bottom
    }));
    assert_rect_close(&page.glyphs[0].bounds, [37.314, 47.112, 48.114, 60.0]);
    assert_rect_close(&page.glyphs[12].bounds, [36.0, 92.124, 43.348, 100.0]);
    assert_eq!(page.glyphs[0].font_name.as_deref(), Some("Helvetica-Bold"));
    assert_eq!(page.glyphs[0].font_weight, Some(700));
    assert_close(page.glyphs[0].font_size, 18.0);
    assert_eq!(page.glyphs[12].font_name.as_deref(), Some("Helvetica"));
    assert_eq!(page.glyphs[12].font_weight, Some(440));
    assert_close(page.glyphs[12].font_size, 11.0);

    assert_eq!(
        page.words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>(),
        ["Bold", "Title", "Alpha", "Beta"]
    );
    assert_eq!(
        page.words
            .iter()
            .map(|word| (word.glyph_start, word.glyph_end))
            .collect::<Vec<_>>(),
        [(0, 4), (5, 10), (12, 17), (18, 22)]
    );

    assert_eq!(page.images.len(), 1);
    assert_eq!(page.images[0].index, 1);
    assert_rect_close(&page.images[0].bounds, [36.0, 150.0, 76.0, 180.0]);
    assert_close(page.images[0].bounds.width(), 40.0);
    assert_close(page.images[0].bounds.height(), 30.0);
    assert_eq!(
        (page.images[0].pixel_width, page.images[0].pixel_height),
        (2, 2)
    );
    assert_eq!(
        page.images[0].rgba,
        [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ]
    );

    assert_eq!(page.links.len(), 1);
    assert_eq!(page.links[0].target, "https://example.test/pdf");
    assert_rect_close(&page.links[0].bounds, [36.0, 94.0, 112.0, 108.0]);

    assert_eq!(page.rules.len(), 2);
    assert_eq!(page.rules[0].kind, RuleKind::Line);
    assert_rect_close(&page.rules[0].bounds, [34.0, 208.0, 202.0, 212.0]);
    assert_close(page.rules[0].stroke_width, 2.0);
    assert_eq!(page.rules[1].kind, RuleKind::Rectangle);
    assert_rect_close(&page.rules[1].bounds, [35.0, 229.0, 137.0, 261.0]);
    assert_close(page.rules[1].stroke_width, 1.0);
}

#[test]
fn checks_input_limits_before_loading_pdfium() {
    let mut request = request_for(workspace_path("tests/fixtures/pdf/digital-basic.pdf"));
    request.limits.max_input_bytes = 1;

    assert!(matches!(
        extract_pdf(&request),
        Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            actual,
            maximum: 1,
        }) if actual > 1
    ));
}

#[test]
fn rejects_page_counts_above_the_request_limit() {
    let mut request = request_for(workspace_path("tests/fixtures/pdf/digital-basic.pdf"));
    request.limits.max_pages = 0;

    assert!(matches!(
        extract_pdf(&request),
        Err(ConversionError::LimitExceeded {
            limit: "max_pages",
            actual: 1,
            maximum: 0,
        })
    ));
}

#[test]
fn rejects_non_regular_sources_before_loading_pdfium() {
    assert!(matches!(
        extract_pdf(&request_for(workspace_path("tests/fixtures/pdf"))),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn returns_ocr_required_for_a_page_without_non_whitespace_text() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("blank.pdf");
    fs::write(&source, blank_pdf()).unwrap();

    assert!(matches!(
        extract_pdf(&request_for(source)),
        Err(ConversionError::OcrRequired)
    ));
}

fn blank_pdf() -> Vec<u8> {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".as_slice(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".as_slice(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] >>".as_slice(),
    ];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}
