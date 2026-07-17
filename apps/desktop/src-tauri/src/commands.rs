use std::{ffi::OsString, fs, io::Read, path::Path};

use mdconvert_core::ConversionLimits;
use serde::Serialize;
use tauri::State;
use url::Url;

use crate::{
    jobs::{JobError, PrintJobId},
    macos_integration::{IntegrationError, IntegrationStatus as MacosWorkflowStatus},
    state::{AppState, SelectionAccess, StateError},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    fn new(code: impl Into<String>) -> Self {
        let code = code.into();
        let message = match code.as_str() {
            "invalid_token" => "selection token is invalid",
            "access_denied" => "selection does not grant this operation",
            "invalid_selection" => "selected path is no longer valid",
            "source_changed" => "selected source changed after authorization",
            "scope_changed" => "selected destination changed after authorization",
            "recovery_required" => "publication rollback requires private artifact recovery",
            "invalid_operation_id" => "conversion ID is invalid",
            "conversion_already_running" => "conversion is already running",
            "conversion_not_running" => "conversion is not running",
            "cancelled" => "conversion was cancelled",
            "invalid_job_id" => "print job ID is invalid",
            "job_not_found" => "print job was not found",
            "already_claimed" => "print job was already claimed",
            "invalid_job_metadata" => "print job metadata is invalid",
            "invalid_external_url" => "external URL is not allowed",
            "invalid_export_format" => "export format is not allowed",
            _ => "operation could not be completed",
        };
        Self {
            code,
            message: message.to_owned(),
        }
    }
}

impl From<StateError> for CommandError {
    fn from(error: StateError) -> Self {
        Self::new(error.code())
    }
}

impl From<JobError> for CommandError {
    fn from(error: JobError) -> Self {
        Self::new(error.code())
    }
}

