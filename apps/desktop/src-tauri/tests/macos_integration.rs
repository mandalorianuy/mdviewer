use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use mdviewer_desktop_lib::macos_integration::{
    ApplicationAlias, ApplicationArtifact, CodeSignatureVerifier, IntegrationError,
    IntegrationManager, IntegrationStatus, WORKFLOW_NAME, sha256_hex,
};
use tempfile::TempDir;

#[test]
fn desktop_base_config_is_portable_and_macos_override_is_the_exact_native_target() {
    let base: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let macos: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.macos.conf.json")).unwrap();

    assert_eq!(base["identifier"], "com.mdviewer.desktop");
    assert_eq!(base["bundle"]["active"], true);
    assert_eq!(
        base["plugins"]["deep-link"]["desktop"]["schemes"],
        serde_json::json!(["mdviewer"])
    );
    assert!(base["bundle"]["fileAssociations"].is_null());
    assert!(base["bundle"]["resources"].is_null());
    let portable = serde_json::to_string(&base).unwrap();
    for macos_only in ["com.adobe.pdf", "libpdfium.dylib", ".cache/pdfium"] {
        assert!(!portable.contains(macos_only));
    }
    assert_eq!(
        macos["bundle"]["fileAssociations"],
        serde_json::json!([{
            "ext": ["pdf"],
            "contentTypes": ["com.adobe.pdf"],
            "name": "PDF document from macOS Print",
            "role": "Viewer",
            "rank": "None"
        }])
    );
    assert_eq!(
        macos["bundle"]["resources"]["../../../.cache/pdfium/chromium-7947/lib/libpdfium.dylib"],
        "lib/libpdfium.dylib"
    );
}

#[derive(Clone, Copy)]
struct FixtureAlias;

impl ApplicationAlias for FixtureAlias {
    fn create_alias(&self, application: &Path, destination: &Path) -> io::Result<()> {
        fs::write(
            destination,
            format!("MDVIEWER-APPLICATION-ALIAS\n{}\n", application.display()),
        )
    }

    fn resolve_alias(&self, alias: &Path) -> io::Result<PathBuf> {
        let content = fs::read_to_string(alias)?;
        let path = content
            .strip_prefix("MDVIEWER-APPLICATION-ALIAS\n")
            .and_then(|value| value.strip_suffix('\n'))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid fixture alias"))?;
        Ok(PathBuf::from(path))
    }
}

#[derive(Clone, Copy)]
struct FixtureSignature;

impl CodeSignatureVerifier for FixtureSignature {
    fn verify(&self, path: &Path, identity: &str, team: &str) -> io::Result<bool> {
        let executable = path.join("Contents/MacOS/mdviewer-desktop");
        let bytes = fs::read(executable)?;
        Ok(identity == "com.mdviewer.desktop"
            && team == "TEAMFIXTURE"
            && !bytes.ends_with(b"BAD-SIGNATURE"))
    }
}

struct SwapTargetOnFirstVerification {
    target: PathBuf,
    displaced: PathBuf,
    swapped: AtomicBool,
}

impl SwapTargetOnFirstVerification {
    fn new(target: PathBuf, displaced: PathBuf) -> Self {
        Self {
            target,
            displaced,
            swapped: AtomicBool::new(false),
        }
    }
}

impl CodeSignatureVerifier for SwapTargetOnFirstVerification {
    fn verify(&self, path: &Path, identity: &str, team: &str) -> io::Result<bool> {
        if !self.swapped.swap(true, Ordering::SeqCst) {
            fs::rename(&self.target, &self.displaced)?;
            fs::write(&self.target, b"unrelated replacement")?;
        }
        FixtureSignature.verify(path, identity, team)
    }
}

struct MutateQuarantineAfterAliasResolution {
    mutated: AtomicBool,
}

impl MutateQuarantineAfterAliasResolution {
    fn new() -> Self {
        Self {
            mutated: AtomicBool::new(false),
        }
    }
}

