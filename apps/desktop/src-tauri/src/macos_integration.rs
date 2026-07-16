use std::{
    fs::{self, File},
    io,
    path::{Component, Path, PathBuf},
};

#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::{ffi::CString, os::unix::ffi::OsStrExt};

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub const WORKFLOW_NAME: &str = "Guardar como Markdown con MDViewer";
pub const DESKTOP_BUNDLE_IDENTIFIER: &str = "com.mdviewer.desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationStatus {
    NotInstalled,
    Installed,
    Outdated,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IntegrationError {
    #[error("the application artifact is invalid")]
    InvalidArtifact,
    #[error("a workflow already exists")]
    TargetExists,
    #[error("the workflow target is unsafe")]
    UnsafeTarget,
    #[error("the workflow signature is invalid")]
    InvalidSignature,
    #[error("the workflow operation failed")]
    Io,
}

impl IntegrationError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidArtifact => "invalid_application_artifact",
            Self::TargetExists => "workflow_target_exists",
            Self::UnsafeTarget => "unsafe_workflow_target",
            Self::InvalidSignature => "invalid_application_signature",
            Self::Io => "workflow_io",
        }
    }
}

pub trait ApplicationAlias {
    fn create_alias(&self, application: &Path, destination: &Path) -> io::Result<()>;
    fn resolve_alias(&self, alias: &Path) -> io::Result<PathBuf>;
}

pub trait CodeSignatureVerifier {
    fn verify(&self, path: &Path, identity: &str, team: &str) -> io::Result<bool>;
}

#[derive(Debug, Clone)]
pub struct ApplicationArtifact {
    application: PathBuf,
    executable: PathBuf,
    sha256: String,
    signature_identity: String,
    team_identifier: String,
}

