pub mod result;

use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use mdconvert_core::{
    Cancellation, ConversionError, ConversionLimits, ConversionRequest, OutputError, OutputTarget,
    OverwritePolicy, WarningCode, publish,
};
use mdconvert_formats::{
    DetectionError, DocxConverter, EpubConverter, ImageConverter, JsonConverter, LocalFormat,
    PptxConverter, StructuredFormat, XlsxConverter, XmlConverter, ZipConverter, detect_format,
    local_v1_formats,
};
use mdconvert_html::{HtmlConverter, detect_html};
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
    let json = standalone_json_mode(&arguments);
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
                output = arguments
                    .get(index)
                    .filter(|value| !is_flag_like(value))
                    .map(PathBuf::from);
                if output.is_none() {
                    return Err(usage());
                }
            }
            Some("--assets") if assets.is_none() => {
                index += 1;
                assets = arguments
                    .get(index)
                    .filter(|value| !is_flag_like(value))
                    .map(PathBuf::from);
                if assets.is_none() {
                    return Err(usage());
                }
            }
            Some("--cancel-file") if cancel_file.is_none() => {
                index += 1;
                cancel_file = arguments
                    .get(index)
                    .filter(|value| !is_flag_like(value))
                    .map(PathBuf::from);
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

fn is_recognized_flag(argument: &OsStr) -> bool {
    matches!(
        argument.to_str(),
        Some("--output" | "--assets" | "--json" | "--cancel-file")
    )
}

fn is_flag_like(argument: &OsStr) -> bool {
    argument.to_string_lossy().starts_with("--")
}

fn standalone_json_mode(arguments: &[OsString]) -> bool {
    let mut index = if arguments
        .get(1)
        .is_some_and(|value| is_recognized_flag(value))
    {
        1
    } else {
        2
    };
    let mut json = false;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--output" | "--assets" | "--cancel-file") => index += 2,
            Some("--json") => {
                json = true;
                index += 1;
            }
            _ => index += 1,
        }
    }
    json
}

fn execute(options: Options) -> Result<ResultEnvelope, CliError> {
    validate_path_syntax(&options.input)?;
    validate_path_syntax(&options.output)?;
    if let Some(path) = options.assets.as_deref() {
        validate_path_syntax(path)?;
    }
    if let Some(path) = options.cancel_file.as_deref() {
        validate_path_syntax(path)?;
    }
    let cancellation = FileCancellation(options.cancel_file);
    cancellation.check()?;
    let opened = read_local_input(&options.input, ConversionLimits::default().max_input_bytes)?;
    let input = opened.path;
    let bytes = opened.bytes;
    let (output, derived_assets) =
        validate_output(&options.output, options.assets.as_deref(), &input)?;
    cancellation.check()?;
    let format = detect_local_format(&input, &bytes)?;
    cancellation.check()?;

    let request = ConversionRequest::new(&input)
        .map_err(|_| CliError::new("invalid_input", "input path is invalid", EXIT_INPUT))?;
    let document = convert(format, bytes, &request).map_err(map_conversion_error)?;
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

fn validate_path_syntax(path: &Path) -> Result<(), CliError> {
    let Some(value) = path.to_str() else {
        return Ok(());
    };
    let slash = value.replace('\\', "/");
    let lower = slash.to_ascii_lowercase();
    let drive_prefixed = lower.as_bytes().get(1) == Some(&b':')
        && lower
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);
    let drive_relative = drive_prefixed
        && lower
            .as_bytes()
            .get(2)
            .is_none_or(|separator| *separator != b'/');
    let foreign_drive = drive_prefixed && !cfg!(windows);
    let scheme = lower.find("://").is_some_and(|separator| {
        separator > 0
            && lower[..separator]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
    });
    let platform_network_prefix = cfg!(unix)
        && (lower.starts_with("/network/servers/")
            || lower.starts_with("/net/")
            || lower.starts_with("/afs/"));
    let unsafe_syntax = lower.starts_with("//")
        || lower.starts_with("/??/")
        || platform_network_prefix
        || drive_relative
        || foreign_drive
        || scheme;
    if unsafe_syntax {
        return Err(CliError::new(
            "unsafe_path",
            "network, device, or foreign drive path syntax is not supported",
            EXIT_USAGE,
        ));
    }
    Ok(())
}

