use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(unix)]
use std::ffi::OsString;

use mdconvert_core::{ConversionRequest, Document};
use mdconvert_formats::{
    CsvConverter, DocxConverter, EpubConverter, ImageConverter, JsonConverter, PptxConverter,
    XlsxConverter, XmlConverter, ZipConverter,
};
use mdconvert_html::HtmlConverter;
#[cfg(target_os = "macos")]
use mdconvert_pdf::PdfConverter;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "mdconvert-cli-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_mdconvert")
}

fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(relative)
}

fn command() -> Command {
    Command::new(binary())
}

fn json_value(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).expect("stream should contain a JSON envelope")
}

fn assert_failed_json(output: &std::process::Output, code: &str, exit_code: i32) {
    assert_eq!(output.status.code(), Some(exit_code));
    assert!(
        output.stdout.is_empty(),
        "JSON failures must leave stdout empty"
    );
    let value = json_value(&output.stderr);
    assert_eq!(value["schema_version"], "mdviewer.convert/v1");
    assert_eq!(value["status"], "failed");
    assert!(value["markdown_path"].is_null());
    assert!(value["assets_path"].is_null());
    assert_eq!(value["metadata"], serde_json::json!({}));
    assert_eq!(value["warnings"], serde_json::json!([]));
    assert_eq!(value["error"]["code"], code);
    assert_eq!(value.as_object().unwrap().len(), 7);
}

fn png_without_semantic_metadata() -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, 300, 300);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![255; 300 * 300])
            .unwrap();
    }
    output
}

#[test]
fn converts_html_and_prints_the_versioned_json_result() {
    let temp = TestDir::new();
    let output_path = temp.path().join("document.md");

    let output = command()
        .args(["convert"])
        .arg(fixture("html/semantic.html"))
        .args(["--output"])
        .arg(&output_path)
        .arg("--json")
        .output()
        .expect("mdconvert should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(value["schema_version"], "mdviewer.convert/v1");
    assert_eq!(value["status"], "succeeded");
    assert_eq!(
        value["markdown_path"],
        fs::canonicalize(&output_path)
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    assert!(value["assets_path"].is_null());
    assert_eq!(value["metadata"]["source_format"], "html");
    assert_eq!(value["warnings"][0]["code"], "external_asset_skipped");
    assert_eq!(value.as_object().unwrap().len(), 6);
    assert!(output.stderr.is_empty());
    assert!(output_path.is_file());
    assert!(!temp.path().join("document.assets").exists());
}

#[test]
fn human_mode_reports_paths_without_document_contents() {
    let temp = TestDir::new();
    let input = temp.path().join("private.csv");
    let output_path = temp.path().join("human.md");
    fs::write(&input, "name,value\nTOP-SECRET-CONTENT,7\n").unwrap();

    let output = command()
        .args(["convert"])
        .arg(&input)
        .args(["--output"])
        .arg(&output_path)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.starts_with("Converted to "));
    assert!(stdout.contains(output_path.file_name().unwrap().to_str().unwrap()));
    assert!(!stdout.contains("TOP-SECRET-CONTENT"));
    assert!(!stderr.contains("TOP-SECRET-CONTENT"));
    assert!(stderr.is_empty());
}

#[test]
fn human_warning_diagnostics_do_not_echo_authored_urls() {
    let temp = TestDir::new();
    let input = temp.path().join("warning.html");
    let output_path = temp.path().join("warning.md");
    let secret = "DO-NOT-LOG-SECRET-URL-TOKEN";
    fs::write(
        &input,
        format!("<!doctype html><img src=\"https://example.test/{secret}\" alt=\"image\">"),
    )
    .unwrap();

    let output = command()
        .args(["convert"])
        .arg(&input)
        .args(["--output"])
        .arg(&output_path)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stdout.contains(secret));
    assert!(!stderr.contains(secret));
    assert_eq!(stderr, "warning[external_asset_skipped]\n");
}

