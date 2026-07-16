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
    phase: Arc<Mutex<PendingOpenedPhase>>,
}

enum PendingOpenedPhase {
    Buffering(Vec<tauri::Url>),
    Ready,
}

impl Default for PendingOpenedPhase {
    fn default() -> Self {
        Self::Buffering(Vec::new())
    }
}

enum OpenedFileRoute {
    Buffered,
    Forward(Vec<tauri::Url>),
}

impl PendingOpenedPrintFiles {
    fn route_urls(&self, urls: &[tauri::Url]) -> Result<OpenedFileRoute, ()> {
        let mut phase = self.phase.lock().map_err(|_| ())?;
        match &mut *phase {
            PendingOpenedPhase::Buffering(queued) => {
                queued.extend_from_slice(urls);
                Ok(OpenedFileRoute::Buffered)
            }
            PendingOpenedPhase::Ready => Ok(OpenedFileRoute::Forward(urls.to_vec())),
        }
    }

    fn mark_ready_locked(phase: &mut PendingOpenedPhase) -> Vec<tauri::Url> {
        match std::mem::replace(phase, PendingOpenedPhase::Ready) {
            PendingOpenedPhase::Buffering(urls) => urls,
            PendingOpenedPhase::Ready => Vec::new(),
        }
    }

    fn take_buffered_and_mark_ready(&self) -> Result<Vec<tauri::Url>, ()> {
        let mut phase = self.phase.lock().map_err(|_| ())?;
        Ok(Self::mark_ready_locked(&mut phase))
    }
}

pub fn route_opened_print_files<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pending: &PendingOpenedPrintFiles,
    urls: &[tauri::Url],
) -> Option<OpenedPrintReport> {
    match pending.route_urls(urls) {
        Ok(OpenedFileRoute::Buffered) => None,
        Ok(OpenedFileRoute::Forward(urls)) => Some(forward_opened_print_files(app, &urls)),
        Err(()) => Some(OpenedPrintReport {
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
    let urls = match pending.take_buffered_and_mark_ready() {
        Ok(urls) => urls,
        Err(()) => {
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

#[cfg(test)]
mod opened_file_transition_tests {
    use std::{fs, sync::mpsc};

    use tauri::Manager;

    use super::*;

    #[test]
    fn ready_transition_cannot_lose_or_duplicate_a_concurrent_opened_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("concurrent.pdf");
        fs::write(&source, b"%PDF-1.7\nconcurrent\n%%EOF\n").unwrap();
        let store =
            PrintJobStore::new(directory.path().join("jobs"), [source.parent().unwrap()]).unwrap();
        let state = state::AppState::new(store, directory.path().join("runtime")).unwrap();
        let app = tauri::test::mock_builder()
            .manage(state)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let pending = PendingOpenedPrintFiles::default();
        let mut transition = pending.phase.lock().unwrap();
        let url = tauri::Url::from_file_path(source).unwrap();
        let concurrent = pending.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let route = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            concurrent.route_urls(&[url]).unwrap()
        });
        started_rx.recv().unwrap();

        let buffered = PendingOpenedPrintFiles::mark_ready_locked(&mut transition);
        drop(transition);
        let OpenedFileRoute::Forward(urls) = route.join().unwrap() else {
            panic!("a route after the ready transition must not buffer");
        };

        assert!(buffered.is_empty());
        let report = forward_opened_print_files(app.handle(), &urls);
        assert_eq!(report.staged_ids.len(), 1);
        assert_eq!(report.rejected, 0);
        assert!(pending.take_buffered_and_mark_ready().unwrap().is_empty());
        assert_eq!(
            app.state::<state::AppState>().pending_print_jobs().unwrap(),
            report.staged_ids
        );
    }
}
