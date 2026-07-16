pub mod result;

use std::{
    ffi::{OsStr, OsString},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use mdconvert_core::{
    Cancellation, ConversionError, ConversionLimits, ConversionRequest, Converter, OutputError,
    OutputTarget, OverwritePolicy, WarningCode, publish,
};
use mdconvert_formats::{
    DetectionError, DocxConverter, EpubConverter, ImageConverter, JsonConverter, LocalFormat,
    PptxConverter, StructuredFormat, XlsxConverter, XmlConverter, ZipConverter, detect_format,
    local_v1_formats,
};
use mdconvert_html::HtmlConverter;
use mdconvert_pdf::PdfConverter;
use result::ResultEnvelope;

pub const EXIT_SUCCESS: u8 = 0;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_INPUT: u8 = 3;
pub const EXIT_CONVERSION: u8 = 4;
pub const EXIT_OUTPUT: u8 = 5;
pub const EXIT_CANCELLED: u8 = 6;

#[derive(Debug)]
struct CliError {
    code: &'static str,
    message: String,
    exit_code: u8,
}

impl CliError {
    fn new(code: &'static str, message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            code,
            message: message.into(),
            exit_code,
        }
    }
}

#[derive(Debug)]
struct Options {
    input: PathBuf,
    output: PathBuf,
    assets: Option<PathBuf>,
    cancel_file: Option<PathBuf>,
}

pub fn run<I, S>(arguments: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let arguments = arguments.into_iter().map(Into::into).collect::<Vec<_>>();
    let json = arguments.iter().any(|argument| argument == "--json");
    match parse_options(&arguments).and_then(execute) {
        Ok(envelope) => {
            if json {
                if write_json(stdout, &envelope).is_err() {
                    return EXIT_OUTPUT;
                }
            } else if let Some(path) = envelope.markdown_path.as_deref() {
                let _ = writeln!(stdout, "Converted to {}", path.display());
                for warning in &envelope.warnings {
                    let _ = writeln!(stderr, "warning[{}]", warning_code(&warning.code));
                }
            }
            EXIT_SUCCESS
        }
        Err(error) => {
            if json {
                let _ = write_json(stderr, &ResultEnvelope::failed(error.code, &error.message));
            } else {
                let _ = writeln!(stderr, "error[{}]: {}", error.code, error.message);
            }
            error.exit_code
        }
    }
}

fn warning_code(code: &WarningCode) -> &'static str {
    match code {
        WarningCode::AmbiguousReadingOrder => "ambiguous_reading_order",
        WarningCode::TableDegraded => "table_degraded",
        WarningCode::FontMetadataInsufficient => "font_metadata_insufficient",
        WarningCode::MissingImageAlt => "missing_image_alt",
        WarningCode::InvalidLinkSkipped => "invalid_link_skipped",
        WarningCode::InvalidAssetSkipped => "invalid_asset_skipped",
        WarningCode::ExternalAssetSkipped => "external_asset_skipped",
        WarningCode::ExternalLinkSkipped => "external_link_skipped",
        WarningCode::AdditionalArchiveEntriesSkipped => "additional_archive_entries_skipped",
        WarningCode::OcrDeferred => "ocr_deferred",
    }
}

fn write_json(writer: &mut dyn Write, envelope: &ResultEnvelope) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, envelope).map_err(std::io::Error::other)?;
    writer.write_all(b"\n")
}

fn parse_options(arguments: &[OsString]) -> Result<Options, CliError> {
    let usage = || {
        CliError::new(
            "invalid_arguments",
            "usage: mdconvert convert <INPUT> --output <FILE.md> [--assets <DIR>] [--json] [--cancel-file <PATH>]",
            EXIT_USAGE,
        )
    };
    if arguments.first().map(OsString::as_os_str) != Some(OsStr::new("convert")) {
        return Err(usage());
    }
    let Some(input) = arguments.get(1) else {
        return Err(usage());
    };
    if input.to_string_lossy().starts_with("--") {
        return Err(usage());
    }

    let mut output = None;
    let mut assets = None;
    let mut cancel_file = None;
    let mut json = false;
    let mut index = 2;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--output") if output.is_none() => {
                index += 1;
                output = arguments.get(index).map(PathBuf::from);
                if output.is_none() {
                    return Err(usage());
                }
            }
            Some("--assets") if assets.is_none() => {
                index += 1;
                assets = arguments.get(index).map(PathBuf::from);
                if assets.is_none() {
                    return Err(usage());
                }
            }
            Some("--cancel-file") if cancel_file.is_none() => {
                index += 1;
                cancel_file = arguments.get(index).map(PathBuf::from);
                if cancel_file.is_none() {
                    return Err(usage());
                }
            }
            Some("--json") if !json => json = true,
            _ => return Err(usage()),
        }
        index += 1;
    }

    Ok(Options {
        input: PathBuf::from(input),
        output: output.ok_or_else(usage)?,
        assets,
        cancel_file,
    })
}