impl ApplicationAlias for MutateQuarantineAfterAliasResolution {
    fn create_alias(&self, application: &Path, destination: &Path) -> io::Result<()> {
        FixtureAlias.create_alias(application, destination)
    }

    fn resolve_alias(&self, alias: &Path) -> io::Result<PathBuf> {
        let resolved = FixtureAlias.resolve_alias(alias)?;
        let is_quarantine = alias
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains(".quarantine-"));
        if is_quarantine && !self.mutated.swap(true, Ordering::SeqCst) {
            fs::write(alias, b"unrelated in-place replacement")?;
        }
        Ok(resolved)
    }
}

fn application(home: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let app = home.join(name);
    let executable = app.join("Contents/MacOS/mdviewer-desktop");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, bytes).unwrap();
    app
}

fn artifact(app: PathBuf) -> ApplicationArtifact {
    let executable = app.join("Contents/MacOS/mdviewer-desktop");
    let bytes = fs::read(&executable).unwrap();
    ApplicationArtifact::new(
        app,
        PathBuf::from("Contents/MacOS/mdviewer-desktop"),
        sha256_hex(&bytes),
        "com.mdviewer.desktop",
        "TEAMFIXTURE",
    )
    .unwrap()
}

#[test]
fn signature_requirement_fields_reject_requirement_language_injection() {
    let home = TempDir::new().unwrap();
    let app = application(home.path(), "MDViewer.app", b"current-signed-app");
    let executable = PathBuf::from("Contents/MacOS/mdviewer-desktop");
    let sha = sha256_hex(&fs::read(app.join(&executable)).unwrap());

    assert_eq!(
        ApplicationArtifact::new(
            app.clone(),
            executable.clone(),
            &sha,
            "com.mdviewer.desktop\" or true",
            "TEAMFIXTURE",
        )
        .unwrap_err(),
        IntegrationError::InvalidArtifact
    );
    assert_eq!(
        ApplicationArtifact::new(
            app,
            executable,
            sha,
            "com.mdviewer.desktop",
            "TEAMFIXTURE\" or true",
        )
        .unwrap_err(),
        IntegrationError::InvalidArtifact
    );
}

fn manager(home: &TempDir, app: PathBuf) -> IntegrationManager<FixtureAlias, FixtureSignature> {
    IntegrationManager::new(home.path(), artifact(app), FixtureAlias, FixtureSignature).unwrap()
}

fn retained_workflow_items(home: &TempDir) -> Vec<PathBuf> {
    let directory = home
        .path()
        .join("Library/Application Support/com.mdviewer.desktop/Retired PDF Services");
    match fs::read_dir(directory) {
        Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("could not inspect retained workflow items: {error}"),
    }
}

#[test]
fn install_creates_the_exact_per_user_application_alias_and_reports_installed() {
    let home = TempDir::new().unwrap();
    let app = application(home.path(), "MDViewer.app", b"current-signed-app");
    let manager = manager(&home, app.clone());

    assert_eq!(manager.status().unwrap(), IntegrationStatus::NotInstalled);
    manager.install().unwrap();

    let target = home.path().join("Library/PDF Services").join(WORKFLOW_NAME);
    assert_eq!(manager.target(), target);
    assert!(fs::symlink_metadata(&target).unwrap().file_type().is_file());
    assert_eq!(FixtureAlias.resolve_alias(&target).unwrap(), app);
    assert_eq!(manager.status().unwrap(), IntegrationStatus::Installed);
    assert!(
        fs::read_dir(target.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".install-"))
    );
}

#[test]
fn a_valid_alias_to_an_older_signed_app_is_outdated_and_repair_is_atomic() {
    let home = TempDir::new().unwrap();
    let old_app = application(home.path(), "MDViewer-old.app", b"old-signed-app");
    let current_app = application(home.path(), "MDViewer.app", b"current-signed-app");
    manager(&home, old_app).install().unwrap();
    let manager = manager(&home, current_app.clone());

    assert_eq!(manager.status().unwrap(), IntegrationStatus::Outdated);
    manager.repair().unwrap();

    assert_eq!(manager.status().unwrap(), IntegrationStatus::Installed);
    assert_eq!(
        FixtureAlias.resolve_alias(&manager.target()).unwrap(),
        current_app
    );
    assert_eq!(retained_workflow_items(&home).len(), 1);
}