#[test]
fn dispatches_every_local_v1_registry_format() {
    let fixtures = [
        #[cfg(target_os = "macos")]
        ("pdf/digital-basic.pdf", "pdf"),
        ("html/semantic.html", "html"),
        ("formats/sample.csv", "csv"),
        ("formats/sample.json", "json"),
        ("formats/sample.xml", "xml"),
        ("formats/bounded.zip", "zip"),
        ("formats/spine.epub", "epub"),
        ("formats/semantic.docx", "docx"),
        ("formats/ordered.pptx", "pptx"),
        ("formats/displayed.xlsx", "xlsx"),
        ("formats/metadata.png", "png"),
        ("formats/metadata.jpg", "jpeg"),
    ];
    let temp = TestDir::new();
    for (index, (input, expected_format)) in fixtures.iter().enumerate() {
        let output_path = temp.path().join(format!("format-{index}.md"));
        let output = command()
            .args(["convert"])
            .arg(fixture(input))
            .args(["--output"])
            .arg(&output_path)
            .arg("--json")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{input} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value = json_value(&output.stdout);
        assert_eq!(value["metadata"]["source_format"], *expected_format);
        assert!(output_path.is_file());
    }
}

#[test]
fn every_registry_converter_accepts_the_same_owned_bytes_after_source_removal() {
    fn removed_source(temp: &TestDir, relative: &str) -> (Vec<u8>, ConversionRequest) {
        let original = fixture(relative);
        let source = temp.path().join(original.file_name().unwrap());
        fs::copy(&original, &source).unwrap();
        let bytes = fs::read(&source).unwrap();
        fs::remove_file(&source).unwrap();
        (bytes, ConversionRequest::new(source).unwrap())
    }

    fn source_format(document: Document) -> String {
        document.metadata.source_format.unwrap()
    }

    let temp = TestDir::new();
    #[cfg(target_os = "macos")]
    {
        let (bytes, request) = removed_source(&temp, "pdf/digital-basic.pdf");
        assert_eq!(
            source_format(PdfConverter.convert_bytes(&bytes, &request).unwrap()),
            "pdf"
        );
    }
    let (bytes, request) = removed_source(&temp, "html/semantic.html");
    assert_eq!(
        source_format(HtmlConverter.convert_bytes(&bytes, &request).unwrap()),
        "html"
    );
    let (bytes, request) = removed_source(&temp, "formats/sample.csv");
    assert_eq!(
        source_format(CsvConverter.convert_bytes(&bytes, &request).unwrap()),
        "csv"
    );
    let (bytes, request) = removed_source(&temp, "formats/sample.json");
    assert_eq!(
        source_format(JsonConverter.convert_bytes(&bytes, &request).unwrap()),
        "json"
    );
    let (bytes, request) = removed_source(&temp, "formats/sample.xml");
    assert_eq!(
        source_format(XmlConverter.convert_bytes(&bytes, &request).unwrap()),
        "xml"
    );
    let (bytes, request) = removed_source(&temp, "formats/bounded.zip");
    assert!(
        source_format(ZipConverter.convert_bytes(&bytes, &request).unwrap()).starts_with("zip")
    );
    let (bytes, request) = removed_source(&temp, "formats/spine.epub");
    assert_eq!(
        source_format(EpubConverter.convert_bytes(&bytes, &request).unwrap()),
        "epub"
    );
    let (bytes, request) = removed_source(&temp, "formats/semantic.docx");
    assert_eq!(
        source_format(DocxConverter.convert_bytes(&bytes, &request).unwrap()),
        "docx"
    );
    let (bytes, request) = removed_source(&temp, "formats/ordered.pptx");
    assert_eq!(
        source_format(PptxConverter.convert_bytes(&bytes, &request).unwrap()),
        "pptx"
    );
    let (bytes, request) = removed_source(&temp, "formats/displayed.xlsx");
    assert_eq!(
        source_format(XlsxConverter.convert_bytes(&bytes, &request).unwrap()),
        "xlsx"
    );
    let (bytes, request) = removed_source(&temp, "formats/metadata.png");
    assert_eq!(
        source_format(ImageConverter.convert_bytes(&bytes, &request).unwrap()),
        "png"
    );
    let (bytes, request) = removed_source(&temp, "formats/metadata.jpg");
    assert_eq!(
        source_format(ImageConverter.convert_bytes(&bytes, &request).unwrap()),
        "jpeg"
    );
}

