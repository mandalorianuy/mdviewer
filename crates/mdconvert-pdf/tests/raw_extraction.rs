use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

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
            (
                "modification_date_status".into(),
                "unsupported_by_pdfium_render_0_9_3".into()
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

#[test]
fn applies_intrinsic_rotation_to_display_dimensions_and_every_raw_rect() {
    let temp = tempdir().unwrap();
    let cases = [
        (
            90,
            [500.0, 400.0],
            [340.0, 37.314, 352.888, 48.114],
            [220.0, 36.0, 250.0, 76.0],
            [292.0, 36.0, 306.0, 112.0],
            [188.0, 34.0, 192.0, 202.0],
        ),
        (
            180,
            [400.0, 500.0],
            [351.886, 340.0, 362.686, 352.888],
            [324.0, 220.0, 364.0, 250.0],
            [288.0, 292.0, 364.0, 306.0],
            [198.0, 188.0, 366.0, 192.0],
        ),
        (
            270,
            [500.0, 400.0],
            [147.112, 351.886, 160.0, 362.686],
            [250.0, 324.0, 280.0, 364.0],
            [194.0, 288.0, 208.0, 364.0],
            [308.0, 198.0, 312.0, 366.0],
        ),
    ];

    for (degrees, dimensions, glyph, image, link, rule) in cases {
        let source = temp.path().join(format!("digital-basic-{degrees}.pdf"));
        fs::write(&source, rotated_fixture(degrees)).unwrap();
        let document = extract_pdf(&request_for(source)).unwrap();
        let page = &document.pages[0];
        assert_eq!(page.rotation_degrees, degrees);
        assert_close(page.width, dimensions[0]);
        assert_close(page.height, dimensions[1]);
        let bold_b = page
            .glyphs
            .iter()
            .find(|glyph| glyph.text == "B" && glyph.font_name.as_deref() == Some("Helvetica-Bold"))
            .expect("rotated page should retain the bold title glyph");
        assert_rect_close(&bold_b.bounds, glyph);
        assert_rect_close(&page.images[0].bounds, image);
        assert_rect_close(&page.links[0].bounds, link);
        assert_rect_close(&page.rules[0].bounds, rule);

        for bounds in page
            .glyphs
            .iter()
            .map(|item| &item.bounds)
            .chain(page.words.iter().map(|item| &item.bounds))
            .chain(page.images.iter().map(|item| &item.bounds))
            .chain(page.links.iter().map(|item| &item.bounds))
            .chain(page.rules.iter().map(|item| &item.bounds))
        {
            assert_rect_within_page(bounds, page.width, page.height);
        }
    }
}

#[test]
fn rejects_asset_count_before_attempting_any_image_decode() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("many-images.pdf");
    fs::write(&source, image_pdf(&[(1_000_000, 1_000_000), (1, 1)])).unwrap();
    let mut request = request_for(source);
    request.limits.max_assets = 1;

    assert!(matches!(
        extract_pdf(&request),
        Err(ConversionError::LimitExceeded {
            limit: "max_assets",
            actual: 2,
            maximum: 1,
        })
    ));
}

#[test]
fn rejects_huge_decoded_image_budget_before_attempting_decode() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("huge-image.pdf");
    fs::write(&source, image_pdf(&[(1_000_000, 1_000_000)])).unwrap();
    let request = request_for(source);

    assert!(matches!(
        extract_pdf(&request),
        Err(ConversionError::LimitExceeded {
            limit: "pdf_decoded_image_bytes",
            actual: 4_000_000_000_000,
            maximum: 524_288_000,
        })
    ));
}

#[test]
fn rejects_cumulative_decoded_image_budget_before_attempting_decode() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("cumulative-images.pdf");
    fs::write(&source, image_pdf(&[(100, 100), (100, 100)])).unwrap();
    let mut request = request_for(source);
    request.limits.max_input_bytes = 50_000;

    assert!(matches!(
        extract_pdf(&request),
        Err(ConversionError::LimitExceeded {
            limit: "pdf_decoded_image_bytes",
            actual: 80_000,
            maximum: 50_000,
        })
    ));
}

#[test]
fn empty_user_password_encryption_is_rejected_after_successful_load() {
    let Some(qpdf) = qpdf() else {
        eprintln!("SKIP: qpdf is unavailable; empty-user encryption regression not run");
        return;
    };
    let temp = tempdir().unwrap();
    let encrypted = encrypt_fixture(&qpdf, &temp, "", "owner-password", "empty-user.pdf");

    assert!(matches!(
        extract_pdf(&request_for(encrypted)),
        Err(ConversionError::EncryptedInput)
    ));
}