#[test]
fn repair_preserves_a_target_swapped_after_validation() {
    let home = TempDir::new().unwrap();
    let app = application(home.path(), "MDViewer.app", b"current-signed-app");
    let installed = manager(&home, app.clone());
    installed.install().unwrap();
    let target = installed.target();
    let displaced = home
        .path()
        .join("managed-alias-displaced-during-validation");
    let manager = IntegrationManager::new(
        home.path(),
        artifact(app.clone()),
        FixtureAlias,
        SwapTargetOnFirstVerification::new(target.clone(), displaced.clone()),
    )
    .unwrap();

    assert_eq!(
        manager.repair().unwrap_err(),
        IntegrationError::UnsafeTarget
    );
    assert_eq!(fs::read(&target).unwrap(), b"unrelated replacement");
    assert_eq!(FixtureAlias.resolve_alias(&displaced).unwrap(), app);
}

#[test]
fn uninstall_preserves_a_target_swapped_after_validation() {
    let home = TempDir::new().unwrap();
    let app = application(home.path(), "MDViewer.app", b"current-signed-app");
    let installed = manager(&home, app.clone());
    installed.install().unwrap();
    let target = installed.target();
    let displaced = home
        .path()
        .join("managed-alias-displaced-during-validation");
    let manager = IntegrationManager::new(
        home.path(),
        artifact(app.clone()),
        FixtureAlias,
        SwapTargetOnFirstVerification::new(target.clone(), displaced.clone()),
    )
    .unwrap();

    assert_eq!(
        manager.uninstall().unwrap_err(),
        IntegrationError::UnsafeTarget
    );
    assert_eq!(fs::read(&target).unwrap(), b"unrelated replacement");
    assert_eq!(FixtureAlias.resolve_alias(&displaced).unwrap(), app);
}

#[test]
fn uninstall_retains_an_in_place_mutation_after_final_alias_revalidation() {
    let home = TempDir::new().unwrap();
    let app = application(home.path(), "MDViewer.app", b"current-signed-app");
    let installed = manager(&home, app.clone());
    installed.install().unwrap();
    let manager = IntegrationManager::new(
        home.path(),
        artifact(app),
        MutateQuarantineAfterAliasResolution::new(),
        FixtureSignature,
    )
    .unwrap();

    manager.uninstall().unwrap();

    assert_eq!(manager.status().unwrap(), IntegrationStatus::NotInstalled);
    let retained = retained_workflow_items(&home);
    assert_eq!(retained.len(), 1);
    assert_eq!(
        fs::read(&retained[0]).unwrap(),
        b"unrelated in-place replacement"
    );
}

#[test]
fn repair_retains_an_in_place_mutation_after_final_alias_revalidation() {
    let home = TempDir::new().unwrap();
    let old_app = application(home.path(), "MDViewer-old.app", b"old-signed-app");
    let current_app = application(home.path(), "MDViewer.app", b"current-signed-app");
    manager(&home, old_app).install().unwrap();
    let manager = IntegrationManager::new(
        home.path(),
        artifact(current_app.clone()),
        MutateQuarantineAfterAliasResolution::new(),
        FixtureSignature,
    )
    .unwrap();

    manager.repair().unwrap();

    assert_eq!(manager.status().unwrap(), IntegrationStatus::Installed);
    assert_eq!(
        FixtureAlias.resolve_alias(&manager.target()).unwrap(),
        current_app
    );
    let retained = retained_workflow_items(&home);
    assert_eq!(retained.len(), 1);
    assert_eq!(
        fs::read(&retained[0]).unwrap(),
        b"unrelated in-place replacement"
    );
}