impl ApplicationArtifact {
    pub fn new(
        application: PathBuf,
        executable: PathBuf,
        sha256: impl Into<String>,
        signature_identity: impl Into<String>,
        team_identifier: impl Into<String>,
    ) -> Result<Self, IntegrationError> {
        let artifact = Self {
            application,
            executable,
            sha256: sha256.into(),
            signature_identity: signature_identity.into(),
            team_identifier: team_identifier.into(),
        };
        let relative_executable = !artifact.executable.as_os_str().is_empty()
            && !artifact.executable.is_absolute()
            && artifact
                .executable
                .components()
                .all(|component| matches!(component, Component::Normal(_)));
        let valid_identity = !artifact.signature_identity.is_empty()
            && artifact
                .signature_identity
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
        let valid_team = !artifact.team_identifier.is_empty()
            && artifact
                .team_identifier
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit());
        if !artifact.application.is_absolute()
            || !relative_executable
            || artifact.sha256.len() != 64
            || !artifact
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            || !valid_identity
            || !valid_team
        {
            return Err(IntegrationError::InvalidArtifact);
        }
        Ok(artifact)
    }

    fn executable_path(&self, application: &Path) -> PathBuf {
        application.join(&self.executable)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SystemCodeSignatureVerifier;

impl CodeSignatureVerifier for SystemCodeSignatureVerifier {
    fn verify(&self, path: &Path, identity: &str, team: &str) -> io::Result<bool> {
        #[cfg(target_os = "macos")]
        {
            let requirement =
                format!("=anchor apple generic and certificate leaf[subject.OU] = \"{team}\"");
            let verified = Command::new("/usr/bin/codesign")
                .args(["--verify", "--strict", "--test-requirement", &requirement])
                .arg(path)
                .output()?;
            if !verified.status.success() {
                return Ok(false);
            }
            let details = Command::new("/usr/bin/codesign")
                .args(["--display", "--verbose=2"])
                .arg(path)
                .output()?;
            let stderr = String::from_utf8_lossy(&details.stderr);
            Ok(details.status.success()
                && stderr
                    .lines()
                    .any(|line| line == format!("Identifier={identity}"))
                && stderr
                    .lines()
                    .any(|line| line == format!("TeamIdentifier={team}")))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (path, identity, team);
            Ok(false)
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy)]
pub struct NativeApplicationAlias;

#[cfg(target_os = "macos")]
impl ApplicationAlias for NativeApplicationAlias {
    fn create_alias(&self, application: &Path, destination: &Path) -> io::Result<()> {
        use objc2_foundation::{NSArray, NSString, NSURL, NSURLBookmarkCreationOptions};

        let application = NSURL::fileURLWithPath(&NSString::from_str(
            application
                .to_str()
                .ok_or_else(|| io::Error::other("invalid application path"))?,
        ));
        let destination = NSURL::fileURLWithPath(&NSString::from_str(
            destination
                .to_str()
                .ok_or_else(|| io::Error::other("invalid alias path"))?,
        ));
        let bookmark = application
            .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
                NSURLBookmarkCreationOptions::SuitableForBookmarkFile,
                None::<&NSArray<_>>,
                None,
            )
            .map_err(|_| io::Error::other("could not create application alias"))?;
        NSURL::writeBookmarkData_toURL_options_error(&bookmark, &destination, 0)
            .map_err(|_| io::Error::other("could not write application alias"))
    }

    fn resolve_alias(&self, alias: &Path) -> io::Result<PathBuf> {
        use objc2_foundation::{NSString, NSURL, NSURLBookmarkResolutionOptions};

        let alias = NSURL::fileURLWithPath(&NSString::from_str(
            alias
                .to_str()
                .ok_or_else(|| io::Error::other("invalid alias path"))?,
        ));
        let resolved = NSURL::URLByResolvingAliasFileAtURL_options_error(
            &alias,
            NSURLBookmarkResolutionOptions::WithoutUI
                | NSURLBookmarkResolutionOptions::WithoutMounting,
        )
        .map_err(|_| io::Error::other("could not resolve application alias"))?;
        let path = resolved
            .path()
            .ok_or_else(|| io::Error::other("application alias has no file path"))?;
        Ok(PathBuf::from(path.to_string()))
    }
}

pub struct IntegrationManager<A, V> {
    target: PathBuf,
    artifact: ApplicationArtifact,
    alias: A,
    verifier: V,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

struct TargetInspection {
    status: IntegrationStatus,
    identity: Option<FileIdentity>,
}

impl<A: ApplicationAlias, V: CodeSignatureVerifier> IntegrationManager<A, V> {
    pub fn new(
        home: impl AsRef<Path>,
        artifact: ApplicationArtifact,
        alias: A,
        verifier: V,
    ) -> Result<Self, IntegrationError> {
        let home = home.as_ref();
        if !home.is_absolute() {
            return Err(IntegrationError::UnsafeTarget);
        }
        Ok(Self {
            target: home.join("Library/PDF Services").join(WORKFLOW_NAME),
            artifact,
            alias,
            verifier,
        })
    }

    #[must_use]
    pub fn target(&self) -> PathBuf {
        self.target.clone()
    }

    pub fn status(&self) -> Result<IntegrationStatus, IntegrationError> {
        self.status_at(&self.target)
    }

    pub fn install(&self) -> Result<(), IntegrationError> {
        if self.status()? != IntegrationStatus::NotInstalled {
            return Err(IntegrationError::TargetExists);
        }
        self.publish(None)
    }

    pub fn repair(&self) -> Result<(), IntegrationError> {
        let inspected = self.inspect_target(&self.target)?;
        match inspected.status {
            IntegrationStatus::Installed | IntegrationStatus::Outdated => self.publish(Some(
                inspected.identity.ok_or(IntegrationError::UnsafeTarget)?,
            )),
            IntegrationStatus::NotInstalled | IntegrationStatus::Invalid => {
                Err(IntegrationError::UnsafeTarget)
            }
        }
    }

    pub fn uninstall(&self) -> Result<(), IntegrationError> {
        let inspected = self.inspect_target(&self.target)?;
        match inspected.status {
            IntegrationStatus::Installed | IntegrationStatus::Outdated => {}
            IntegrationStatus::NotInstalled | IntegrationStatus::Invalid => {
                return Err(IntegrationError::UnsafeTarget);
            }
        }
        let quarantine = self.quarantine_validated_target(
            inspected.identity.ok_or(IntegrationError::UnsafeTarget)?,
        )?;
        fs::remove_file(quarantine).map_err(|_| IntegrationError::Io)?;
        sync_parent(&self.target)
    }

    fn status_at(&self, target: &Path) -> Result<IntegrationStatus, IntegrationError> {
        Ok(self.inspect_target(target)?.status)
    }

    fn inspect_target(&self, target: &Path) -> Result<TargetInspection, IntegrationError> {
        let metadata = match fs::symlink_metadata(target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(TargetInspection {
                    status: IntegrationStatus::NotInstalled,
                    identity: None,
                });
            }
            Err(_) => return Err(IntegrationError::Io),
        };
        if !metadata.file_type().is_file() {
            return Ok(TargetInspection {
                status: IntegrationStatus::Invalid,
                identity: None,
            });
        }
        let identity = file_identity(&metadata);
        let application = match self.alias.resolve_alias(target) {
            Ok(application) if application.is_absolute() => application,
            Ok(_) | Err(_) => {
                return Ok(TargetInspection {
                    status: IntegrationStatus::Invalid,
                    identity,
                });
            }
        };
        if !self
            .verifier
            .verify(
                &application,
                &self.artifact.signature_identity,
                &self.artifact.team_identifier,
            )
            .map_err(|_| IntegrationError::Io)?
        {
            return Ok(TargetInspection {
                status: IntegrationStatus::Invalid,
                identity,
            });
        }
        let executable = self.artifact.executable_path(&application);
        let bytes = match fs::read(executable) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(TargetInspection {
                    status: IntegrationStatus::Invalid,
                    identity,
                });
            }
        };
        let status = if sha256_hex(&bytes) == self.artifact.sha256 {
            IntegrationStatus::Installed
        } else {
            IntegrationStatus::Outdated
        };
        Ok(TargetInspection { status, identity })
    }

    fn publish(&self, replace: Option<FileIdentity>) -> Result<(), IntegrationError> {
        if !self
            .verifier
            .verify(
                &self.artifact.application,
                &self.artifact.signature_identity,
                &self.artifact.team_identifier,
            )
            .map_err(|_| IntegrationError::Io)?
        {
            return Err(IntegrationError::InvalidSignature);
        }
        let executable = fs::read(self.artifact.executable_path(&self.artifact.application))
            .map_err(|_| IntegrationError::InvalidArtifact)?;
        if sha256_hex(&executable) != self.artifact.sha256 {
            return Err(IntegrationError::InvalidArtifact);
        }
        let parent = self.target.parent().ok_or(IntegrationError::UnsafeTarget)?;
        ensure_safe_parent(parent)?;
        let temporary = parent.join(format!(".{WORKFLOW_NAME}.install-{}", Uuid::new_v4()));
        let result = (|| {
            self.alias
                .create_alias(&self.artifact.application, &temporary)
                .map_err(|_| IntegrationError::Io)?;
            File::open(&temporary)
                .and_then(|file| file.sync_all())
                .map_err(|_| IntegrationError::Io)?;
            if self.status_at(&temporary)? != IntegrationStatus::Installed {
                return Err(IntegrationError::InvalidArtifact);
            }
            if let Some(expected) = replace {
                let quarantine = self.quarantine_validated_target(expected)?;
                if move_no_replace(&temporary, &self.target).is_err() {
                    let _ = move_no_replace(&quarantine, &self.target);
                    return Err(IntegrationError::UnsafeTarget);
                }
                fs::remove_file(quarantine).map_err(|_| IntegrationError::Io)?;
            } else {
                move_no_replace(&temporary, &self.target).map_err(|error| {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        IntegrationError::TargetExists
                    } else {
                        IntegrationError::Io
                    }
                })?;
            }
            sync_parent(&self.target)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn quarantine_validated_target(
        &self,
        expected: FileIdentity,
    ) -> Result<PathBuf, IntegrationError> {
        let parent = self.target.parent().ok_or(IntegrationError::UnsafeTarget)?;
        let quarantine = parent.join(format!(".{WORKFLOW_NAME}.quarantine-{}", Uuid::new_v4()));
        move_no_replace(&self.target, &quarantine).map_err(|_| IntegrationError::UnsafeTarget)?;

        let moved = fs::symlink_metadata(&quarantine).ok();
        let identity_matches = moved
            .as_ref()
            .and_then(file_identity)
            .is_some_and(|identity| identity == expected);
        let status_matches = identity_matches
            && matches!(
                self.status_at(&quarantine),
                Ok(IntegrationStatus::Installed | IntegrationStatus::Outdated)
            );
        let identity_still_matches = status_matches
            && fs::symlink_metadata(&quarantine)
                .ok()
                .as_ref()
                .and_then(file_identity)
                .is_some_and(|identity| identity == expected);
        if !identity_still_matches {
            let _ = move_no_replace(&quarantine, &self.target);
            return Err(IntegrationError::UnsafeTarget);
        }
        Ok(quarantine)
    }
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    Some(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn file_identity(_metadata: &fs::Metadata) -> Option<FileIdentity> {
    None
}

#[cfg(target_os = "macos")]
fn move_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid source path"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid destination path"))?;
    // SAFETY: both C strings are alive for the call and contain no interior NUL bytes.
    let result = unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn move_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::hard_link(source, destination)?;
    fs::remove_file(source)
}

