use std::{
    ffi::OsString,
    fs, io,
    path::Path,
    sync::{Arc, Mutex},
};

use mdviewer_desktop_lib::jobs::{JobState, PrintJobStore};
use mdviewer_pdf_workflow::{WorkflowError, default_job_root, run_invocation};
use tempfile::TempDir;

fn pdf(path: &Path) {
    fs::write(path, b"%PDF-1.7\nfixture\n").unwrap();
}

#[test]
fn final_argument_is_staged_and_cups_arguments_are_opaque() {
    let home = TempDir::new().unwrap();
    let source = home.path().join("quarterly report.pdf");
    pdf(&source);
    let jobs = default_job_root(home.path());
    let dispatched = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&dispatched);

    let id = run_invocation(
        &[
            OsString::from("42"),
            OsString::from("facundo"),
            OsString::from("opaque title"),
            OsString::from("1"),
            OsString::from("media=A4 raw/path=must-not-be-parsed"),
            source.clone().into_os_string(),
        ],
        &jobs,
        move |url| {
            captured.lock().unwrap().push(url.to_owned());
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(
        dispatched.lock().unwrap().as_slice(),
        [format!("mdviewer://print/{id}")]
    );
    let store = PrintJobStore::new(&jobs, [home.path()]).unwrap();
    let claimed = store.claim(id).unwrap();
    assert_eq!(claimed.state, JobState::Claimed);
    assert_eq!(claimed.title, "quarterly report");
    assert_eq!(
        fs::read(claimed.input_pdf).unwrap(),
        fs::read(source).unwrap()
    );
}

#[test]
fn persistence_failure_never_dispatches_and_does_not_disclose_paths() {
    let home = TempDir::new().unwrap();
    let source = home.path().join("secret-customer-name.pdf");
    pdf(&source);
    let dispatched = Arc::new(Mutex::new(false));
    let captured = Arc::clone(&dispatched);

    let error = run_invocation(
        &[source.clone().into_os_string()],
        Path::new("relative/jobs"),
        move |_| {
            *captured.lock().unwrap() = true;
            Ok(())
        },
    )
    .unwrap_err();

    assert_eq!(error, WorkflowError::Persistence);
    assert!(!*dispatched.lock().unwrap());
    assert!(!error.to_string().contains("secret-customer-name"));
}

#[test]
fn dispatch_failure_is_nonzero_after_a_durable_job_exists() {
    let home = TempDir::new().unwrap();
    let source = home.path().join("input.pdf");
    pdf(&source);
    let jobs = default_job_root(home.path());

    let error = run_invocation(&[source.into_os_string()], &jobs, |_| {
        Err(io::Error::other("launch services unavailable"))
    })
    .unwrap_err();

    assert_eq!(error, WorkflowError::Dispatch);
    let entries = fs::read_dir(jobs)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0]
            .file_name()
            .to_string_lossy()
            .ends_with(".staged")
    );
}

#[test]
fn missing_or_non_pdf_final_argument_is_rejected_without_dispatch() {
    let home = TempDir::new().unwrap();
    let text = home.path().join("input.txt");
    fs::write(&text, b"not a pdf").unwrap();
    let jobs = default_job_root(home.path());

    assert_eq!(
        run_invocation::<fn(&str) -> io::Result<()>>(&[], &jobs, |_| Ok(())).unwrap_err(),
        WorkflowError::Invocation
    );
    assert_eq!(
        run_invocation(&[text.into_os_string()], &jobs, |_| Ok(())).unwrap_err(),
        WorkflowError::Persistence
    );
}