fn execute(options: Options) -> Result<ResultEnvelope, CliError> {
    if looks_like_url(&options.input) {
        return Err(CliError::new(
            "network_input_unsupported",
            "only local file inputs are supported",
            EXIT_INPUT,
        ));
    }
    let input = validate_input(&options.input)?;
    let (output, derived_assets) =
        validate_output(&options.output, options.assets.as_deref(), &input)?;
    let cancellation = FileCancellation(options.cancel_file);
    cancellation.check()?;

    let bytes = fs::read(&input)
        .map_err(|_| CliError::new("input_unreadable", "input could not be read", EXIT_INPUT))?;
    cancellation.check()?;
    let format = detect_local_format(&input, &bytes)?;
    cancellation.check()?;

    let request = ConversionRequest::new(&input)
        .map_err(|_| CliError::new("invalid_input", "input path is invalid", EXIT_INPUT))?;
    let document = convert(format, &request).map_err(map_conversion_error)?;
    cancellation.check()?;
    if !document.assets.is_empty() && derived_assets.exists() {
        return Err(CliError::new(
            "output_exists",
            "assets output already exists",
            EXIT_OUTPUT,
        ));
    }
    let metadata = document.metadata.clone();
    cancellation.check()?;
    let written = publish(
        &document,
        &OutputTarget {
            markdown_path: output,
            overwrite: OverwritePolicy::Deny,
        },
        &cancellation,
    )
    .map_err(map_output_error)?;

    Ok(ResultEnvelope::succeeded(
        written.markdown_path,
        written.assets_dir,
        metadata,
        written.warnings,
    ))
}

fn looks_like_url(path: &Path) -> bool {
    path.to_str().is_some_and(|value| {
        let lower = value.to_ascii_lowercase();
        lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("file://")
    })
}

fn validate_input(path: &Path) -> Result<PathBuf, CliError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CliError::new("input_not_found", "input file does not exist", EXIT_INPUT)
        } else {
            CliError::new(
                "input_unreadable",
                "input could not be inspected",
                EXIT_INPUT,
            )
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(CliError::new(
            "input_symlink",
            "symlink inputs are not supported",
            EXIT_INPUT,
        ));
    }
    if !metadata.is_file() {
        return Err(CliError::new(
            "input_not_regular",
            "input must be a regular file",
            EXIT_INPUT,
        ));
    }
    if metadata.len() > ConversionLimits::default().max_input_bytes {
        return Err(CliError::new(
            "limit_exceeded",
            "input exceeds a conversion limit",
            EXIT_CONVERSION,
        ));
    }
    fs::canonicalize(path).map_err(|_| {
        CliError::new(
            "input_unreadable",
            "input could not be resolved",
            EXIT_INPUT,
        )
    })
}

fn validate_output(
    requested: &Path,
    requested_assets: Option<&Path>,
    input: &Path,
) -> Result<(PathBuf, PathBuf), CliError> {
    if requested.extension().and_then(OsStr::to_str) != Some("md") {
        return Err(CliError::new(
            "invalid_output",
            "output must have the lowercase .md extension",
            EXIT_USAGE,
        ));
    }
    let name = requested
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| CliError::new("invalid_output", "output path is invalid", EXIT_USAGE))?;
    if name == "." || name == ".." {
        return Err(CliError::new(
            "invalid_output",
            "output path is invalid",
            EXIT_USAGE,
        ));
    }
    let parent = requested
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|_| {
        CliError::new(
            "invalid_output",
            "output parent must be an existing directory",
            EXIT_USAGE,
        )
    })?;
    if !parent.is_dir() {
        return Err(CliError::new(
            "invalid_output",
            "output parent must be a directory",
            EXIT_USAGE,
        ));
    }
    let output = parent.join(name);
    let derived_assets = output.with_extension("assets");
    if output == input || input.starts_with(&derived_assets) {
        return Err(CliError::new(
            "source_output_alias",
            "output and assets must not alias or contain the input",
            EXIT_USAGE,
        ));
    }
    if fs::symlink_metadata(&output).is_ok() {
        return Err(CliError::new(
            "output_exists",
            "Markdown output already exists",
            EXIT_OUTPUT,
        ));
    }
    if fs::symlink_metadata(&derived_assets).is_ok() {
        return Err(CliError::new(
            "output_exists",
            "assets output already exists",
            EXIT_OUTPUT,
        ));
    }
    if let Some(requested_assets) = requested_assets {
        let normalized = normalize_future_path(requested_assets)?;
        if normalized != derived_assets {
            return Err(CliError::new(
                "invalid_assets_path",
                "--assets must equal the output path with its extension replaced by .assets",
                EXIT_USAGE,
            ));
        }
    }
    Ok((output, derived_assets))
}