#[test]
fn invalid_signature_or_unrelated_target_is_never_repaired_or_removed() {
    let home = TempDir::new().unwrap();
    let app = application(home.path(), "MDViewer.app", b"current-signed-app");
    let manager = manager(&home, app.clone());
    manager.install().unwrap();
    fs::write(
        app.join("Contents/MacOS/mdviewer-desktop"),
        b"current-signed-appBAD-SIGNATURE",
    )
    .unwrap();

    assert_eq!(manager.status().unwrap(), IntegrationStatus::Invalid);
    assert_eq!(
        manager.repair().unwrap_err(),
        IntegrationError::UnsafeTarget
    );
    assert_eq!(
        manager.uninstall().unwrap_err(),
        IntegrationError::UnsafeTarget
    );

    fs::remove_file(manager.target()).unwrap();
    fs::write(manager.target(), b"unrelated file").unwrap();
    assert_eq!(manager.status().unwrap(), IntegrationStatus::Invalid);
    assert_eq!(
        manager.install().unwrap_err(),
        IntegrationError::TargetExists
    );
    assert_eq!(
        manager.repair().unwrap_err(),
        IntegrationError::UnsafeTarget
    );
    assert_eq!(fs::read(manager.target()).unwrap(), b"unrelated file");
}

#[test]
fn uninstall_accepts_current_or_outdated_owned_alias_and_preserves_siblings() {
    let home = TempDir::new().unwrap();
    let old_app = application(home.path(), "MDViewer-old.app", b"old-signed-app");
    let current_app = application(home.path(), "MDViewer.app", b"current-signed-app");
    manager(&home, old_app).install().unwrap();
    let manager = manager(&home, current_app);
    let sibling = manager.target().parent().unwrap().join("Keep Me");
    fs::write(&sibling, b"unrelated").unwrap();
    assert_eq!(manager.status().unwrap(), IntegrationStatus::Outdated);

    manager.uninstall().unwrap();

    assert_eq!(manager.status().unwrap(), IntegrationStatus::NotInstalled);
    assert_eq!(fs::read(sibling).unwrap(), b"unrelated");
    assert!(manager.target().parent().unwrap().is_dir());
    assert_eq!(retained_workflow_items(&home).len(), 1);
}

