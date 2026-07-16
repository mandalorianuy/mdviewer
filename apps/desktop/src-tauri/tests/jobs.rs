use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use mdviewer_desktop_lib::jobs::{JobState, PrintJobId, PrintJobStore};

fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mdviewer-task12-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn pdf(path: &Path) {
    fs::write(path, b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n%%EOF\n").unwrap();
}

fn store(root: &Path, scope: &Path) -> PrintJobStore {
    PrintJobStore::new(root, [scope]).unwrap()
}

#[test]
fn print_job_ids_accept_only_canonical_lowercase_uuid_v4() {
    let valid = "6ba7b810-9dad-4f11-80b4-00c04fd430c8";
    assert_eq!(PrintJobId::parse(valid).unwrap().to_string(), valid);

    for hostile in [
        "6BA7B810-9DAD-4F11-80B4-00C04FD430C8",
        "{6ba7b810-9dad-4f11-80b4-00c04fd430c8}",
        "6ba7b8109dad4f1180b400c04fd430c8",
        "6ba7b810-9dad-1f11-80b4-00c04fd430c8",
        "../6ba7b810-9dad-4f11-80b4-00c04fd430c8",
        "6ba7b810-9dad-4f11-80b4-00c04fd430c8/extra",
        "not-a-uuid",
    ] {
        assert!(PrintJobId::parse(hostile).is_err(), "accepted {hostile:?}");
    }
}

#[test]
fn root_must_be_absolute_and_stage_requires_an_authorized_source_scope() {
    assert_eq!(
        PrintJobStore::new("relative/jobs", std::iter::empty::<&Path>())
            .unwrap_err()
            .code(),
        "invalid_job_root"
    );

    let temp = temp_dir("scope");
    let root = temp.join("jobs");
    let allowed = temp.join("allowed");
    let outside = temp.join("outside");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&outside).unwrap();
    let source = outside.join("secret.pdf");
    pdf(&source);

    let error = store(&root, &allowed)
        .stage_pdf(&source, Some("secret/path\n.pdf"))
        .unwrap_err();
    assert_eq!(error.code(), "unauthorized_source");
    assert!(!format!("{error}").contains("secret"));
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn staging_copies_a_regular_pdf_into_private_storage_with_exact_metadata() {
    let temp = temp_dir("stage");
    let root = temp.join("jobs");
    let allowed = temp.join("allowed");
    fs::create_dir(&allowed).unwrap();
    let source = allowed.join("source.pdf");
    pdf(&source);

    let job = store(&root, &allowed)
        .stage_pdf(&source, Some("  Quarterly / report\n2026  "))
        .unwrap();
    assert_eq!(job.state, JobState::Staged);
    assert_eq!(job.title, "Quarterly report 2026");
    assert_eq!(
        fs::read(&job.input_pdf).unwrap(),
        fs::read(&source).unwrap()
    );
    let raw = fs::read_to_string(&job.metadata_path).unwrap();
    assert!(raw.starts_with("{\"schema\":\"mdviewer.print-job/v1\",\"id\":"));
    assert!(raw.ends_with("}\n"));
    let metadata: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(metadata["schema"], "mdviewer.print-job/v1");
    assert_eq!(metadata["id"], job.id.to_string());
    assert_eq!(metadata["title"], "Quarterly report 2026");
    assert_eq!(metadata.as_object().unwrap().len(), 4);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&job.directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&job.input_pdf).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&job.metadata_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn stage_rejects_non_pdf_and_symlink_sources_without_creating_a_job() {
    let temp = temp_dir("source-links");
    let root = temp.join("jobs");
    let allowed = temp.join("allowed");
    fs::create_dir(&allowed).unwrap();
    let text = allowed.join("not.pdf");
    fs::write(&text, b"authored private text").unwrap();
    assert_eq!(
        store(&root, &allowed)
            .stage_pdf(&text, None)
            .unwrap_err()
            .code(),
        "invalid_pdf"
    );

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&text, allowed.join("link.pdf")).unwrap();
        assert_eq!(
            store(&root, &allowed)
                .stage_pdf(&allowed.join("link.pdf"), None)
                .unwrap_err()
                .code(),
            "unsafe_job_path"
        );
    }
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    fs::remove_dir_all(temp).unwrap();
}

