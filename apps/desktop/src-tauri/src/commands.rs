use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::Read,
    path::Path,
};

use mdconvert_core::ConversionLimits;
use serde::Serialize;
use tauri::State;

use crate::{
    jobs::{JobError, PrintJobId},
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenDocumentResult {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveDocumentResult {
    pub saved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConversionResult {
    pub operation_id: String,
    pub markdown_path: String,
    pub assets_path: Option<String>,
    pub warning_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClaimedPrintJob {
    pub id: String,
    pub title: String,
    pub created_unix_ms: u64,
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
    let mut file =
        open_read_no_follow(&selection.path).map_err(|_| CommandError::new("open_failed"))?;
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
    Ok(SaveDocumentResult { saved: true })
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
        state.record_warnings(operation_id, warning_codes.clone())?;
        Ok(ConversionResult {
            operation_id: operation_id.to_owned(),
            markdown_path: output.path.to_string_lossy().into_owned(),
            assets_path: assets_path.map(|path| path.to_string_lossy().into_owned()),
            warning_codes,
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
    Ok(ClaimedPrintJob {
        id: job.id.to_string(),
        title: job.title,
        created_unix_ms: job.created_unix_ms,
    })
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
        _ => "unknown_warning",
    }
}

fn open_read_no_follow(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    options.open(path)
}

mod ipc {
    use super::*;

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
    pub(super) fn integration_status(
        state: State<'_, AppState>,
    ) -> Result<IntegrationStatus, CommandError> {
        super::integration_status(&state)
    }
}

pub fn invoke_handler<R: tauri::Runtime>()
-> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        ipc::open,
        ipc::save,
        ipc::convert,
        ipc::cancel,
        ipc::warnings,
        ipc::claim_print_job,
        ipc::integration_status
    ]
}