fn normalize_future_path(path: &Path) -> Result<PathBuf, CliError> {
    let name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            CliError::new("invalid_assets_path", "assets path is invalid", EXIT_USAGE)
        })?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|_| {
        CliError::new(
            "invalid_assets_path",
            "assets parent must exist",
            EXIT_USAGE,
        )
    })?;
    Ok(parent.join(name))
}

fn extension_format(path: &Path) -> Option<LocalFormat> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    local_v1_formats()
        .iter()
        .copied()
        .find(|format| format.extensions().contains(&extension.as_str()))
}

fn detect_local_format(path: &Path, bytes: &[u8]) -> Result<LocalFormat, CliError> {
    let extension = extension_format(path);
    if let Some(format @ (LocalFormat::Csv | LocalFormat::Json | LocalFormat::Xml)) = extension {
        return detect_format(path, bytes)
            .map(structured_local_format)
            .map_err(map_detection_error)
            .and_then(|actual| {
                if actual == format {
                    Ok(format)
                } else {
                    Err(format_conflict())
                }
            });
    }

    let pdf = bytes.starts_with(b"%PDF-");
    let png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
    let jpeg = bytes.starts_with(&[0xff, 0xd8]);
    let zip = bytes.starts_with(b"PK\x03\x04");
    let html = looks_like_html(bytes);
    if let Some(format) = extension {
        let matches = match format {
            LocalFormat::Pdf => pdf,
            LocalFormat::Png => png,
            LocalFormat::Jpeg => jpeg,
            LocalFormat::Html => html,
            LocalFormat::Zip
            | LocalFormat::Epub
            | LocalFormat::Docx
            | LocalFormat::Pptx
            | LocalFormat::Xlsx => zip,
            LocalFormat::Csv | LocalFormat::Json | LocalFormat::Xml => unreachable!(),
        };
        return matches.then_some(format).ok_or_else(format_conflict);
    }
    if pdf {
        return Ok(LocalFormat::Pdf);
    }
    if png {
        return Ok(LocalFormat::Png);
    }
    if jpeg {
        return Ok(LocalFormat::Jpeg);
    }
    if zip {
        return Err(CliError::new(
            "ambiguous_format",
            "ZIP-container input requires a .zip, .epub, .docx, .pptx, or .xlsx extension",
            EXIT_INPUT,
        ));
    }
    if html {
        return Ok(LocalFormat::Html);
    }
    match detect_format(Path::new(""), bytes) {
        Ok(format) => Ok(structured_local_format(format)),
        Err(error) => Err(map_detection_error(error)),
    }
}

fn looks_like_html(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes)
        .ok()
        .map(str::trim_start)
        .map(str::to_ascii_lowercase)
        .is_some_and(|text| {
            [
                "<!doctype html",
                "<!--",
                "<html",
                "<head",
                "<body",
                "<title",
                "<meta",
                "<link",
                "<main",
                "<article",
                "<section",
                "<nav",
                "<aside",
                "<header",
                "<footer",
                "<div",
                "<p",
                "<h1",
                "<h2",
                "<h3",
                "<h4",
                "<h5",
                "<h6",
                "<ul",
                "<ol",
                "<table",
                "<blockquote",
                "<pre",
            ]
            .iter()
            .any(|prefix| text.starts_with(prefix))
        })
}

fn structured_local_format(format: StructuredFormat) -> LocalFormat {
    match format {
        StructuredFormat::Csv => LocalFormat::Csv,
        StructuredFormat::Json => LocalFormat::Json,
        StructuredFormat::Xml => LocalFormat::Xml,
    }
}

