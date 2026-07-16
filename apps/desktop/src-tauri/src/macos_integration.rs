use std::{
    ffi::OsStr,
    fs::{self, File},
    io,
    path::{Component, Path, PathBuf},
};

#[cfg(target_os = "macos")]
use std::process::Command;

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

#[doc(hidden)]
pub trait RetentionInterlock {
    fn after_directories_opened(&self) -> io::Result<()>;

    fn after_temporary_validated(&self, _temporary: &Path) -> io::Result<()> {
        Ok(())
    }
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct NoopRetentionInterlock;

impl RetentionInterlock for NoopRetentionInterlock {
    fn after_directories_opened(&self) -> io::Result<()> {
        Ok(())
    }
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

pub struct IntegrationManager<A, V, H = NoopRetentionInterlock> {
    home: PathBuf,
    target: PathBuf,
    retained: PathBuf,
    artifact: ApplicationArtifact,
    alias: A,
    verifier: V,
    retention_interlock: H,
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

impl<A: ApplicationAlias, V: CodeSignatureVerifier>
    IntegrationManager<A, V, NoopRetentionInterlock>
{
    pub fn new(
        home: impl AsRef<Path>,
        artifact: ApplicationArtifact,
        alias: A,
        verifier: V,
    ) -> Result<Self, IntegrationError> {
        IntegrationManager::new_with_retention_interlock(
            home,
            artifact,
            alias,
            verifier,
            NoopRetentionInterlock,
        )
    }
}

impl<A: ApplicationAlias, V: CodeSignatureVerifier, H: RetentionInterlock>
    IntegrationManager<A, V, H>
{
    #[doc(hidden)]
    pub fn new_with_retention_interlock(
        home: impl AsRef<Path>,
        artifact: ApplicationArtifact,
        alias: A,
        verifier: V,
        retention_interlock: H,
    ) -> Result<Self, IntegrationError> {
        let home = home.as_ref();
        if !home.is_absolute() {
            return Err(IntegrationError::UnsafeTarget);
        }
        Ok(Self {
            home: home.to_path_buf(),
            target: home.join("Library/PDF Services").join(WORKFLOW_NAME),
            retained: home
                .join("Library/Application Support/com.mdviewer.desktop/Retired PDF Services"),
            artifact,
            alias,
            verifier,
            retention_interlock,
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
        self.retain_or_restore(quarantine)?;
        Ok(())
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
        ensure_safe_descendant_directories(&self.home, parent)?;
        let temporary = parent.join(format!(".{WORKFLOW_NAME}.install-{}", Uuid::new_v4()));
        let result = (|| {
            self.alias
                .create_alias(&self.artifact.application, &temporary)
                .map_err(|_| IntegrationError::Io)?;
            File::open(&temporary)
                .and_then(|file| file.sync_all())
                .map_err(|_| IntegrationError::Io)?;
            let inspected_temporary = self.inspect_target(&temporary)?;
            if inspected_temporary.status != IntegrationStatus::Installed {
                return Err(IntegrationError::InvalidArtifact);
            }
            let temporary_identity = inspected_temporary
                .identity
                .ok_or(IntegrationError::InvalidArtifact)?;
            self.retention_interlock
                .after_temporary_validated(&temporary)
                .map_err(|_| IntegrationError::Io)?;
            let previous = if let Some(expected) = replace {
                let quarantine = self.quarantine_validated_target(expected)?;
                let retained = self.retain_or_restore(quarantine)?;
                Some((retained, expected))
            } else {
                None
            };
            if let Err(error) = self.move_no_replace(&temporary, &self.target) {
                if let Some((retained, expected)) = previous.as_ref() {
                    let _ = self.restore_owned_workflow(retained, *expected);
                }
                return Err(
                    if replace.is_none() && error.kind() == io::ErrorKind::AlreadyExists {
                        IntegrationError::TargetExists
                    } else {
                        IntegrationError::UnsafeTarget
                    },
                );
            }
            if !self.target_matches_identity_and_status(
                &self.target,
                temporary_identity,
                |status| status == IntegrationStatus::Installed,
            ) {
                if self.retain_path(&self.target).is_ok()
                    && let Some((retained, expected)) = previous.as_ref()
                {
                    let _ = self.restore_owned_workflow(retained, *expected);
                }
                return Err(IntegrationError::UnsafeTarget);
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = self.retain_path(&temporary);
        }
        result
    }

    fn quarantine_validated_target(
        &self,
        expected: FileIdentity,
    ) -> Result<PathBuf, IntegrationError> {
        let parent = self.target.parent().ok_or(IntegrationError::UnsafeTarget)?;
        let quarantine = parent.join(format!(".{WORKFLOW_NAME}.quarantine-{}", Uuid::new_v4()));
        self.move_no_replace(&self.target, &quarantine)
            .map_err(|_| IntegrationError::UnsafeTarget)?;

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
            let _ = self.move_no_replace(&quarantine, &self.target);
            return Err(IntegrationError::UnsafeTarget);
        }
        Ok(quarantine)
    }

    fn retain_or_restore(&self, quarantine: PathBuf) -> Result<PathBuf, IntegrationError> {
        match self.retain_path(&quarantine) {
            Ok(retained) => Ok(retained),
            Err(error) => {
                let _ = self.move_no_replace(&quarantine, &self.target);
                Err(error)
            }
        }
    }

    fn retain_path(&self, source: &Path) -> Result<PathBuf, IntegrationError> {
        match fs::symlink_metadata(source) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(IntegrationError::UnsafeTarget);
            }
            Err(_) => return Err(IntegrationError::Io),
        }
        ensure_safe_descendant_directories(&self.home, &self.retained)?;
        let retained = self
            .retained
            .join(format!("{WORKFLOW_NAME}-{}.retained", Uuid::new_v4()));
        let source_directory = VerifiedDirectory::open_descendant(
            &self.home,
            source.parent().ok_or(IntegrationError::UnsafeTarget)?,
        )
        .map_err(|_| IntegrationError::UnsafeTarget)?;
        let retained_directory = VerifiedDirectory::open_descendant(&self.home, &self.retained)
            .map_err(|_| IntegrationError::UnsafeTarget)?;
        self.retention_interlock
            .after_directories_opened()
            .map_err(|_| IntegrationError::Io)?;
        move_between_verified_directories_no_replace(
            &source_directory,
            file_name(source)?,
            &retained_directory,
            file_name(&retained)?,
        )
        .map_err(|_| IntegrationError::UnsafeTarget)?;
        Ok(retained)
    }

    fn move_no_replace(&self, source: &Path, destination: &Path) -> io::Result<()> {
        let source_directory = VerifiedDirectory::open_descendant(
            &self.home,
            source
                .parent()
                .ok_or_else(|| io::Error::other("source has no parent"))?,
        )?;
        let destination_directory = VerifiedDirectory::open_descendant(
            &self.home,
            destination
                .parent()
                .ok_or_else(|| io::Error::other("destination has no parent"))?,
        )?;
        move_between_verified_directories_no_replace(
            &source_directory,
            file_name_io(source)?,
            &destination_directory,
            file_name_io(destination)?,
        )
    }

    fn restore_owned_workflow(
        &self,
        retained: &Path,
        expected: FileIdentity,
    ) -> Result<(), IntegrationError> {
        self.move_no_replace(retained, &self.target)
            .map_err(|_| IntegrationError::UnsafeTarget)?;
        if self.target_matches_identity_and_status(&self.target, expected, |status| {
            matches!(
                status,
                IntegrationStatus::Installed | IntegrationStatus::Outdated
            )
        }) {
            return Ok(());
        }
        let _ = self.retain_path(&self.target);
        Err(IntegrationError::UnsafeTarget)
    }

    fn target_matches_identity_and_status(
        &self,
        target: &Path,
        expected: FileIdentity,
        accepts: impl FnOnce(IntegrationStatus) -> bool,
    ) -> bool {
        let Ok(inspected) = self.inspect_target(target) else {
            return false;
        };
        if inspected.identity != Some(expected) || !accepts(inspected.status) {
            return false;
        }
        fs::symlink_metadata(target)
            .ok()
            .as_ref()
            .and_then(file_identity)
            == Some(expected)
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectoryIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    volume: u64,
    #[cfg(windows)]
    file_id: [u8; 16],
}

struct VerifiedDirectory {
    home: PathBuf,
    path: PathBuf,
    file: File,
    identity: DirectoryIdentity,
}

impl VerifiedDirectory {
    fn open_descendant(home: &Path, path: &Path) -> io::Result<Self> {
        let file = open_directory_chain(home, path)?;
        let identity = directory_identity(&file)?;
        Ok(Self {
            home: home.to_path_buf(),
            path: path.to_path_buf(),
            file,
            identity,
        })
    }

    fn path_still_matches(&self) -> io::Result<bool> {
        let current = open_directory_chain(&self.home, &self.path)?;
        Ok(directory_identity(&current)? == self.identity)
    }
}

fn move_between_verified_directories_no_replace(
    source: &VerifiedDirectory,
    source_name: &OsStr,
    destination: &VerifiedDirectory,
    destination_name: &OsStr,
) -> io::Result<()> {
    if !source.path_still_matches()? || !destination.path_still_matches()? {
        return Err(io::Error::other("directory path changed before move"));
    }
    move_relative_no_replace(source, source_name, destination, destination_name)?;
    let durable = source
        .file
        .sync_all()
        .and_then(|_| destination.file.sync_all())
        .is_ok();
    if durable
        && source.path_still_matches().unwrap_or(false)
        && destination.path_still_matches().unwrap_or(false)
    {
        return Ok(());
    }
    let _ = move_relative_no_replace(destination, destination_name, source, source_name);
    let _ = source.file.sync_all();
    let _ = destination.file.sync_all();
    Err(io::Error::other("directory path changed during move"))
}

fn open_directory_chain(home: &Path, path: &Path) -> io::Result<File> {
    let relative = path
        .strip_prefix(home)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path is outside home"))?;
    let mut directory = open_directory_no_follow(home)?;
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid directory component",
            ));
        };
        directory = open_relative_directory_no_follow(&directory, component)?;
    }
    Ok(directory)
}

#[cfg(unix)]
fn open_directory_no_follow(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

#[cfg(unix)]
fn open_relative_directory_no_follow(parent: &File, name: &OsStr) -> io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let name = std::ffi::CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid directory name"))?;
    // SAFETY: parent is a live directory fd and name is a valid C string.
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: openat returned a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

#[cfg(windows)]
fn open_directory_no_follow(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
        FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
    };

    let directory = fs::OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | SYNCHRONIZE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(path)?;
    validate_windows_directory(&directory)?;
    Ok(directory)
}

#[cfg(windows)]
fn open_relative_directory_no_follow(parent: &File, name: &OsStr) -> io::Result<File> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_OPEN,
        Win32::Storage::FileSystem::{FILE_GENERIC_READ, FILE_GENERIC_WRITE, SYNCHRONIZE},
    };

    let directory = crate::state::windows_nt_open_relative(
        parent,
        name,
        FILE_GENERIC_READ | FILE_GENERIC_WRITE | SYNCHRONIZE,
        FILE_OPEN,
        Some(true),
    )?;
    validate_windows_directory(&directory)?;
    Ok(directory)
}

