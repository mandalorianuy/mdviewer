use std::{fs, io, path::Path};

use mdviewer_desktop_lib::macos_integration::{
    CodeSignatureVerifier, IntegrationError, IntegrationManager, IntegrationStatus,
    WORKFLOW_MARKER, WORKFLOW_NAME, WorkflowArtifact, sha256_hex,
};
use tempfile::TempDir;

#[derive(Clone, Copy)]
struct FixtureSignature;

impl CodeSignatureVerifier for FixtureSignature {
    fn verify(&self, path: &Path, identity: &str) -> io::Result<bool> {
        let bytes = fs::read(path)?;
        Ok(identity == "com.mdviewer.pdf-workflow" && !bytes.ends_with(b"BAD-SIGNATURE"))
    }
}

fn bytes(version: &str) -> Vec<u8> {
    format!("#!/bin/false\n{WORKFLOW_MARKER}\nversion={version}\n").into_bytes()
}

fn artifact(version: &str) -> WorkflowArtifact {
    let bytes = bytes(version);
    WorkflowArtifact::new(
        bytes.clone(),
        version,
        sha256_hex(&bytes),
        "com.mdviewer.pdf-workflow",
    )
    .unwrap()
}

fn manager(home: &TempDir) -> IntegrationManager<FixtureSignature> {
    IntegrationManager::new(home.path(), artifact("0.1.0"), FixtureSignature).unwrap()
}

#[test]
fn install_uses_the_exact_per_user_path_and_reports_installed() {
    let home = TempDir::new().unwrap();
    let manager = manager(&home);

    assert_eq!(manager.status().unwrap(), IntegrationStatus::NotInstalled);
    manager.install().unwrap();

    let target = home.path().join("Library/PDF Services").join(WORKFLOW_NAME);
    assert_eq!(manager.target(), target);
    assert_eq!(fs::read(&target).unwrap(), bytes("0.1.0"));
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
fn version_and_checksum_mismatches_are_distinguished_and_repair_is_atomic() {
    let home = TempDir::new().unwrap();
    let manager = manager(&home);
    fs::create_dir_all(manager.target().parent().unwrap()).unwrap();
    fs::write(manager.target(), bytes("0.0.9")).unwrap();
    assert_eq!(manager.status().unwrap(), IntegrationStatus::Outdated);

    manager.repair().unwrap();
    assert_eq!(manager.status().unwrap(), IntegrationStatus::Installed);

    fs::write(
        manager.target(),
        [bytes("0.1.0"), b"tampered".to_vec()].concat(),
    )
    .unwrap();
    assert_eq!(manager.status().unwrap(), IntegrationStatus::Invalid);
    manager.repair().unwrap();
    assert_eq!(manager.status().unwrap(), IntegrationStatus::Installed);
}

#[test]
fn invalid_signature_is_never_reported_as_installed() {
    let home = TempDir::new().unwrap();
    let manager = manager(&home);
    manager.install().unwrap();
    fs::write(
        manager.target(),
        [bytes("0.1.0"), b"BAD-SIGNATURE".to_vec()].concat(),
    )
    .unwrap();

    assert_eq!(manager.status().unwrap(), IntegrationStatus::Invalid);
}

#[test]
fn uninstall_removes_only_the_exact_current_regular_file() {
    let home = TempDir::new().unwrap();
    let manager = manager(&home);
    manager.install().unwrap();
    let sibling = manager.target().parent().unwrap().join("Keep Me");
    fs::write(&sibling, b"unrelated").unwrap();

    manager.uninstall().unwrap();

    assert_eq!(manager.status().unwrap(), IntegrationStatus::NotInstalled);
    assert_eq!(fs::read(sibling).unwrap(), b"unrelated");
    assert!(manager.target().parent().unwrap().is_dir());
}

#[test]
fn install_and_uninstall_refuse_unrelated_or_tampered_targets() {
    let home = TempDir::new().unwrap();
    let manager = manager(&home);
    fs::create_dir_all(manager.target().parent().unwrap()).unwrap();
    fs::write(manager.target(), b"unrelated executable").unwrap();

    assert_eq!(
        manager.install().unwrap_err(),
        IntegrationError::TargetExists
    );
    assert_eq!(
        manager.repair().unwrap_err(),
        IntegrationError::UnsafeTarget
    );
    assert_eq!(
        manager.uninstall().unwrap_err(),
        IntegrationError::UnsafeTarget
    );
    assert_eq!(fs::read(manager.target()).unwrap(), b"unrelated executable");
}

#[cfg(unix)]
#[test]
fn symlink_targets_are_invalid_and_are_never_followed() {
    use std::os::unix::fs::symlink;

    let home = TempDir::new().unwrap();
    let manager = manager(&home);
    fs::create_dir_all(manager.target().parent().unwrap()).unwrap();
    let outside = home.path().join("outside");
    fs::write(&outside, bytes("0.1.0")).unwrap();
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
    assert_eq!(fs::read(outside).unwrap(), bytes("0.1.0"));
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "writes the explicitly authorized per-user PDF Services target"]
fn installs_the_explicit_embedded_development_artifact() {
    assert_eq!(
        std::env::var("MDVIEWER_CONFIRM_REAL_WORKFLOW_INSTALL").as_deref(),
        Ok("yes")
    );
    let manager = mdviewer_desktop_lib::macos_integration::embedded_manager().unwrap();
    match manager.status().unwrap() {
        IntegrationStatus::NotInstalled => manager.install().unwrap(),
        IntegrationStatus::Installed => {}
        IntegrationStatus::Outdated | IntegrationStatus::Invalid => manager.repair().unwrap(),
    }
    assert_eq!(manager.status().unwrap(), IntegrationStatus::Installed);
}