#[test]
fn image_success_publishes_required_assets_and_local_ocr_no_text_warning() {
    let temp = TestDir::new();
    let input = temp.path().join("pixel.png");
    fs::write(&input, png_without_semantic_metadata()).unwrap();
    let output_path = temp.path().join("image.md");
    let assets_path = temp.path().join("image.assets");

    let output = command()
        .args(["convert"])
        .arg(&input)
        .args(["--output"])
        .arg(&output_path)
        .args(["--assets"])
        .arg(&assets_path)
        .arg("--json")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_value(&output.stdout);
    assert_eq!(
        value["assets_path"],
        fs::canonicalize(&assets_path)
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(value["warnings"][0]["code"], "ocr_no_text_found");
    assert!(assets_path.join("image-001.png").is_file());
    assert!(assets_path.join(".mdviewer-assets.json").is_file());
}

#[test]
fn explicit_assets_must_match_the_transactional_derived_path() {
    let temp = TestDir::new();
    let output_path = temp.path().join("image.md");
    let unrelated_assets = temp.path().join("somewhere-else");

    let output = command()
        .args(["convert"])
        .arg(fixture("formats/metadata.png"))
        .args(["--output"])
        .arg(&output_path)
        .args(["--assets"])
        .arg(&unrelated_assets)
        .arg("--json")
        .output()
        .unwrap();

    assert_failed_json(&output, "invalid_assets_path", 2);
    assert!(!output_path.exists());
    assert!(!unrelated_assets.exists());
}

#[test]
fn valid_explicit_assets_path_is_not_created_when_document_has_no_assets() {
    let temp = TestDir::new();
    let input = temp.path().join("fragment.html");
    let output_path = temp.path().join("fragment.md");
    let assets_path = temp.path().join("fragment.assets");
    fs::write(&input, "<p>A valid HTML fragment</p>").unwrap();

    let output = command()
        .args(["convert"])
        .arg(&input)
        .args(["--output"])
        .arg(&output_path)
        .args(["--assets"])
        .arg(&assets_path)
        .arg("--json")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_value(&output.stdout);
    assert!(value["assets_path"].is_null());
    assert!(output_path.is_file());
    assert!(!assets_path.exists());
}

#[test]
fn unknown_ambiguous_and_conflicting_formats_are_typed() {
    let temp = TestDir::new();
    let unknown = temp.path().join("unknown.bin");
    fs::write(&unknown, "not a recognized local format").unwrap();
    let unknown_output = command()
        .args(["convert"])
        .arg(&unknown)
        .args(["--output"])
        .arg(temp.path().join("unknown.md"))
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&unknown_output, "unknown_format", 3);

    let ambiguous = temp.path().join("package.bin");
    fs::copy(fixture("formats/bounded.zip"), &ambiguous).unwrap();
    let ambiguous_output = command()
        .args(["convert"])
        .arg(&ambiguous)
        .args(["--output"])
        .arg(temp.path().join("ambiguous.md"))
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&ambiguous_output, "ambiguous_format", 3);

    let conflict = temp.path().join("conflict.json");
    fs::copy(fixture("formats/sample.xml"), &conflict).unwrap();
    let conflict_output = command()
        .args(["convert"])
        .arg(&conflict)
        .args(["--output"])
        .arg(temp.path().join("conflict.md"))
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&conflict_output, "format_conflict", 3);
}

#[test]
fn missing_input_and_invalid_arguments_are_stable_json_failures() {
    let temp = TestDir::new();
    let missing = command()
        .args(["convert"])
        .arg(temp.path().join("missing.html"))
        .args(["--output"])
        .arg(temp.path().join("missing.md"))
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&missing, "input_not_found", 3);

    let invalid = command().args(["convert", "--json"]).output().unwrap();
    assert_failed_json(&invalid, "invalid_arguments", 2);
}

#[test]
fn oversized_input_is_rejected_before_detection_or_publication() {
    let temp = TestDir::new();
    let input = temp.path().join("oversized.pdf");
    let file = fs::File::create(&input).unwrap();
    file.set_len(500 * 1024 * 1024 + 1).unwrap();
    let markdown = temp.path().join("oversized.md");

    let output = command()
        .args(["convert"])
        .arg(&input)
        .args(["--output"])
        .arg(&markdown)
        .arg("--json")
        .output()
        .unwrap();

    assert_failed_json(&output, "limit_exceeded", 4);
    assert!(!markdown.exists());
}