#[cfg(unix)]
#[test]
fn stage_rejects_a_parent_replaced_with_a_symlink_after_scope_authorization() {
    let temp = temp_dir("source-parent-swap");
    let root = temp.join("jobs");
    let allowed = temp.join("allowed");
    let selected_parent = allowed.join("selected");
    let held_parent = allowed.join("selected-held");
    let outside = temp.join("outside");
    fs::create_dir_all(&selected_parent).unwrap();
    fs::create_dir(&outside).unwrap();
    let selected_source = selected_parent.join("input.pdf");
    pdf(&selected_source);
    pdf(&outside.join("input.pdf"));
    let store = store(&root, &allowed);

    fs::rename(&selected_parent, &held_parent).unwrap();
    std::os::unix::fs::symlink(&outside, &selected_parent).unwrap();
    let error = store.stage_pdf(&selected_source, None).unwrap_err();

    assert!(matches!(
        error.code(),
        "unsafe_job_path" | "unauthorized_source"
    ));
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn source_opening_is_anchored_to_authorized_scope_handles() {
    let jobs_source =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/jobs.rs")).unwrap();
    let state_source =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/state.rs")).unwrap();
    assert!(jobs_source.contains("struct AuthorizedScope"));
    assert!(jobs_source.contains("libc::openat("));
    assert!(jobs_source.contains("windows_nt_open_relative"));
    assert!(jobs_source.contains("root_directory"));
    assert!(!jobs_source.contains("MoveFileExW"));
    assert!(state_source.contains("NtCreateFile"));
    assert!(state_source.contains("RootDirectory"));
    assert!(state_source.contains("SetFileInformationByHandle"));
}

#[test]
fn windows_private_storage_has_explicit_owner_and_protected_user_only_dacl_proof() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = fs::read_to_string(manifest.join("src/jobs.rs")).unwrap();
    let cargo = fs::read_to_string(manifest.join("Cargo.toml")).unwrap();
    for api in [
        "SetKernelObjectSecurity",
        "GetKernelObjectSecurity",
        "SetSecurityDescriptorOwner",
        "SetSecurityDescriptorDacl",
        "PROTECTED_DACL_SECURITY_INFORMATION",
        "EqualSid",
        "GetAce",
    ] {
        assert!(source.contains(api), "missing Windows ACL API proof: {api}");
    }
    assert!(cargo.contains("Win32_System_Threading"));
    assert!(cargo.contains("Win32_System_SystemServices"));
}