fn ensure_safe_parent(parent: &Path) -> Result<(), IntegrationError> {
    let library = parent.parent().ok_or(IntegrationError::UnsafeTarget)?;
    if library.exists() {
        let metadata = fs::symlink_metadata(library).map_err(|_| IntegrationError::Io)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(IntegrationError::UnsafeTarget);
        }
    } else {
        fs::create_dir_all(library).map_err(|_| IntegrationError::Io)?;
    }
    if parent.exists() {
        let metadata = fs::symlink_metadata(parent).map_err(|_| IntegrationError::Io)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(IntegrationError::UnsafeTarget);
        }
    } else {
        fs::create_dir(parent).map_err(|_| IntegrationError::Io)?;
    }
    Ok(())
}

fn sync_parent(path: &Path) -> Result<(), IntegrationError> {
    File::open(path.parent().ok_or(IntegrationError::UnsafeTarget)?)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| IntegrationError::Io)
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(target_os = "macos")]
fn current_application_bundle() -> Result<(PathBuf, PathBuf), IntegrationError> {
    let executable = std::env::current_exe().map_err(|_| IntegrationError::InvalidArtifact)?;
    let application = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .ok_or(IntegrationError::InvalidArtifact)?
        .to_path_buf();
    let relative = executable
        .strip_prefix(&application)
        .map_err(|_| IntegrationError::InvalidArtifact)?
        .to_path_buf();
    Ok((application, relative))
}