#[test]
fn refuses_existing_markdown_or_assets_without_modifying_them() {
    let temp = TestDir::new();
    let markdown = temp.path().join("existing.md");
    fs::write(&markdown, "KEEP MARKDOWN").unwrap();
    let output = command()
        .args(["convert"])
        .arg(fixture("html/semantic.html"))
        .args(["--output"])
        .arg(&markdown)
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&output, "output_exists", 5);
    assert_eq!(fs::read_to_string(&markdown).unwrap(), "KEEP MARKDOWN");

    let markdown = temp.path().join("assets-collision.md");
    let assets = temp.path().join("assets-collision.assets");
    fs::create_dir(&assets).unwrap();
    fs::write(assets.join("personal.txt"), "KEEP ASSET").unwrap();
    let output = command()
        .args(["convert"])
        .arg(fixture("html/semantic.html"))
        .args(["--output"])
        .arg(&markdown)
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&output, "output_exists", 5);
    assert!(!markdown.exists());
    assert_eq!(
        fs::read_to_string(assets.join("personal.txt")).unwrap(),
        "KEEP ASSET"
    );
}

#[test]
fn concurrent_no_clobber_publications_allow_only_one_winner() {
    let temp = TestDir::new();
    let markdown = temp.path().join("race.md");
    let mut first = command();
    first
        .args(["convert"])
        .arg(fixture("formats/metadata.png"))
        .args(["--output"])
        .arg(&markdown)
        .arg("--json");
    let mut second = command();
    second
        .args(["convert"])
        .arg(fixture("formats/metadata.png"))
        .args(["--output"])
        .arg(&markdown)
        .arg("--json");

    first.stdout(Stdio::piped()).stderr(Stdio::piped());
    second.stdout(Stdio::piped()).stderr(Stdio::piped());
    let first = first.spawn().unwrap();
    let second = second.spawn().unwrap();
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();

    let codes = [first.status.code(), second.status.code()];
    assert!(codes.contains(&Some(0)));
    assert!(codes.contains(&Some(5)));
    assert!(markdown.is_file());
    assert!(
        temp.path()
            .join("race.assets/.mdviewer-assets.json")
            .is_file()
    );
}

