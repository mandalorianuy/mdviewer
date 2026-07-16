use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use mdviewer_desktop_lib::jobs::{PrintJobId, PrintJobStore};
use thiserror::Error;

pub const WORKFLOW_NAME: &str = "Guardar como Markdown con MDViewer";
pub const WORKFLOW_MARKER: &str = "com.mdviewer.pdf-workflow/v1";

#[used]
static EMBEDDED_MARKER: &str = concat!(
    "com.mdviewer.pdf-workflow/v1\nversion=",
    env!("CARGO_PKG_VERSION"),
    "\n"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WorkflowError {
    #[error("the PDF Workflow invocation is invalid")]
    Invocation,
    #[error("the print job could not be persisted")]
    Persistence,
    #[error("MDViewer could not be opened")]
    Dispatch,
}

#[must_use]
pub fn default_job_root(home: &Path) -> PathBuf {
    home.join("Library/Application Support/com.mdviewer.desktop/print-jobs")
}

pub fn run_invocation<F>(
    arguments: &[OsString],
    job_root: &Path,
    dispatch: F,
) -> Result<PrintJobId, WorkflowError>
where
    F: FnOnce(&str) -> io::Result<()>,
{
    let source = arguments
        .last()
        .map(PathBuf::from)
        .ok_or(WorkflowError::Invocation)?;
    let scope = source.parent().ok_or(WorkflowError::Invocation)?;
    let title = source.file_stem().and_then(|value| value.to_str());
    let root_parent = job_root.parent().ok_or(WorkflowError::Persistence)?;
    fs::create_dir_all(root_parent).map_err(|_| WorkflowError::Persistence)?;
    if fs::symlink_metadata(root_parent)
        .map_err(|_| WorkflowError::Persistence)?
        .file_type()
        .is_symlink()
    {
        return Err(WorkflowError::Persistence);
    }
    let store = PrintJobStore::new(job_root, [scope]).map_err(|_| WorkflowError::Persistence)?;
    let job = store
        .stage_pdf(&source, title)
        .map_err(|_| WorkflowError::Persistence)?;
    let url = format!("mdviewer://print/{}", job.id);
    dispatch(&url).map_err(|_| WorkflowError::Dispatch)?;
    Ok(job.id)
}

#[cfg(target_os = "macos")]
pub fn dispatch_with_launch_services(url: &str) -> io::Result<()> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg(url)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("Launch Services rejected the URL"))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn dispatch_with_launch_services(_url: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Launch Services is unavailable",
    ))
}