#[cfg(target_os = "macos")]
fn current_code_identity(application: &Path) -> Result<(String, String), IntegrationError> {
    let details = Command::new("/usr/bin/codesign")
        .args(["--display", "--verbose=2"])
        .arg(application)
        .output()
        .map_err(|_| IntegrationError::InvalidSignature)?;
    if !details.status.success() {
        return Err(IntegrationError::InvalidSignature);
    }
    let stderr = String::from_utf8_lossy(&details.stderr);
    let value = |prefix: &str| {
        stderr
            .lines()
            .find_map(|line| line.strip_prefix(prefix))
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let identity = value("Identifier=").ok_or(IntegrationError::InvalidSignature)?;
    let team = value("TeamIdentifier=").ok_or(IntegrationError::InvalidSignature)?;
    Ok((identity, team))
}

#[cfg(target_os = "macos")]
pub fn embedded_manager()
-> Result<IntegrationManager<NativeApplicationAlias, SystemCodeSignatureVerifier>, IntegrationError>
{
    let home = std::env::var_os("HOME").ok_or(IntegrationError::UnsafeTarget)?;
    let (application, executable) = current_application_bundle()?;
    let bytes =
        fs::read(application.join(&executable)).map_err(|_| IntegrationError::InvalidArtifact)?;
    let (identity, team) = current_code_identity(&application)?;
    if identity != DESKTOP_BUNDLE_IDENTIFIER {
        return Err(IntegrationError::InvalidSignature);
    }
    let artifact =
        ApplicationArtifact::new(application, executable, sha256_hex(&bytes), identity, team)?;
    IntegrationManager::new(
        PathBuf::from(home),
        artifact,
        NativeApplicationAlias,
        SystemCodeSignatureVerifier,
    )
}

#[cfg(target_os = "macos")]
pub fn manager_for_application(
    home: impl AsRef<Path>,
    application: PathBuf,
) -> Result<IntegrationManager<NativeApplicationAlias, SystemCodeSignatureVerifier>, IntegrationError>
{
    let executable = PathBuf::from("Contents/MacOS/mdviewer-desktop");
    let bytes =
        fs::read(application.join(&executable)).map_err(|_| IntegrationError::InvalidArtifact)?;
    let (identity, team) = current_code_identity(&application)?;
    if identity != DESKTOP_BUNDLE_IDENTIFIER {
        return Err(IntegrationError::InvalidSignature);
    }
    let artifact =
        ApplicationArtifact::new(application, executable, sha256_hex(&bytes), identity, team)?;
    IntegrationManager::new(
        home,
        artifact,
        NativeApplicationAlias,
        SystemCodeSignatureVerifier,
    )
}