struct OpenedInput {
    path: PathBuf,
    bytes: Vec<u8>,
}

fn read_local_input(path: &Path, maximum: u64) -> Result<OpenedInput, CliError> {
    read_local_input_with_hook(path, maximum, || {})
}

fn read_local_input_with_hook(
    path: &Path,
    maximum: u64,
    after_open: impl FnOnce(),
) -> Result<OpenedInput, CliError> {
    let name = path
        .file_name()
        .ok_or_else(|| CliError::new("input_not_found", "input file does not exist", EXIT_INPUT))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CliError::new("input_not_found", "input file does not exist", EXIT_INPUT)
        } else {
            CliError::new("input_unreadable", "input could not be opened", EXIT_INPUT)
        }
    })?;
    let normalized = parent.join(name);
    let mut file = open_input_no_follow(&normalized).map_err(|error| {
        if is_no_follow_error(&error) {
            CliError::new(
                "input_symlink",
                "symlink inputs are not supported",
                EXIT_INPUT,
            )
        } else if error.kind() == std::io::ErrorKind::NotFound {
            CliError::new("input_not_found", "input file does not exist", EXIT_INPUT)
        } else {
            CliError::new("input_unreadable", "input could not be opened", EXIT_INPUT)
        }
    })?;
    let before = file.metadata().map_err(|_| {
        CliError::new(
            "input_unreadable",
            "input could not be inspected",
            EXIT_INPUT,
        )
    })?;
    if is_reparse_or_symlink(&before) {
        return Err(CliError::new(
            "input_symlink",
            "symlink inputs are not supported",
            EXIT_INPUT,
        ));
    }
    if !before.is_file() {
        return Err(CliError::new(
            "input_not_regular",
            "input must be a regular file",
            EXIT_INPUT,
        ));
    }
    if before.len() > maximum {
        return Err(CliError::new(
            "limit_exceeded",
            "input exceeds a conversion limit",
            EXIT_CONVERSION,
        ));
    }

    after_open();
    let capacity = usize::try_from(before.len()).unwrap_or(usize::MAX);
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(capacity).map_err(|_| {
        CliError::new(
            "input_unreadable",
            "input could not be buffered",
            EXIT_INPUT,
        )
    })?;
    (&mut file)
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| CliError::new("input_unreadable", "input could not be read", EXIT_INPUT))?;
    let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual > maximum {
        return Err(CliError::new(
            "limit_exceeded",
            "input exceeds a conversion limit",
            EXIT_CONVERSION,
        ));
    }
    let after = file.metadata().map_err(|_| {
        CliError::new(
            "input_unreadable",
            "input could not be verified",
            EXIT_INPUT,
        )
    })?;
    if !same_file_identity(&before, &after)
        || actual != before.len()
        || after.len() != before.len()
        || after.modified().ok() != before.modified().ok()
    {
        return Err(CliError::new(
            "input_changed",
            "input changed while it was being read",
            EXIT_INPUT,
        ));
    }
    Ok(OpenedInput {
        path: normalized,
        bytes,
    })
}

fn open_input_no_follow(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}

fn is_no_follow_error(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    if error.raw_os_error() == Some(libc::ELOOP) {
        return true;
    }
    false
}

#[cfg(unix)]
fn same_file_identity(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    before.dev() == after.dev() && before.ino() == after.ino()
}

#[cfg(windows)]
fn same_file_identity(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    before.volume_serial_number() == after.volume_serial_number()
        && before.file_index() == after.file_index()
}

#[cfg(not(any(unix, windows)))]
fn same_file_identity(_before: &fs::Metadata, _after: &fs::Metadata) -> bool {
    true
}

#[cfg(windows)]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
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
    let html = if pdf || png || jpeg || zip {
        false
    } else {
        detect_html(
            bytes,
            mdconvert_core::ConversionLimits::default().max_input_bytes,
        )
        .unwrap_or(false)
    };
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
    let exit_code = if code == "limit_exceeded" {
        EXIT_CONVERSION
    } else {
        EXIT_INPUT
    };
    CliError::new(
        code,
        "input format could not be determined safely",
        exit_code,
    )
}