#[cfg(windows)]
fn validate_windows_directory(directory: &File) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reparse directory rejected",
        ));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn open_directory_no_follow(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "verified directories are unsupported on this platform",
    ))
}

#[cfg(not(any(unix, windows)))]
fn open_relative_directory_no_follow(_parent: &File, _name: &OsStr) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "verified directories are unsupported on this platform",
    ))
}

#[cfg(unix)]
fn directory_identity(directory: &File) -> io::Result<DirectoryIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = directory.metadata()?;
    Ok(DirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn directory_identity(directory: &File) -> io::Result<DirectoryIdentity> {
    use std::{mem::size_of, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };

    let mut identity = FILE_ID_INFO::default();
    // SAFETY: identity is a correctly sized writable FILE_ID_INFO buffer.
    if unsafe {
        GetFileInformationByHandleEx(
            directory.as_raw_handle(),
            FileIdInfo,
            (&raw mut identity).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(DirectoryIdentity {
        volume: identity.VolumeSerialNumber,
        file_id: identity.FileId.Identifier,
    })
}

#[cfg(not(any(unix, windows)))]
fn directory_identity(_directory: &File) -> io::Result<DirectoryIdentity> {
    Ok(DirectoryIdentity {})
}

#[cfg(target_os = "macos")]
fn move_relative_no_replace(
    source: &VerifiedDirectory,
    source_name: &OsStr,
    destination: &VerifiedDirectory,
    destination_name: &OsStr,
) -> io::Result<()> {
    use std::os::{fd::AsRawFd, unix::ffi::OsStrExt};

    let source_name = std::ffi::CString::new(source_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid source name"))?;
    let destination_name = std::ffi::CString::new(destination_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid destination name"))?;
    // SAFETY: both directory fds and C strings are live for the call.
    let result = unsafe {
        libc::renameatx_np(
            source.file.as_raw_fd(),
            source_name.as_ptr(),
            destination.file.as_raw_fd(),
            destination_name.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn move_relative_no_replace(
    source: &VerifiedDirectory,
    source_name: &OsStr,
    destination: &VerifiedDirectory,
    destination_name: &OsStr,
) -> io::Result<()> {
    use std::os::{fd::AsRawFd, unix::ffi::OsStrExt};

    let source_name = std::ffi::CString::new(source_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid source name"))?;
    let destination_name = std::ffi::CString::new(destination_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid destination name"))?;
    // SAFETY: both directory fds and C strings are live for the call.
    let result = unsafe {
        libc::renameat2(
            source.file.as_raw_fd(),
            source_name.as_ptr(),
            destination.file.as_raw_fd(),
            destination_name.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn move_relative_no_replace(
    source: &VerifiedDirectory,
    source_name: &OsStr,
    destination: &VerifiedDirectory,
    destination_name: &OsStr,
) -> io::Result<()> {
    let source_file = crate::state::windows_open_relative_for_mutation(&source.file, source_name)?;
    crate::state::windows_rename_open_handle(&source_file, &destination.file, destination_name)
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "android",
    target_os = "windows"
)))]
fn move_relative_no_replace(
    _source: &VerifiedDirectory,
    _source_name: &OsStr,
    _destination: &VerifiedDirectory,
    _destination_name: &OsStr,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "exclusive relative rename is unsupported on this platform",
    ))
}

fn file_name(path: &Path) -> Result<&OsStr, IntegrationError> {
    file_name_io(path).map_err(|_| IntegrationError::UnsafeTarget)
}

fn file_name_io(path: &Path) -> io::Result<&OsStr> {
    path.file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))
}

#[cfg(unix)]
fn ensure_safe_descendant_directories(
    ancestor: &Path,
    directory: &Path,
) -> Result<(), IntegrationError> {
    let relative = directory
        .strip_prefix(ancestor)
        .map_err(|_| IntegrationError::UnsafeTarget)?;
    let mut current = open_directory_no_follow(ancestor).map_err(|_| IntegrationError::Io)?;
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(IntegrationError::UnsafeTarget);
        };
        match open_relative_directory_no_follow(&current, component) {
            Ok(next) => current = next,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                create_relative_directory(&current, component)?;
                current.sync_all().map_err(|_| IntegrationError::Io)?;
                current = open_relative_directory_no_follow(&current, component)
                    .map_err(|_| IntegrationError::UnsafeTarget)?;
            }
            Err(_) => return Err(IntegrationError::UnsafeTarget),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_relative_directory(parent: &File, name: &OsStr) -> Result<(), IntegrationError> {
    use std::os::{fd::AsRawFd, unix::ffi::OsStrExt};

    let name =
        std::ffi::CString::new(name.as_bytes()).map_err(|_| IntegrationError::UnsafeTarget)?;
    // SAFETY: parent is a live directory fd and name is a valid C string.
    let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
    if result == 0 {
        Ok(())
    } else {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::AlreadyExists {
            Ok(())
        } else {
            Err(IntegrationError::Io)
        }
    }
}

#[cfg(windows)]
fn ensure_safe_descendant_directories(
    ancestor: &Path,
    directory: &Path,
) -> Result<(), IntegrationError> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_OPEN_IF,
        Win32::Storage::FileSystem::{FILE_GENERIC_READ, FILE_GENERIC_WRITE, SYNCHRONIZE},
    };

    let relative = directory
        .strip_prefix(ancestor)
        .map_err(|_| IntegrationError::UnsafeTarget)?;
    let mut current = open_directory_no_follow(ancestor).map_err(|_| IntegrationError::Io)?;
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(IntegrationError::UnsafeTarget);
        };
        match open_relative_directory_no_follow(&current, component) {
            Ok(next) => current = next,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let next = crate::state::windows_nt_open_relative(
                    &current,
                    component,
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE | SYNCHRONIZE,
                    FILE_OPEN_IF,
                    Some(true),
                )
                .map_err(|_| IntegrationError::Io)?;
                validate_windows_directory(&next).map_err(|_| IntegrationError::UnsafeTarget)?;
                current.sync_all().map_err(|_| IntegrationError::Io)?;
                current = next;
            }
            Err(_) => return Err(IntegrationError::UnsafeTarget),
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn ensure_safe_descendant_directories(
    _ancestor: &Path,
    _directory: &Path,
) -> Result<(), IntegrationError> {
    Err(IntegrationError::UnsafeTarget)
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