impl From<IntegrationError> for CommandError {
    fn from(error: IntegrationError) -> Self {
        Self::new(error.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenDocumentResult {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveDocumentResult {
    pub saved: bool,
    pub write_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenSelectionResult {
    pub name: String,
    pub read_token: String,
    pub write_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveSelectionResult {
    pub name: String,
    pub write_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConversionResult {
    pub operation_id: String,
    #[serde(skip_serializing)]
    pub markdown_path: String,
    #[serde(skip_serializing)]
    pub assets_path: Option<String>,
    pub warning_codes: Vec<String>,
    pub markdown_token: String,
    pub write_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClaimedPrintJob {
    pub id: String,
    pub title: String,
    pub created_unix_ms: u64,
    pub source_token: String,
}

pub fn authorize_open_selection(
    state: &AppState,
    path: &Path,
    writable: bool,
) -> Result<OpenSelectionResult, CommandError> {
    let name = selected_name(path)?;
    let read_token = state.authorize_user_selection(path, SelectionAccess::Read)?;
    let write_token = if writable {
        Some(state.authorize_user_selection(path, SelectionAccess::Write)?)
    } else {
        None
    };
    Ok(OpenSelectionResult {
        name,
        read_token,
        write_token,
    })
}

pub fn authorize_save_selection(
    state: &AppState,
    path: &Path,
) -> Result<SaveSelectionResult, CommandError> {
    let name = selected_name(path)?;
    let write_token = state.authorize_user_selection(path, SelectionAccess::Write)?;
    Ok(SaveSelectionResult { name, write_token })
}

pub fn authorize_export_selection(
    state: &AppState,
    path: &Path,
    format: &str,
) -> Result<SaveSelectionResult, CommandError> {
    if format != "html" {
        return Err(CommandError::new("invalid_export_format"));
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
    {
        return Err(CommandError::new("invalid_selection"));
    }
    authorize_save_selection(state, path)
}

fn selected_name(path: &Path) -> Result<String, CommandError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CommandError::new("invalid_selection"))
}

pub fn sanitized_markdown_name(value: &str) -> String {
    use unicode_segmentation::UnicodeSegmentation;

    let mut result = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    while result.ends_with(['.', ' ']) {
        result.pop();
    }
    if result
        .get(result.len().saturating_sub(3)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".md"))
    {
        result.truncate(result.len() - 3);
        while result.ends_with(['.', ' ']) {
            result.pop();
        }
    }
    if result.is_empty() {
        result.push_str("Documento");
    }

    let mut bounded = String::new();
    for grapheme in result.graphemes(true).take(117) {
        if bounded.len() + grapheme.len() + 3 > 240 {
            break;
        }
        bounded.push_str(grapheme);
    }
    if bounded.is_empty() {
        bounded.push_str("Documento");
    }
    bounded.push_str(".md");
    bounded
}

pub fn sanitized_export_name(value: &str, format: &str) -> Result<String, CommandError> {
    if format != "html" {
        return Err(CommandError::new("invalid_export_format"));
    }
    let value = value.trim();
    let without_html = value
        .get(value.len().saturating_sub(5)..)
        .filter(|suffix| suffix.eq_ignore_ascii_case(".html"))
        .map_or(value, |_| &value[..value.len() - 5]);
    let without_markdown = without_html
        .get(without_html.len().saturating_sub(3)..)
        .filter(|suffix| suffix.eq_ignore_ascii_case(".md"))
        .map_or(without_html, |_| &without_html[..without_html.len() - 3]);
    let markdown_name = sanitized_markdown_name(without_markdown);
    Ok(format!("{}.html", markdown_name.trim_end_matches(".md")))
}

pub fn validate_external_url(value: &str) -> Result<Url, CommandError> {
    let authority = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .filter(|authority| !authority.is_empty() && !authority.contains('@'))
        .ok_or_else(|| CommandError::new("invalid_external_url"))?;
    if authority.contains('\\') {
        return Err(CommandError::new("invalid_external_url"));
    }
    let url = Url::parse(value).map_err(|_| CommandError::new("invalid_external_url"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(CommandError::new("invalid_external_url"));
    }
    Ok(url)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntegrationStatus {
    pub deep_link_scheme: &'static str,
    pub print_jobs_available: bool,
    pub network_access: bool,
    pub pending_print_job_ids: Vec<String>,
}

pub fn open_document(state: &AppState, token: &str) -> Result<OpenDocumentResult, CommandError> {
    let selection = state.selection(token, SelectionAccess::Read)?;
    let mut file = selection
        .open_read()
        .map_err(|_| CommandError::new("open_failed"))?;
    let metadata = file
        .metadata()
        .map_err(|_| CommandError::new("open_failed"))?;
    selection.verify_handle(&file)?;
    if !metadata.is_file() || metadata.len() > ConversionLimits::default().max_input_bytes {
        return Err(CommandError::new("invalid_input"));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(ConversionLimits::default().max_input_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CommandError::new("open_failed"))?;
    selection.verify_handle(&file)?;
    let content = String::from_utf8(bytes).map_err(|_| CommandError::new("invalid_text"))?;
    Ok(OpenDocumentResult { content })
}

pub fn save_document(
    state: &AppState,
    token: &str,
    content: &str,
) -> Result<SaveDocumentResult, CommandError> {
    let selection = state.take_selection(token, SelectionAccess::Write)?;
    selection.persist_content(content.as_bytes())?;
    let write_token = state.renew_write_selection(&selection)?;
    Ok(SaveDocumentResult {
        saved: true,
        write_token,
    })
}

pub fn convert_document(
    state: &AppState,
    operation_id: &str,
    source_token: &str,
    output_token: &str,
) -> Result<ConversionResult, CommandError> {
    let cancel_marker = state.begin_conversion(operation_id)?;
    let result = (|| {
        let source = state.snapshot_source(source_token)?;
        let output = state.take_selection(output_token, SelectionAccess::Write)?;
        let staging = state.conversion_staging(operation_id, &output.path)?;
        let arguments = vec![
            OsString::from("convert"),
            source.path().as_os_str().to_owned(),
            OsString::from("--output"),
            staging.markdown_path().as_os_str().to_owned(),
            OsString::from("--json"),
            OsString::from("--cancel-file"),
            cancel_marker.into_os_string(),
        ];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit_code = mdconvert_cli::run(arguments, &mut stdout, &mut stderr);
        let payload = if exit_code == mdconvert_cli::EXIT_SUCCESS {
            serde_json::from_slice::<serde_json::Value>(&stdout)
        } else {
            serde_json::from_slice::<serde_json::Value>(&stderr)
        }
        .map_err(|_| CommandError::new("conversion_failed"))?;

        if exit_code != mdconvert_cli::EXIT_SUCCESS {
            let code = payload
                .pointer("/error/code")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("conversion_failed");
            return Err(CommandError::new(code));
        }

        let warning_codes = payload["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|warning| warning["code"].as_str())
            .map(stable_warning_code)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let assets_path = output.publish_conversion(&staging)?;
        let markdown_token = state.authorize_published_output(&output)?;
        let write_token = state.renew_write_selection(&output)?;
        state.record_warnings(operation_id, warning_codes.clone())?;
        Ok(ConversionResult {
            operation_id: operation_id.to_owned(),
            markdown_path: output.path.to_string_lossy().into_owned(),
            assets_path: assets_path.map(|path| path.to_string_lossy().into_owned()),
            warning_codes,
            markdown_token,
            write_token,
        })
    })();
    let end_result = state.end_conversion(operation_id);
    match (result, end_result) {
        (_, Err(error)) => Err(error.into()),
        (result, Ok(())) => result,
    }
}

pub async fn convert_document_async(
    state: AppState,
    operation_id: String,
    source_token: String,
    output_token: String,
) -> Result<ConversionResult, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        convert_document(&state, &operation_id, &source_token, &output_token)
    })
    .await
    .map_err(|_| CommandError::new("conversion_failed"))?
}

pub fn cancel_conversion(state: &AppState, operation_id: &str) -> Result<(), CommandError> {
    state.cancel_conversion(operation_id).map_err(Into::into)
}

pub fn warning_codes(state: &AppState, operation_id: &str) -> Result<Vec<String>, CommandError> {
    state.warning_codes(operation_id).map_err(Into::into)
}

pub fn claim_print_job(state: &AppState, id: &str) -> Result<ClaimedPrintJob, CommandError> {
    let id = PrintJobId::parse(id)?;
    let job = match state.jobs().claim(id) {
        Ok(job) => job,
        Err(JobError::AlreadyClaimed) => {
            state.dequeue_print_job(id)?;
            return Err(JobError::AlreadyClaimed.into());
        }
        Err(error) => return Err(error.into()),
    };
    state.dequeue_print_job(id)?;
    let source_token = match state.authorize_user_selection(&job.input_pdf, SelectionAccess::Read) {
        Ok(token) => token,
        Err(error) => {
            let _ = state.jobs().finish(id);
            return Err(error.into());
        }
    };
    Ok(ClaimedPrintJob {
        id: job.id.to_string(),
        title: job.title,
        created_unix_ms: job.created_unix_ms,
        source_token,
    })
}

pub fn finish_print_job(state: &AppState, id: &str) -> Result<(), CommandError> {
    let id = PrintJobId::parse(id)?;
    state.jobs().finish(id).map_err(Into::into)
}

pub fn integration_status(state: &AppState) -> Result<IntegrationStatus, CommandError> {
    let available = fs::symlink_metadata(state.jobs().root())
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false);
    Ok(IntegrationStatus {
        deep_link_scheme: "mdviewer",
        print_jobs_available: available,
        network_access: false,
        pending_print_job_ids: state
            .pending_print_jobs()?
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
    })
}

#[cfg(target_os = "macos")]
pub fn macos_workflow_status() -> Result<MacosWorkflowStatus, CommandError> {
    Ok(crate::macos_integration::embedded_manager()?.status()?)
}

#[cfg(not(target_os = "macos"))]
pub fn macos_workflow_status() -> Result<MacosWorkflowStatus, CommandError> {
    Ok(MacosWorkflowStatus::NotInstalled)
}

#[cfg(target_os = "macos")]
pub fn install_macos_workflow() -> Result<MacosWorkflowStatus, CommandError> {
    let manager = crate::macos_integration::embedded_manager()?;
    manager.install()?;
    Ok(manager.status()?)
}

#[cfg(not(target_os = "macos"))]
pub fn install_macos_workflow() -> Result<MacosWorkflowStatus, CommandError> {
    Err(CommandError::new("unsupported_platform"))
}

#[cfg(target_os = "macos")]
pub fn repair_macos_workflow() -> Result<MacosWorkflowStatus, CommandError> {
    let manager = crate::macos_integration::embedded_manager()?;
    manager.repair()?;
    Ok(manager.status()?)
}

#[cfg(not(target_os = "macos"))]
pub fn repair_macos_workflow() -> Result<MacosWorkflowStatus, CommandError> {
    Err(CommandError::new("unsupported_platform"))
}

#[cfg(target_os = "macos")]
pub fn uninstall_macos_workflow() -> Result<MacosWorkflowStatus, CommandError> {
    let manager = crate::macos_integration::embedded_manager()?;
    manager.uninstall()?;
    Ok(manager.status()?)
}

#[cfg(not(target_os = "macos"))]
pub fn uninstall_macos_workflow() -> Result<MacosWorkflowStatus, CommandError> {
    Err(CommandError::new("unsupported_platform"))
}

#[cfg(target_os = "macos")]
pub fn macos_virtual_printer_status() -> Result<MacosWorkflowStatus, CommandError> {
    Ok(crate::macos_virtual_printer::embedded_manager()?.status()?)
}

#[cfg(not(target_os = "macos"))]
pub fn macos_virtual_printer_status() -> Result<MacosWorkflowStatus, CommandError> {
    Ok(MacosWorkflowStatus::NotInstalled)
}

#[cfg(target_os = "macos")]
pub fn install_macos_virtual_printer() -> Result<MacosWorkflowStatus, CommandError> {
    let manager = crate::macos_virtual_printer::embedded_manager()?;
    manager.install()?;
    Ok(manager.status()?)
}

#[cfg(not(target_os = "macos"))]
pub fn install_macos_virtual_printer() -> Result<MacosWorkflowStatus, CommandError> {
    Err(CommandError::new("unsupported_platform"))
}

#[cfg(target_os = "macos")]
pub fn repair_macos_virtual_printer() -> Result<MacosWorkflowStatus, CommandError> {
    let manager = crate::macos_virtual_printer::embedded_manager()?;
    manager.repair()?;
    Ok(manager.status()?)
}

#[cfg(not(target_os = "macos"))]
pub fn repair_macos_virtual_printer() -> Result<MacosWorkflowStatus, CommandError> {
    Err(CommandError::new("unsupported_platform"))
}

#[cfg(target_os = "macos")]
pub fn uninstall_macos_virtual_printer() -> Result<MacosWorkflowStatus, CommandError> {
    let manager = crate::macos_virtual_printer::embedded_manager()?;
    manager.uninstall()?;
    Ok(manager.status()?)
}

#[cfg(not(target_os = "macos"))]
pub fn uninstall_macos_virtual_printer() -> Result<MacosWorkflowStatus, CommandError> {
    Err(CommandError::new("unsupported_platform"))
}

fn stable_warning_code(serialized: &str) -> &'static str {
    match serialized {
        "AmbiguousReadingOrder" => "ambiguous_reading_order",
        "TableDegraded" => "table_degraded",
        "FontMetadataInsufficient" => "font_metadata_insufficient",
        "MissingImageAlt" => "missing_image_alt",
        "InvalidLinkSkipped" => "invalid_link_skipped",
        "InvalidAssetSkipped" => "invalid_asset_skipped",
        "ExternalAssetSkipped" => "external_asset_skipped",
        "ExternalLinkSkipped" => "external_link_skipped",
        "AdditionalArchiveEntriesSkipped" => "additional_archive_entries_skipped",
        "OcrDeferred" => "ocr_deferred",
        "OcrNoTextFound" => "ocr_no_text_found",
        "OcrLowConfidence" => "ocr_low_confidence",
        _ => "unknown_warning",
    }
}

mod ipc {
    use super::*;

    #[tauri::command]
    pub(super) async fn select_open_document(
        state: State<'_, AppState>,
    ) -> Result<Option<OpenSelectionResult>, CommandError> {
        let Some(file) = rfd::AsyncFileDialog::new()
            .add_filter("Markdown", &["md", "markdown"])
            .pick_file()
            .await
        else {
            return Ok(None);
        };
        authorize_open_selection(&state, file.path(), true).map(Some)
    }

    #[tauri::command]
    pub(super) async fn select_conversion_source(
        state: State<'_, AppState>,
    ) -> Result<Option<OpenSelectionResult>, CommandError> {
        let Some(file) = rfd::AsyncFileDialog::new()
            .add_filter(
                "Documentos compatibles",
                &[
                    "pdf", "html", "htm", "csv", "json", "xml", "zip", "epub", "docx", "pptx",
                    "xlsx", "png", "jpg", "jpeg",
                ],
            )
            .pick_file()
            .await
        else {
            return Ok(None);
        };
        authorize_open_selection(&state, file.path(), false).map(Some)
    }

    #[tauri::command]
    pub(super) async fn select_save_document(
        state: State<'_, AppState>,
        suggested_name: String,
    ) -> Result<Option<SaveSelectionResult>, CommandError> {
        let name = sanitized_markdown_name(&suggested_name);
        let Some(file) = rfd::AsyncFileDialog::new()
            .add_filter("Markdown", &["md"])
            .set_file_name(name)
            .save_file()
            .await
        else {
            return Ok(None);
        };
        authorize_save_selection(&state, file.path()).map(Some)
    }

    #[tauri::command]
    pub(super) async fn select_export_document(
        state: State<'_, AppState>,
        suggested_name: String,
        format: String,
    ) -> Result<Option<SaveSelectionResult>, CommandError> {
        let name = sanitized_export_name(&suggested_name, &format)?;
        let Some(file) = rfd::AsyncFileDialog::new()
            .add_filter("HTML", &["html"])
            .set_file_name(name)
            .save_file()
            .await
        else {
            return Ok(None);
        };
        authorize_export_selection(&state, file.path(), &format).map(Some)
    }

    #[tauri::command]
    pub(super) fn open(
        state: State<'_, AppState>,
        token: String,
    ) -> Result<OpenDocumentResult, CommandError> {
        open_document(&state, &token)
    }

    #[tauri::command]
    pub(super) fn save(
        state: State<'_, AppState>,
        token: String,
        content: String,
    ) -> Result<SaveDocumentResult, CommandError> {
        save_document(&state, &token, &content)
    }

    #[tauri::command]
    pub(super) async fn convert(
        state: State<'_, AppState>,
        operation_id: String,
        source_token: String,
        output_token: String,
    ) -> Result<ConversionResult, CommandError> {
        convert_document_async(
            state.inner().clone(),
            operation_id,
            source_token,
            output_token,
        )
        .await
    }

    #[tauri::command]
    pub(super) fn cancel(
        state: State<'_, AppState>,
        operation_id: String,
    ) -> Result<(), CommandError> {
        cancel_conversion(&state, &operation_id)
    }

    #[tauri::command]
    pub(super) fn warnings(
        state: State<'_, AppState>,
        operation_id: String,
    ) -> Result<Vec<String>, CommandError> {
        warning_codes(&state, &operation_id)
    }

    #[tauri::command]
    pub(super) fn claim_print_job(
        state: State<'_, AppState>,
        id: String,
    ) -> Result<ClaimedPrintJob, CommandError> {
        super::claim_print_job(&state, &id)
    }

    #[tauri::command]
    pub(super) fn finish_print_job(
        state: State<'_, AppState>,
        id: String,
    ) -> Result<(), CommandError> {
        super::finish_print_job(&state, &id)
    }

    #[tauri::command]
    pub(super) fn open_external(url: String) -> Result<(), CommandError> {
        let url = validate_external_url(&url)?;
        open::that_detached(url.as_str()).map_err(|_| CommandError::new("external_open_failed"))
    }

    #[tauri::command]
    pub(super) fn integration_status(
        state: State<'_, AppState>,
    ) -> Result<IntegrationStatus, CommandError> {
        super::integration_status(&state)
    }

    #[tauri::command]
    pub(super) fn macos_workflow_status() -> Result<MacosWorkflowStatus, CommandError> {
        super::macos_workflow_status()
    }

    #[tauri::command]
    pub(super) fn install_macos_workflow() -> Result<MacosWorkflowStatus, CommandError> {
        super::install_macos_workflow()
    }

    #[tauri::command]
    pub(super) fn repair_macos_workflow() -> Result<MacosWorkflowStatus, CommandError> {
        super::repair_macos_workflow()
    }

    #[tauri::command]
    pub(super) fn uninstall_macos_workflow() -> Result<MacosWorkflowStatus, CommandError> {
        super::uninstall_macos_workflow()
    }

    #[tauri::command]
    pub(super) fn macos_virtual_printer_status() -> Result<MacosWorkflowStatus, CommandError> {
        super::macos_virtual_printer_status()
    }

    #[tauri::command]
    pub(super) fn install_macos_virtual_printer() -> Result<MacosWorkflowStatus, CommandError> {
        super::install_macos_virtual_printer()
    }

    #[tauri::command]
    pub(super) fn repair_macos_virtual_printer() -> Result<MacosWorkflowStatus, CommandError> {
        super::repair_macos_virtual_printer()
    }

    #[tauri::command]
    pub(super) fn uninstall_macos_virtual_printer() -> Result<MacosWorkflowStatus, CommandError> {
        super::uninstall_macos_virtual_printer()
    }
}

pub fn invoke_handler<R: tauri::Runtime>()
-> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        ipc::select_open_document,
        ipc::select_conversion_source,
        ipc::select_save_document,
        ipc::select_export_document,
        ipc::open,
        ipc::save,
        ipc::convert,
        ipc::cancel,
        ipc::warnings,
        ipc::claim_print_job,
        ipc::finish_print_job,
        ipc::open_external,
        ipc::integration_status,
        ipc::macos_workflow_status,
        ipc::install_macos_workflow,
        ipc::repair_macos_workflow,
        ipc::uninstall_macos_workflow,
        ipc::macos_virtual_printer_status,
        ipc::install_macos_virtual_printer,
        ipc::repair_macos_virtual_printer,
        ipc::uninstall_macos_virtual_printer
    ]
}