#[test]
fn preexisting_cancel_file_stops_before_conversion_and_leaves_no_partials() {
    let temp = TestDir::new();
    let markdown = temp.path().join("cancelled.md");
    let cancel = temp.path().join("cancel.now");
    fs::write(&cancel, "cancel").unwrap();

    let output = command()
        .args(["convert"])
        .arg(fixture("formats/metadata.png"))
        .args(["--output"])
        .arg(&markdown)
        .args(["--cancel-file"])
        .arg(&cancel)
        .arg("--json")
        .output()
        .unwrap();

    assert_failed_json(&output, "cancelled", 6);
    assert!(!markdown.exists());
    assert!(!temp.path().join("cancelled.assets").exists());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[cfg(target_os = "macos")]
#[test]
fn scanned_pdf_returns_ocr_required_without_outputs() {
    let temp = TestDir::new();
    let markdown = temp.path().join("scanned.md");
    let output = command()
        .args(["convert"])
        .arg(fixture("pdf/scanned.pdf"))
        .args(["--output"])
        .arg(&markdown)
        .arg("--json")
        .output()
        .unwrap();

    assert_failed_json(&output, "ocr_required", 4);
    assert!(!markdown.exists());
    assert!(!temp.path().join("scanned.assets").exists());
}

#[test]
fn absent_pdfium_is_a_typed_failure() {
    let temp = TestDir::new();
    let markdown = temp.path().join("pdf.md");
    let output = command()
        .env_remove("PDFIUM_DYNAMIC_LIB_PATH")
        .args(["convert"])
        .arg(fixture("pdf/digital-basic.pdf"))
        .args(["--output"])
        .arg(&markdown)
        .arg("--json")
        .output()
        .unwrap();

    assert_failed_json(&output, "pdfium_unavailable", 4);
    assert!(!markdown.exists());
}

#[test]
fn unicode_input_and_output_paths_round_trip_as_absolute_json_paths() {
    let temp = TestDir::new();
    let input = temp.path().join("résumé-文件.json");
    let markdown = temp.path().join("salida-ñ-文件.md");
    fs::write(&input, r#"{"saludo":"hola"}"#).unwrap();

    let output = command()
        .args(["convert"])
        .arg(&input)
        .args(["--output"])
        .arg(&markdown)
        .arg("--json")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_value(&output.stdout);
    assert_eq!(
        value["markdown_path"],
        fs::canonicalize(&markdown)
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
}

#[test]
fn failures_never_echo_hostile_document_contents() {
    let temp = TestDir::new();
    let input = temp.path().join("corrupt.pdf");
    let secret = "DO-NOT-LOG-THIS-DOCUMENT-SECRET";
    fs::write(&input, format!("%PDF-1.7\n{secret}\nnot actually a PDF")).unwrap();
    let output = command()
        .env_remove("PDFIUM_DYNAMIC_LIB_PATH")
        .args(["convert"])
        .arg(&input)
        .args(["--output"])
        .arg(temp.path().join("corrupt.md"))
        .arg("--json")
        .output()
        .unwrap();

    assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
}

#[test]
fn local_only_interface_rejects_urls() {
    let temp = TestDir::new();
    let output = command()
        .args([
            "convert",
            "https://example.test/private-document",
            "--output",
        ])
        .arg(temp.path().join("network.md"))
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&output, "unsafe_path", 2);
}

#[test]
fn rejects_network_device_and_foreign_drive_syntax_for_every_path_argument() {
    let temp = TestDir::new();
    let valid_input = fixture("html/semantic.html");
    let hostile = vec![
        r"\\server\share\document.pdf",
        "//server/share/document.pdf",
        r"\\?\C:\document.pdf",
        r"\\.\PhysicalDrive0",
        r"\??\C:\document.pdf",
        r"C:document.pdf",
        "smb://server/share/document.pdf",
        "NUL",
        "CON.md",
        "folder/con .md",
        "folder/AUX.txt",
        "nested/PRN... ",
        "CLOCK$",
        "CONIN$",
        "conout$.md",
        "nested/CoNiN$.txt... ",
        "nested/cOnOuT$ .log",
        "COM1.log",
        "COM¹.txt",
        "LPT9",
        "LPT².md",
        r"\?\GLOBALROOT\Device\HarddiskVolume1\document.pdf",
        r"\Device\HarddiskVolume1\document.pdf",
    ];
    #[cfg(unix)]
    let hostile = {
        let mut hostile = hostile;
        hostile.extend([
            r"C:\document.pdf",
            "/Network/Servers/share/document.pdf",
            "/net/server/document.pdf",
        ]);
        hostile
    };
    #[cfg(windows)]
    let hostile = {
        let mut hostile = hostile;
        hostile.extend(["normal/file:stream", r"C:\dir\file:stream"]);
        hostile
    };

    for (index, path) in hostile.iter().enumerate() {
        let input_result = command()
            .args(["convert", path, "--output"])
            .arg(temp.path().join(format!("input-{index}.md")))
            .arg("--json")
            .output()
            .unwrap();
        assert_failed_json(&input_result, "unsafe_path", 2);

        let output_result = command()
            .args(["convert"])
            .arg(&valid_input)
            .args(["--output", path, "--json"])
            .output()
            .unwrap();
        assert_failed_json(&output_result, "unsafe_path", 2);

        let assets_result = command()
            .args(["convert"])
            .arg(&valid_input)
            .args(["--output"])
            .arg(temp.path().join(format!("assets-{index}.md")))
            .args(["--assets", path, "--json"])
            .output()
            .unwrap();
        assert_failed_json(&assets_result, "unsafe_path", 2);

        let cancel_result = command()
            .args(["convert"])
            .arg(&valid_input)
            .args(["--output"])
            .arg(temp.path().join(format!("cancel-{index}.md")))
            .args(["--cancel-file", path, "--json"])
            .output()
            .unwrap();
        assert_failed_json(&cancel_result, "unsafe_path", 2);
    }
}

#[test]
fn compatibility_character_basenames_are_not_dos_device_aliases() {
    let temp = TestDir::new();
    let input = temp.path().join("ＣＯＮ.json");
    let output_path = temp.path().join("COM①.md");
    let assets = temp.path().join("COM①.assets");
    let cancellation = temp.path().join("ＣＯＭ1.cancel");
    fs::write(&input, r#"{"status":"local"}"#).unwrap();

    let output = command()
        .args(["convert"])
        .arg(&input)
        .args(["--output"])
        .arg(&output_path)
        .args(["--assets"])
        .arg(&assets)
        .args(["--cancel-file"])
        .arg(&cancellation)
        .arg("--json")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        json_value(&output.stdout)["metadata"]["source_format"],
        "json"
    );
    assert!(output_path.is_file());
    assert!(!assets.exists());
}

#[cfg(unix)]
#[test]
fn local_colon_paths_are_valid_on_unix() {
    let temp = TestDir::new();
    let input = temp.path().join("report:2026.json");
    let output_path = temp.path().join("report:2026.md");
    let assets = temp.path().join("report:2026.assets");
    let cancellation = temp.path().join("cancel:marker");
    fs::write(&input, r#"{"year":2026,"markup":"<span>local</span>"}"#).unwrap();

    let output = command()
        .args(["convert"])
        .arg(&input)
        .args(["--output"])
        .arg(&output_path)
        .args(["--assets"])
        .arg(&assets)
        .args(["--cancel-file"])
        .arg(&cancellation)
        .arg("--json")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        json_value(&output.stdout)["metadata"]["source_format"],
        "json"
    );
    assert!(output_path.is_file());
    assert!(!assets.exists());
}

#[cfg(unix)]
#[test]
fn non_utf8_path_arguments_fail_before_lookup_or_publication() {
    use std::os::unix::ffi::OsStringExt;

    let temp = TestDir::new();
    let valid_input = fixture("html/semantic.html");
    let hostile = temp
        .path()
        .join(OsString::from_vec(b"not-unicode-\xff.md".to_vec()));

    let cases = [
        (hostile.as_os_str(), "input"),
        (hostile.as_os_str(), "output"),
        (hostile.as_os_str(), "assets"),
        (hostile.as_os_str(), "cancel"),
    ];
    for (index, (path, kind)) in cases.into_iter().enumerate() {
        let output_path = temp.path().join(format!("unicode-{index}.md"));
        let mut process = command();
        process.arg("convert");
        if kind == "input" {
            process.arg(path);
        } else {
            process.arg(&valid_input);
        }
        process.arg("--output");
        if kind == "output" {
            process.arg(path);
        } else {
            process.arg(&output_path);
        }
        if kind == "assets" {
            process.arg("--assets").arg(path);
        }
        if kind == "cancel" {
            process.arg("--cancel-file").arg(path);
        }
        let output = process.arg("--json").output().unwrap();
        assert_failed_json(&output, "unsafe_path", 2);
        assert!(!output_path.exists());
    }
}

#[test]
fn option_values_cannot_be_recognized_flags_and_consumed_json_is_not_json_mode() {
    let temp = TestDir::new();
    let input = fixture("html/semantic.html");
    for arguments in [
        vec!["--output", "--json"],
        vec!["--assets", "--json"],
        vec!["--cancel-file", "--json"],
    ] {
        let output = command()
            .args(["convert"])
            .arg(&input)
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "error[invalid_arguments]: usage: mdconvert convert <INPUT> --output <FILE.md> [--assets <DIR>] [--json] [--cancel-file <PATH>]\n"
        );
    }
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn unknown_flag_like_option_values_are_invalid_arguments() {
    let input = fixture("html/semantic.html");
    for option in ["--output", "--assets", "--cancel-file"] {
        let output = command()
            .args(["convert"])
            .arg(&input)
            .args([option, "--not-a-value"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .starts_with("error[invalid_arguments]:")
        );
    }
}

#[test]
fn duplicate_and_unknown_options_are_invalid_arguments() {
    let temp = TestDir::new();
    let input = fixture("html/semantic.html");
    let output_path = temp.path().join("document.md");
    for extra in [
        vec!["--json", "--json"],
        vec!["--output", output_path.to_str().unwrap()],
        vec!["--assets", "document.assets", "--assets", "document.assets"],
        vec!["--cancel-file", "cancel", "--cancel-file", "cancel"],
        vec!["--unknown"],
    ] {
        let mut arguments = vec!["convert", input.to_str().unwrap(), "--output"];
        arguments.push(output_path.to_str().unwrap());
        arguments.extend(extra);
        arguments.push("--json");
        let output = command().args(arguments).output().unwrap();
        assert_failed_json(&output, "invalid_arguments", 2);
    }
    assert!(!output_path.exists());
}

#[test]
fn parser_backed_html_detection_accepts_fragments_custom_elements_and_comments() {
    let temp = TestDir::new();
    for (index, html) in [
        "<span>inline fragment</span>",
        "<widget-card>custom element</widget-card>",
        "<!-- authored comment -->comment-only fragment",
        "<!doctype html><html></html>",
    ]
    .iter()
    .enumerate()
    {
        let input = temp.path().join(format!("fragment-{index}.html"));
        let output_path = temp.path().join(format!("fragment-{index}.md"));
        fs::write(&input, html).unwrap();
        let output = command()
            .args(["convert"])
            .arg(&input)
            .args(["--output"])
            .arg(&output_path)
            .arg("--json")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{html:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let incompatible = temp.path().join("not-html.html");
    fs::copy(fixture("pdf/digital-basic.pdf"), &incompatible).unwrap();
    let output = command()
        .args(["convert"])
        .arg(&incompatible)
        .args(["--output"])
        .arg(temp.path().join("not-html.md"))
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&output, "format_conflict", 3);
}

#[test]
fn strong_and_structured_detection_precede_html_signals_deterministically() {
    let temp = TestDir::new();
    for (index, (content, expected)) in [
        (r#"{"markup":"<span>inside JSON</span>"}"#, "json"),
        ("name,markup\nAlice,<b>inside CSV</b>\n", "csv"),
    ]
    .into_iter()
    .enumerate()
    {
        let input = temp.path().join(format!("extensionless-{index}"));
        let output_path = temp.path().join(format!("extensionless-{index}.md"));
        fs::write(&input, content).unwrap();
        let output = command()
            .args(["convert"])
            .arg(&input)
            .args(["--output"])
            .arg(&output_path)
            .arg("--json")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            json_value(&output.stdout)["metadata"]["source_format"],
            expected
        );
    }

    for (index, (fixture_path, extension)) in [
        ("pdf/digital-basic.pdf", "json"),
        ("formats/metadata.png", "csv"),
        ("formats/bounded.zip", "xml"),
    ]
    .into_iter()
    .enumerate()
    {
        let input = temp.path().join(format!("strong-{index}.{extension}"));
        fs::copy(fixture(fixture_path), &input).unwrap();
        let output_path = temp.path().join(format!("strong-{index}.md"));
        let output = command()
            .args(["convert"])
            .arg(&input)
            .args(["--output"])
            .arg(&output_path)
            .arg("--json")
            .output()
            .unwrap();
        assert_failed_json(&output, "format_conflict", 3);
        assert!(!output_path.exists());
    }

    let empty_zip = temp.path().join("empty-zip.json");
    let mut empty_zip_bytes = b"PK\x05\x06".to_vec();
    empty_zip_bytes.extend_from_slice(&[0; 18]);
    fs::write(&empty_zip, empty_zip_bytes).unwrap();
    let empty_zip_output = temp.path().join("empty-zip.md");
    let output = command()
        .args(["convert"])
        .arg(&empty_zip)
        .args(["--output"])
        .arg(&empty_zip_output)
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&output, "format_conflict", 3);
    assert!(!empty_zip_output.exists());
}

#[test]
fn hardlink_output_and_assets_aliases_are_rejected_as_source_aliases() {
    let temp = TestDir::new();
    let input = temp.path().join("source.html");
    fs::write(&input, "<!doctype html><p>source</p>").unwrap();

    for (index, alias_assets) in [false, true].into_iter().enumerate() {
        let output_path = temp.path().join(format!("hardlink-{index}.md"));
        let alias = if alias_assets {
            output_path.with_extension("assets")
        } else {
            output_path.clone()
        };
        fs::hard_link(&input, &alias).unwrap();
        let output = command()
            .args(["convert"])
            .arg(&input)
            .args(["--output"])
            .arg(&output_path)
            .arg("--json")
            .output()
            .unwrap();
        assert_failed_json(&output, "source_output_alias", 2);
        assert_eq!(
            fs::read_to_string(&input).unwrap(),
            "<!doctype html><p>source</p>"
        );
        fs::remove_file(alias).unwrap();
    }
}

#[test]
fn existing_equivalent_assets_spelling_uses_source_alias_taxonomy_when_supported() {
    let temp = TestDir::new();
    for (index, (actual_name, requested_name)) in [
        ("Alias.assets", "alias.assets"),
        ("Café.assets", "Cafe\u{301}.assets"),
    ]
    .into_iter()
    .enumerate()
    {
        let actual_assets = temp.path().join(actual_name);
        fs::create_dir(&actual_assets).unwrap();
        let input = actual_assets.join(format!("source-{index}.html"));
        fs::write(&input, "<!doctype html><p>source</p>").unwrap();
        let requested_assets = temp.path().join(requested_name);
        if fs::canonicalize(&requested_assets).is_err() {
            fs::remove_file(&input).unwrap();
            fs::remove_dir(&actual_assets).unwrap();
            continue;
        }
        let output_path = requested_assets.with_extension("md");
        let output = command()
            .args(["convert"])
            .arg(&input)
            .args(["--output"])
            .arg(&output_path)
            .arg("--json")
            .output()
            .unwrap();
        assert_failed_json(&output, "source_output_alias", 2);
        assert_eq!(
            fs::read_to_string(&input).unwrap(),
            "<!doctype html><p>source</p>"
        );
    }
}

#[test]
fn structured_detection_depth_limit_uses_conversion_exit_code() {
    let temp = TestDir::new();
    for (depth, expected_code) in [(127, 0), (129, 4)] {
        let input = temp.path().join(format!("depth-{depth}.bin"));
        let output_path = temp.path().join(format!("depth-{depth}.md"));
        fs::write(
            &input,
            format!("{}0{}", "[".repeat(depth), "]".repeat(depth)),
        )
        .unwrap();
        let output = command()
            .args(["convert"])
            .arg(&input)
            .args(["--output"])
            .arg(&output_path)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(expected_code));
        if expected_code == 4 {
            assert_failed_json(&output, "limit_exceeded", 4);
            assert!(!output_path.exists());
        } else {
            assert!(output_path.is_file());
        }
    }
}

#[cfg(unix)]
#[test]
fn rejects_symlink_nonregular_and_source_inside_assets_inputs() {
    use std::os::unix::fs::symlink;

    let temp = TestDir::new();
    let real = temp.path().join("real.html");
    let link = temp.path().join("link.html");
    fs::write(&real, "<!doctype html><p>local</p>").unwrap();
    symlink(&real, &link).unwrap();
    let output = command()
        .args(["convert"])
        .arg(&link)
        .args(["--output"])
        .arg(temp.path().join("link.md"))
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&output, "input_symlink", 3);

    let directory = temp.path().join("directory.html");
    fs::create_dir(&directory).unwrap();
    let output = command()
        .args(["convert"])
        .arg(&directory)
        .args(["--output"])
        .arg(temp.path().join("directory.md"))
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&output, "input_not_regular", 3);

    let assets = temp.path().join("alias.assets");
    fs::create_dir(&assets).unwrap();
    let nested_input = assets.join("source.html");
    fs::write(&nested_input, "<!doctype html><p>nested</p>").unwrap();
    let output = command()
        .args(["convert"])
        .arg(&nested_input)
        .args(["--output"])
        .arg(temp.path().join("alias.md"))
        .arg("--json")
        .output()
        .unwrap();
    assert_failed_json(&output, "source_output_alias", 2);
}