#[cfg(unix)]
#[test]
fn interfered_retention_directory_is_preserved_and_restores_the_workflow() {
    use std::os::unix::fs::symlink;

    let home = TempDir::new().unwrap();
    let app = application(home.path(), "MDViewer.app", b"current-signed-app");
    let manager = manager(&home, app.clone());
    manager.install().unwrap();
    let retained = home
        .path()
        .join("Library/Application Support/com.mdviewer.desktop/Retired PDF Services");
    fs::create_dir_all(retained.parent().unwrap()).unwrap();
    let outside = home.path().join("unrelated-retention-target");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), b"unrelated").unwrap();
    symlink(&outside, &retained).unwrap();

    assert_eq!(
        manager.uninstall().unwrap_err(),
        IntegrationError::UnsafeTarget
    );
    assert_eq!(manager.status().unwrap(), IntegrationStatus::Installed);
    assert_eq!(FixtureAlias.resolve_alias(&manager.target()).unwrap(), app);
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"unrelated");
    assert!(
        fs::symlink_metadata(retained)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn symlink_targets_are_invalid_and_are_never_followed() {
    use std::os::unix::fs::symlink;

    let home = TempDir::new().unwrap();
    let app = application(home.path(), "MDViewer.app", b"current-signed-app");
    let manager = manager(&home, app);
    fs::create_dir_all(manager.target().parent().unwrap()).unwrap();
    let outside = home.path().join("outside");
    fs::write(&outside, b"unrelated").unwrap();
    symlink(&outside, manager.target()).unwrap();

    assert_eq!(manager.status().unwrap(), IntegrationStatus::Invalid);
    assert_eq!(
        manager.repair().unwrap_err(),
        IntegrationError::UnsafeTarget
    );
    assert_eq!(
        manager.uninstall().unwrap_err(),
        IntegrationError::UnsafeTarget
    );
    assert_eq!(fs::read(outside).unwrap(), b"unrelated");
}

#[cfg(target_os = "macos")]
#[test]
fn native_alias_round_trips_without_finder_automation() {
    use mdviewer_desktop_lib::macos_integration::NativeApplicationAlias;

    let directory = TempDir::new().unwrap();
    let target = std::env::current_exe().unwrap();
    let alias = directory.path().join("MDViewer alias");

    NativeApplicationAlias
        .create_alias(&target, &alias)
        .unwrap();

    assert!(fs::symlink_metadata(&alias).unwrap().file_type().is_file());
    assert_eq!(
        fs::canonicalize(NativeApplicationAlias.resolve_alias(&alias).unwrap()).unwrap(),
        fs::canonicalize(target).unwrap()
    );
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "writes the explicitly authorized per-user PDF Services target"]
fn installs_the_exact_embedded_development_application_alias() {
    assert_eq!(
        std::env::var("MDVIEWER_CONFIRM_REAL_WORKFLOW_INSTALL").as_deref(),
        Ok("yes")
    );
    let application = std::env::var_os("MDVIEWER_APPLICATION_BUNDLE")
        .map(PathBuf::from)
        .expect("MDVIEWER_APPLICATION_BUNDLE points to the signed development app");
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let manager =
        mdviewer_desktop_lib::macos_integration::manager_for_application(home, application)
            .unwrap();
    match manager.status().unwrap() {
        IntegrationStatus::NotInstalled => manager.install().unwrap(),
        IntegrationStatus::Installed => {}
        IntegrationStatus::Outdated => manager.repair().unwrap(),
        IntegrationStatus::Invalid => panic!("refusing to replace an unrelated PDF Service"),
    }
    assert_eq!(manager.status().unwrap(), IntegrationStatus::Installed);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires the freshly built and signed application bundle"]
fn signed_application_uses_its_bundled_pdfium_without_environment_configuration() {
    use std::process::Command;

    use mdviewer_desktop_lib::{
        commands::{authorize_save_selection, claim_print_job, convert_document, finish_print_job},
        configure_bundled_pdfium,
        jobs::PrintJobStore,
        state::AppState,
    };

    assert!(std::env::var_os("PDFIUM_DYNAMIC_LIB_PATH").is_none());
    let application = std::env::var_os("MDVIEWER_APPLICATION_BUNDLE")
        .map(PathBuf::from)
        .expect("MDVIEWER_APPLICATION_BUNDLE points to the signed development app");
    let runtime = application.join("Contents/Resources/lib/libpdfium.dylib");
    assert!(runtime.is_file());
    let signature = Command::new("/usr/bin/codesign")
        .args([
            "--verify",
            "--strict",
            "--test-requirement",
            "=anchor apple generic and certificate leaf[subject.OU] = \"NXJ8VR67NC\"",
        ])
        .arg(&runtime)
        .output()
        .unwrap();
    assert!(signature.status.success());
    configure_bundled_pdfium(&application.join("Contents/Resources")).unwrap();

    let directory = TempDir::new().unwrap();
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/pdf/digital-basic.pdf");
    let jobs = PrintJobStore::new(
        directory.path().join("print-jobs"),
        [input.parent().unwrap()],
    )
    .unwrap();
    let staged = jobs.stage_pdf(&input, Some("Packaged PDF")).unwrap();
    let state = AppState::new(jobs, directory.path().join("runtime")).unwrap();
    let claimed = claim_print_job(&state, &staged.id.to_string()).unwrap();
    let output = directory.path().join("packaged.md");
    let destination = authorize_save_selection(&state, &output).unwrap();
    convert_document(
        &state,
        &uuid::Uuid::new_v4().to_string(),
        &claimed.source_token,
        &destination.write_token,
    )
    .unwrap();
    finish_print_job(&state, &claimed.id).unwrap();

    assert!(output.is_file());
    assert!(!fs::read_to_string(output).unwrap().trim().is_empty());
    assert_eq!(fs::read_dir(state.jobs().root()).unwrap().count(), 0);
}
