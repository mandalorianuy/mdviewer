pub mod commands;
pub mod deep_link;
pub mod jobs;
pub mod macos_integration;
pub mod state;

use jobs::{PrintJobId, PrintJobStore};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

#[cfg(target_os = "macos")]
const BUNDLED_PDFIUM_PATH: &str = "lib/libpdfium.dylib";

#[cfg(target_os = "macos")]
pub fn configure_bundled_pdfium(
    resource_directory: &std::path::Path,
) -> Result<(), mdconvert_core::ConversionError> {
    mdconvert_pdf::configure_pdfium_library_path(resource_directory.join(BUNDLED_PDFIUM_PATH))
}

pub fn builder() -> tauri::Builder<tauri::Wry> {
    builder_with_pending_opened_files(PendingOpenedPrintFiles::default())
}

fn builder_with_pending_opened_files(
    pending_opened_files: PendingOpenedPrintFiles,
) -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            configure_bundled_pdfium(&app.path().resource_dir()?)?;
            let app_data = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&app_data)?;
            let jobs = jobs::PrintJobStore::new(
                app_data.join("print-jobs"),
                std::iter::empty::<&std::path::Path>(),
            )?;
            let state = state::AppState::new(jobs, app_data.join("runtime"))?;
            app.manage(state);
            let report = flush_pending_opened_print_files(app.handle(), &pending_opened_files);
            log_opened_print_report(&report);
            if let Some(urls) = app.deep_link().get_current()? {
                for url in urls {
                    forward_print_deep_link(app.handle(), url.as_str());
                }
            }
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    forward_print_deep_link(&handle, url.as_str());
                }
            });
            Ok(())
        })
        .invoke_handler(commands::invoke_handler())
}

pub fn forward_print_deep_link<R: tauri::Runtime>(app: &tauri::AppHandle<R>, value: &str) {
    let Ok(id) = deep_link::parse_print_deep_link(value) else {
        return;
    };
    let state = app.state::<state::AppState>();
    if state.queue_print_job(id).is_ok() {
        let _ = app.emit("print-job-requested", id.to_string());
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenedPrintReport {
    pub staged_ids: Vec<PrintJobId>,
    pub rejected: u64,
    pub rejection_codes: Vec<&'static str>,
}

#[derive(Clone, Default)]
pub struct PendingOpenedPrintFiles {
    urls: Arc<Mutex<Vec<tauri::Url>>>,
}

pub fn route_opened_print_files<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pending: &PendingOpenedPrintFiles,
    urls: &[tauri::Url],
) -> Option<OpenedPrintReport> {
    if app.try_state::<state::AppState>().is_some() {
        return Some(forward_opened_print_files(app, urls));
    }

    match pending.urls.lock() {
        Ok(mut queued) => {
            queued.extend_from_slice(urls);
            None
        }
        Err(_) => Some(OpenedPrintReport {
            staged_ids: Vec::new(),
            rejected: urls.len() as u64,
            rejection_codes: vec!["state_unavailable"; urls.len()],
        }),
    }
}

pub fn flush_pending_opened_print_files<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pending: &PendingOpenedPrintFiles,
) -> OpenedPrintReport {
    let urls = match pending.urls.lock() {
        Ok(mut queued) => std::mem::take(&mut *queued),
        Err(_) => {
            return OpenedPrintReport {
                staged_ids: Vec::new(),
                rejected: 1,
                rejection_codes: vec!["state_unavailable"],
            };
        }
    };
    forward_opened_print_files(app, &urls)
}

fn log_opened_print_report(report: &OpenedPrintReport) {
    let received = report.staged_ids.len() as u64 + report.rejected;
    if received == 0 {
        return;
    }
    eprintln!(
        "mdviewer-opened-event received={} staged={} rejected={} codes={}",
        received,
        report.staged_ids.len(),
        report.rejected,
        report.rejection_codes.join(",")
    );
}

pub fn forward_opened_print_files<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    urls: &[tauri::Url],
) -> OpenedPrintReport {
    let state = app.state::<state::AppState>();
    let mut report = OpenedPrintReport::default();
    for url in urls {
        let result = (|| {
            let source = url.to_file_path().map_err(|_| "non_file_url")?;
            let scope = source.parent().ok_or("missing_parent")?;
            let intake =
                PrintJobStore::new(state.jobs().root(), [scope]).map_err(|error| error.code())?;
            let title = source.file_stem().and_then(|value| value.to_str());
            let job = intake
                .stage_pdf(&source, title)
                .map_err(|error| error.code())?;
            if let Err(error) = state.queue_print_job(job.id) {
                if intake.claim(job.id).is_ok() {
                    let _ = intake.finish(job.id);
                }
                return Err(error.code());
            }
            Ok(job.id)
        })();
        match result {
            Ok(id) => {
                report.staged_ids.push(id);
                let _ = app.emit("print-job-requested", id.to_string());
            }
            Err(code) => {
                report.rejected = report.rejected.saturating_add(1);
                report.rejection_codes.push(code);
            }
        }
    }
    report
}

pub fn run() {
    let pending_opened_files = PendingOpenedPrintFiles::default();
    builder_with_pending_opened_files(pending_opened_files.clone())
        .build(tauri::generate_context!())
        .expect("MDViewer failed to build")
        .run(move |app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = event
                && let Some(report) = route_opened_print_files(app, &pending_opened_files, &urls)
            {
                log_opened_print_report(&report);
            }
        });
}