#[test]
fn password_required_aes256_is_rejected_as_encrypted_input() {
    let Some(qpdf) = qpdf() else {
        eprintln!("SKIP: qpdf is unavailable; password-required encryption regression not run");
        return;
    };
    let temp = tempdir().unwrap();
    let encrypted = encrypt_fixture(
        &qpdf,
        &temp,
        "user-password",
        "owner-password",
        "password-required.pdf",
    );

    assert!(matches!(
        extract_pdf(&request_for(encrypted)),
        Err(ConversionError::EncryptedInput)
    ));
}

fn blank_pdf() -> Vec<u8> {
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] >>".to_vec(),
    ];
    pdf_with_objects(&objects)
}

fn image_pdf(dimensions: &[(u32, u32)]) -> Vec<u8> {
    let names = (0..dimensions.len())
        .map(|index| format!("/I{} {} 0 R", index + 1, index + 5))
        .collect::<Vec<_>>()
        .join(" ");
    let content = (0..dimensions.len())
        .map(|index| format!("q 1 0 0 1 {} 0 cm /I{} Do Q", index, index + 1))
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /XObject << {names} >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        stream_object(&content),
    ];
    for (width, height) in dimensions {
        objects.push(
            format!(
                "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length 3 >>\nstream\n"
            )
            .into_bytes()
            .into_iter()
            .chain([0, 0, 0])
            .chain(b"\nendstream".iter().copied())
            .collect(),
        );
    }
    pdf_with_objects(&objects)
}

fn stream_object(data: &[u8]) -> Vec<u8> {
    format!("<< /Length {} >>\nstream\n", data.len())
        .into_bytes()
        .into_iter()
        .chain(data.iter().copied())
        .chain(b"\nendstream".iter().copied())
        .collect()
}

fn pdf_with_objects(objects: &[Vec<u8>]) -> Vec<u8> {
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

fn rotated_fixture(degrees: i16) -> Vec<u8> {
    let base = fs::read(workspace_path("tests/fixtures/pdf/digital-basic.pdf")).unwrap();
    let xref = find_bytes(&base, b"xref\n").unwrap();
    let mut objects = base[..xref].to_vec();
    let media_box = find_bytes(&objects, b"/MediaBox [0 0 300 400]").unwrap();
    let replacement = format!("/MediaBox [0 0 400 500] /Rotate {degrees}");
    objects.splice(
        media_box..media_box + b"/MediaBox [0 0 300 400]".len(),
        replacement.bytes(),
    );

    let mut offsets = Vec::new();
    for number in 1..=11 {
        offsets.push(find_bytes(&objects, format!("{number} 0 obj\n").as_bytes()).unwrap());
    }
    let xref = objects.len();
    objects.extend_from_slice(b"xref\n0 12\n0000000000 65535 f \n");
    for offset in offsets {
        objects.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    objects.extend_from_slice(
        format!(
            "trailer\n<< /Size 12 /Root 1 0 R /Info 9 0 R /ID [<00112233445566778899AABBCCDDEEFF><00112233445566778899AABBCCDDEEFF>] >>\nstartxref\n{xref}\n%%EOF\n"
        )
        .as_bytes(),
    );
    objects
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn assert_rect_within_page(rect: &RawRect, width: f32, height: f32) {
    assert!(rect.left >= -TOLERANCE, "left out of page: {rect:?}");
    assert!(rect.top >= -TOLERANCE, "top out of page: {rect:?}");
    assert!(
        rect.right <= width + TOLERANCE,
        "right out of page: {rect:?}"
    );
    assert!(
        rect.bottom <= height + TOLERANCE,
        "bottom out of page: {rect:?}"
    );
}

fn qpdf() -> Option<PathBuf> {
    let output = Command::new("qpdf").arg("--version").output().ok()?;
    output.status.success().then(|| PathBuf::from("qpdf"))
}

fn encrypt_fixture(
    qpdf: &Path,
    temp: &tempfile::TempDir,
    user_password: &str,
    owner_password: &str,
    name: &str,
) -> PathBuf {
    let output = temp.path().join(name);
    let status = Command::new(qpdf)
        .args(["--encrypt", user_password, owner_password, "256", "--"])
        .arg(workspace_path("tests/fixtures/pdf/digital-basic.pdf"))
        .arg(&output)
        .status()
        .expect("qpdf should start after availability check");
    assert!(status.success(), "qpdf encryption should succeed");
    output
}