fn format_conflict() -> CliError {
    CliError::new(
        "format_conflict",
        "input extension conflicts with its validated content",
        EXIT_INPUT,
    )
}

fn map_detection_error(error: DetectionError) -> CliError {
    let code = match error {
        DetectionError::Ambiguous { .. } => "ambiguous_format",
        DetectionError::Conflict { .. } => "format_conflict",
        DetectionError::LimitExceeded { .. } => "limit_exceeded",
        DetectionError::InvalidUtf8 { .. } | DetectionError::Unsupported => "unknown_format",
    };
    CliError::new(
        code,
        "input format could not be determined safely",
        EXIT_INPUT,
    )
}

fn convert(
    format: LocalFormat,
    request: &ConversionRequest,
) -> Result<mdconvert_core::Document, ConversionError> {
    match format {
        LocalFormat::Pdf => PdfConverter.convert(request),
        LocalFormat::Html => HtmlConverter.convert(request),
        LocalFormat::Csv => mdconvert_formats::CsvConverter.convert(request),
        LocalFormat::Json => JsonConverter.convert(request),
        LocalFormat::Xml => XmlConverter.convert(request),
        LocalFormat::Zip => ZipConverter.convert(request),
        LocalFormat::Epub => EpubConverter.convert(request),
        LocalFormat::Docx => DocxConverter.convert(request),
        LocalFormat::Pptx => PptxConverter.convert(request),
        LocalFormat::Xlsx => XlsxConverter.convert(request),
        LocalFormat::Png | LocalFormat::Jpeg => ImageConverter.convert(request),
    }
}

fn map_conversion_error(error: ConversionError) -> CliError {
    let (code, message) = match error {
        ConversionError::InvalidRequest(_) => ("invalid_request", "conversion request is invalid"),
        ConversionError::Io { .. } => ("input_io", "input could not be read"),
        ConversionError::UnsupportedFormat { .. } => {
            ("unsupported_format", "input format is not supported")
        }
        ConversionError::UnsupportedInput { .. } => {
            ("unsupported_input", "input uses an unsupported feature")
        }
        ConversionError::CorruptInput { .. } => {
            ("corrupt_input", "input is corrupt or structurally invalid")
        }
        ConversionError::EncryptedInput => ("encrypted_input", "encrypted input is not supported"),
        ConversionError::LimitExceeded { .. } => {
            ("limit_exceeded", "input exceeds a conversion limit")
        }
        ConversionError::OcrRequired => ("ocr_required", "OCR is required to convert this input"),
        ConversionError::ConversionFailed { ref message }
            if message.contains("PDFIUM_DYNAMIC_LIB_PATH") || message.contains("PDFium") =>
        {
            (
                "pdfium_unavailable",
                "the pinned PDFium runtime is unavailable",
            )
        }
        ConversionError::ConversionFailed { .. } => ("conversion_failed", "conversion failed"),
    };
    CliError::new(code, message, EXIT_CONVERSION)
}

fn map_output_error(error: OutputError) -> CliError {
    match error {
        OutputError::Cancelled => {
            CliError::new("cancelled", "conversion was cancelled", EXIT_CANCELLED)
        }
        OutputError::OutputExists(_) | OutputError::UnownedAssetsDirectory(_) => {
            CliError::new("output_exists", "output already exists", EXIT_OUTPUT)
        }
        OutputError::InvalidTarget(_)
        | OutputError::InvalidAssetFileName(_)
        | OutputError::DuplicateAssetFileName(_)
        | OutputError::InvalidManifest { .. } => {
            CliError::new("invalid_output", "output target is invalid", EXIT_OUTPUT)
        }
        OutputError::Emit(_) => {
            CliError::new("emit_failed", "Markdown emission failed", EXIT_OUTPUT)
        }
        OutputError::Io { .. } | OutputError::TransactionFailed { .. } => {
            CliError::new("output_io", "output publication failed", EXIT_OUTPUT)
        }
    }
}

struct FileCancellation(Option<PathBuf>);

impl FileCancellation {
    fn check(&self) -> Result<(), CliError> {
        if self.is_cancelled() {
            Err(CliError::new(
                "cancelled",
                "conversion was cancelled",
                EXIT_CANCELLED,
            ))
        } else {
            Ok(())
        }
    }
}

impl Cancellation for FileCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.as_deref().is_some_and(|path| {
            fs::symlink_metadata(path).map_or_else(
                |error| error.kind() != std::io::ErrorKind::NotFound,
                |_| true,
            )
        })
    }
}