fn convert(
    format: LocalFormat,
    bytes: Vec<u8>,
    request: &ConversionRequest,
) -> Result<mdconvert_core::Document, ConversionError> {
    match format {
        LocalFormat::Pdf => PdfConverter.convert_bytes(&bytes, request),
        LocalFormat::Html => HtmlConverter.convert_bytes(&bytes, request),
        LocalFormat::Csv => mdconvert_formats::CsvConverter.convert_bytes(&bytes, request),
        LocalFormat::Json => JsonConverter.convert_bytes(&bytes, request),
        LocalFormat::Xml => XmlConverter.convert_bytes(&bytes, request),
        LocalFormat::Zip => ZipConverter.convert_bytes(&bytes, request),
        LocalFormat::Epub => EpubConverter.convert_bytes(&bytes, request),
        LocalFormat::Docx => DocxConverter.convert_bytes(&bytes, request),
        LocalFormat::Pptx => PptxConverter.convert_bytes(&bytes, request),
        LocalFormat::Xlsx => XlsxConverter.convert_bytes(&bytes, request),
        LocalFormat::Png | LocalFormat::Jpeg => ImageConverter.convert_owned_bytes(bytes, request),
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
        ConversionError::PdfiumUnavailable => (
            "pdfium_unavailable",
            "the pinned PDFium runtime is unavailable",
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_path_syntax_validator_rejects_network_devices_and_foreign_drives() {
        let mut paths = vec![
            r"\\server\share\file",
            "//server/share/file",
            r"\\?\C:\file",
            r"\\.\PhysicalDrive0",
            r"\??\C:\file",
            r"C:file",
            "smb://server/share/file",
        ];
        #[cfg(unix)]
        paths.extend([
            r"C:\file",
            "/Network/Servers/share/file",
            "/net/server/file",
            "/afs/example/file",
        ]);
        for path in paths {
            let error = validate_path_syntax(Path::new(path)).unwrap_err();
            assert_eq!(error.code, "unsafe_path", "path {path:?}");
        }
    }

    #[test]
    fn pure_path_syntax_validator_accepts_local_relative_and_absolute_paths() {
        for path in ["document.pdf", "folder/document.pdf", "/tmp/document.pdf"] {
            assert!(
                validate_path_syntax(Path::new(path)).is_ok(),
                "path {path:?}"
            );
        }
        #[cfg(windows)]
        assert!(validate_path_syntax(Path::new(r"C:\local\document.pdf")).is_ok());
    }

    #[test]
    fn pdfium_mapping_depends_on_the_typed_error_not_message_text() {
        let unavailable = map_conversion_error(ConversionError::PdfiumUnavailable);
        assert_eq!(unavailable.code, "pdfium_unavailable");

        let ordinary_failure = map_conversion_error(ConversionError::ConversionFailed {
            message: "PDFium unavailable while processing page geometry".into(),
        });
        assert_eq!(ordinary_failure.code, "conversion_failed");
    }

    #[test]
    fn metadata_identity_distinguishes_distinct_open_files() {
        let directory = tempfile::TempDir::new().unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        fs::write(&first, b"same length").unwrap();
        fs::write(&second, b"same length").unwrap();

        assert!(!same_file_identity(
            &fs::metadata(first).unwrap(),
            &fs::metadata(second).unwrap()
        ));
    }

    #[test]
    fn opened_input_bytes_are_stable_when_the_path_is_replaced_after_open() {
        let directory = tempfile::TempDir::new().unwrap();
        let source = directory.path().join("source.html");
        fs::write(&source, b"<!doctype html><p>original bytes</p>").unwrap();

        let opened = read_local_input_with_hook(
            &source,
            ConversionLimits::default().max_input_bytes,
            || {
                fs::remove_file(&source).unwrap();
                fs::write(&source, b"<!doctype html><p>replacement bytes</p>").unwrap();
            },
        )
        .unwrap();

        assert!(
            String::from_utf8(opened.bytes)
                .unwrap()
                .contains("original bytes")
        );
        assert!(
            fs::read_to_string(source)
                .unwrap()
                .contains("replacement bytes")
        );
    }
}