#[test]
fn claim_is_exactly_once_and_missing_input_or_tampered_metadata_fail_closed() {
    let temp = temp_dir("claim");
    let root = temp.join("jobs");
    let allowed = temp.join("allowed");
    fs::create_dir(&allowed).unwrap();
    let source = allowed.join("source.pdf");
    pdf(&source);
    let store = store(&root, &allowed);

    let first = store.stage_pdf(&source, Some("First")).unwrap();
    let claimed = store.claim(first.id).unwrap();
    assert_eq!(claimed.state, JobState::Claimed);
    assert_eq!(store.claim(first.id).unwrap_err().code(), "already_claimed");

    let missing = store.stage_pdf(&source, None).unwrap();
    fs::remove_file(&missing.input_pdf).unwrap();
    assert_eq!(store.claim(missing.id).unwrap_err().code(), "missing_input");

    let missing_metadata = store.stage_pdf(&source, None).unwrap();
    fs::remove_file(&missing_metadata.metadata_path).unwrap();
    assert_eq!(
        store.claim(missing_metadata.id).unwrap_err().code(),
        "invalid_job_metadata"
    );

    let tampered = store.stage_pdf(&source, None).unwrap();
    fs::write(
        &tampered.metadata_path,
        b"{\"schema\":\"wrong\",\"id\":\"forged\"}\n",
    )
    .unwrap();
    assert_eq!(
        store.claim(tampered.id).unwrap_err().code(),
        "invalid_job_metadata"
    );
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn an_existing_claimed_job_is_verified_before_reporting_a_double_claim() {
    let temp = temp_dir("claimed-tamper");
    let root = temp.join("jobs");
    let allowed = temp.join("allowed");
    fs::create_dir(&allowed).unwrap();
    let source = allowed.join("source.pdf");
    pdf(&source);
    let store = store(&root, &allowed);
    let staged = store.stage_pdf(&source, None).unwrap();
    let claimed = store.claim(staged.id).unwrap();
    fs::write(&claimed.metadata_path, b"{}\n").unwrap();

    assert_eq!(
        store.claim(staged.id).unwrap_err().code(),
        "invalid_job_metadata"
    );
    fs::remove_dir_all(temp).unwrap();
}

#[cfg(unix)]
#[test]
fn job_permissions_must_remain_exactly_user_private() {
    use std::os::unix::fs::PermissionsExt;

    let temp = temp_dir("permissions");
    let root = temp.join("jobs");
    let allowed = temp.join("allowed");
    fs::create_dir(&allowed).unwrap();
    let source = allowed.join("source.pdf");
    pdf(&source);
    let store = store(&root, &allowed);
    let staged = store.stage_pdf(&source, None).unwrap();

    fs::set_permissions(&staged.input_pdf, fs::Permissions::from_mode(0o400)).unwrap();
    assert_eq!(
        store.claim(staged.id).unwrap_err().code(),
        "unsafe_job_path"
    );
    fs::remove_dir_all(temp).unwrap();
}

#[cfg(unix)]
#[test]
fn claim_rejects_job_directory_symlink_escape_and_input_hardlinks() {
    use std::os::unix::fs::symlink;

    let temp = temp_dir("job-links");
    let root = temp.join("jobs");
    let allowed = temp.join("allowed");
    let outside = temp.join("outside");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&outside).unwrap();
    let source = allowed.join("source.pdf");
    pdf(&source);
    let store = store(&root, &allowed);

    let escaped_id = PrintJobId::new();
    let escaped = outside.join("escaped.staged");
    fs::create_dir(&escaped).unwrap();
    symlink(&escaped, root.join(format!("{escaped_id}.staged"))).unwrap();
    assert_eq!(
        store.claim(escaped_id).unwrap_err().code(),
        "unsafe_job_path"
    );

    let linked = store.stage_pdf(&source, None).unwrap();
    let external = outside.join("same.pdf");
    fs::hard_link(&linked.input_pdf, &external).unwrap();
    assert_eq!(
        store.claim(linked.id).unwrap_err().code(),
        "unsafe_job_path"
    );
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn finish_removes_only_a_valid_claimed_job_and_cleanup_reports_both_stale_states() {
    let temp = temp_dir("cleanup");
    let root = temp.join("jobs");
    let allowed = temp.join("allowed");
    fs::create_dir(&allowed).unwrap();
    let source = allowed.join("source.pdf");
    pdf(&source);
    let store = store(&root, &allowed);

    let finished = store.stage_pdf(&source, None).unwrap();
    store.claim(finished.id).unwrap();
    store.finish(finished.id).unwrap();
    assert_eq!(
        store.finish(finished.id).unwrap_err().code(),
        "job_not_found"
    );

    let staged = store.stage_pdf(&source, None).unwrap();
    let claimed = store.stage_pdf(&source, None).unwrap();
    store.claim(claimed.id).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    let report = store.cleanup_older_than(Duration::from_millis(1)).unwrap();
    assert!(report.removed_staged.contains(&staged.id));
    assert!(report.removed_claimed.contains(&claimed.id));
    assert!(report.rejected.is_empty());
    fs::remove_dir_all(temp).unwrap();
}

#[cfg(unix)]
#[test]
fn cleanup_recovers_only_safe_stale_incomplete_stage_directories() {
    use std::{
        io::Write,
        os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    };

    let temp = temp_dir("incomplete-cleanup");
    let root = temp.join("jobs");
    let allowed = temp.join("allowed");
    fs::create_dir(&allowed).unwrap();
    let store = store(&root, &allowed);

    let safe = store.root().join(format!(
        ".stage-{}-{}",
        PrintJobId::new(),
        uuid::Uuid::new_v4()
    ));
    fs::DirBuilder::new().mode(0o700).create(&safe).unwrap();
    let mut partial = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(safe.join("input.pdf"))
        .unwrap();
    partial.write_all(b"%PDF-1.7\n").unwrap();
    partial.sync_all().unwrap();

    let hostile = store.root().join(format!(
        ".stage-{}-{}",
        PrintJobId::new(),
        uuid::Uuid::new_v4()
    ));
    fs::DirBuilder::new().mode(0o700).create(&hostile).unwrap();
    std::os::unix::fs::symlink(&safe, hostile.join("input.pdf")).unwrap();

    std::thread::sleep(Duration::from_millis(5));
    let report = store.cleanup_older_than(Duration::from_millis(1)).unwrap();
    assert_eq!(report.removed_incomplete, 1);
    assert_eq!(report.rejected.len(), 1);
    assert!(!safe.exists());
    assert!(hostile.exists());
    fs::remove_dir_all(temp).unwrap();
}

#[cfg(unix)]
#[test]
fn cleanup_removes_staged_and_claimed_jobs_strictly_older_than_24_hours() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    fn set_mtime(path: &Path, seconds: i64) {
        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        let times = [
            libc::timespec {
                tv_sec: seconds,
                tv_nsec: 0,
            },
            libc::timespec {
                tv_sec: seconds,
                tv_nsec: 0,
            },
        ];
        assert_eq!(
            unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) },
            0
        );
    }

    let temp = temp_dir("24h-cleanup");
    let root = temp.join("jobs");
    let allowed = temp.join("allowed");
    fs::create_dir(&allowed).unwrap();
    let source = allowed.join("source.pdf");
    pdf(&source);
    let store = store(&root, &allowed);
    let staged = store.stage_pdf(&source, None).unwrap();
    let claimed = store.stage_pdf(&source, None).unwrap();
    let claimed = store.claim(claimed.id).unwrap();
    let recent = store.stage_pdf(&source, None).unwrap();
    let old = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        - 25 * 60 * 60;
    set_mtime(&staged.directory, old);
    set_mtime(&claimed.directory, old);

    let report = store
        .cleanup_older_than(Duration::from_secs(24 * 60 * 60))
        .unwrap();
    assert!(report.removed_staged.contains(&staged.id));
    assert!(report.removed_claimed.contains(&claimed.id));
    assert!(report.skipped_recent.contains(&recent.id));
    fs::remove_dir_all(temp).unwrap();
}
