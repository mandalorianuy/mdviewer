use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
use std::process::Command;

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub const WORKFLOW_NAME: &str = "Guardar como Markdown con MDViewer";
pub const WORKFLOW_MARKER: &str = "com.mdviewer.pdf-workflow/v1";
#[cfg(target_os = "macos")]
const WORKFLOW_SIGNATURE_IDENTITY: &str = "com.mdviewer.pdf-workflow";
#[cfg(target_os = "macos")]
const EMBEDDED_WORKFLOW: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mdviewer-pdf-workflow"));
#[cfg(target_os = "macos")]
include!(concat!(env!("OUT_DIR"), "/workflow_metadata.rs"));

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
    #[error("the workflow artifact is invalid")]
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
            Self::InvalidArtifact => "invalid_workflow_artifact",
            Self::TargetExists => "workflow_target_exists",
            Self::UnsafeTarget => "unsafe_workflow_target",
            Self::InvalidSignature => "invalid_workflow_signature",
            Self::Io => "workflow_io",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowArtifact {
    bytes: Vec<u8>,
    version: String,
    sha256: String,
    signature_identity: String,
}

impl WorkflowArtifact {
    pub fn new(
        bytes: Vec<u8>,
        version: impl Into<String>,
        sha256: impl Into<String>,
        signature_identity: impl Into<String>,
    ) -> Result<Self, IntegrationError> {
        let artifact = Self {
            bytes,
            version: version.into(),
            sha256: sha256.into(),
            signature_identity: signature_identity.into(),
        };
        if artifact.bytes.is_empty()
            || artifact.signature_identity.is_empty()
            || artifact.sha256 != sha256_hex(&artifact.bytes)
            || embedded_version(&artifact.bytes) != Some(artifact.version.as_str())
        {
            return Err(IntegrationError::InvalidArtifact);
        }
        Ok(artifact)
    }
}

pub trait CodeSignatureVerifier {
    fn verify(&self, path: &Path, identity: &str) -> io::Result<bool>;
}

#[derive(Debug, Clone, Copy)]
pub struct SystemCodeSignatureVerifier;

impl CodeSignatureVerifier for SystemCodeSignatureVerifier {
    fn verify(&self, path: &Path, identity: &str) -> io::Result<bool> {
        #[cfg(target_os = "macos")]
        {
            let verified = Command::new("/usr/bin/codesign")
                .args(["--verify", "--strict"])
                .arg(path)
                .status()?;
            if !verified.success() {
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
                    .any(|line| line == format!("Identifier={identity}")))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (path, identity);
            Ok(false)
        }
    }
}

pub struct IntegrationManager<V> {
    target: PathBuf,
    artifact: WorkflowArtifact,
    verifier: V,
}

impl<V: CodeSignatureVerifier> IntegrationManager<V> {
    pub fn new(
        home: impl AsRef<Path>,
        artifact: WorkflowArtifact,
        verifier: V,
    ) -> Result<Self, IntegrationError> {
        let home = home.as_ref();
        if !home.is_absolute() {
            return Err(IntegrationError::UnsafeTarget);
        }
        Ok(Self {
            target: home.join("Library/PDF Services").join(WORKFLOW_NAME),
            artifact,
            verifier,
        })
    }

    #[must_use]
    pub fn target(&self) -> PathBuf {
        self.target.clone()
    }

    pub fn status(&self) -> Result<IntegrationStatus, IntegrationError> {
        let metadata = match fs::symlink_metadata(&self.target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(IntegrationStatus::NotInstalled);
            }
            Err(_) => return Err(IntegrationError::Io),
        };
        if !metadata.file_type().is_file() {
            return Ok(IntegrationStatus::Invalid);
        }
        let bytes = fs::read(&self.target).map_err(|_| IntegrationError::Io)?;
        if !contains_marker(&bytes)
            || !self
                .verifier
                .verify(&self.target, &self.artifact.signature_identity)
                .map_err(|_| IntegrationError::Io)?
        {
            return Ok(IntegrationStatus::Invalid);
        }
        if embedded_version(&bytes) != Some(self.artifact.version.as_str()) {
            return Ok(IntegrationStatus::Outdated);
        }
        if sha256_hex(&bytes) != self.artifact.sha256 {
            return Ok(IntegrationStatus::Invalid);
        }
        Ok(IntegrationStatus::Installed)
    }

    pub fn install(&self) -> Result<(), IntegrationError> {
        if self.status()? != IntegrationStatus::NotInstalled {
            return Err(IntegrationError::TargetExists);
        }
        self.publish(false)
    }

    pub fn repair(&self) -> Result<(), IntegrationError> {
        match self.status()? {
            IntegrationStatus::NotInstalled => return Err(IntegrationError::UnsafeTarget),
            IntegrationStatus::Installed
            | IntegrationStatus::Outdated
            | IntegrationStatus::Invalid => {}
        }
        let metadata =
            fs::symlink_metadata(&self.target).map_err(|_| IntegrationError::UnsafeTarget)?;
        if !metadata.file_type().is_file()
            || !contains_marker(
                &fs::read(&self.target).map_err(|_| IntegrationError::UnsafeTarget)?,
            )
        {
            return Err(IntegrationError::UnsafeTarget);
        }
        self.publish(true)
    }

    pub fn uninstall(&self) -> Result<(), IntegrationError> {
        if self.status()? != IntegrationStatus::Installed {
            return Err(IntegrationError::UnsafeTarget);
        }
        fs::remove_file(&self.target).map_err(|_| IntegrationError::Io)?;
        sync_parent(&self.target)
    }

    fn publish(&self, replace: bool) -> Result<(), IntegrationError> {
        let parent = self.target.parent().ok_or(IntegrationError::UnsafeTarget)?;
        ensure_safe_parent(parent)?;
        let temporary = parent.join(format!(".{WORKFLOW_NAME}.install-{}", Uuid::new_v4()));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o700);
            }
            let mut file = options.open(&temporary).map_err(|_| IntegrationError::Io)?;
            file.write_all(&self.artifact.bytes)
                .map_err(|_| IntegrationError::Io)?;
            file.sync_all().map_err(|_| IntegrationError::Io)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))
                    .map_err(|_| IntegrationError::Io)?;
                file.sync_all().map_err(|_| IntegrationError::Io)?;
            }
            if !self
                .verifier
                .verify(&temporary, &self.artifact.signature_identity)
                .map_err(|_| IntegrationError::Io)?
            {
                return Err(IntegrationError::InvalidSignature);
            }
            if replace {
                fs::rename(&temporary, &self.target).map_err(|_| IntegrationError::Io)?;
            } else {
                fs::hard_link(&temporary, &self.target).map_err(|error| {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        IntegrationError::TargetExists
                    } else {
                        IntegrationError::Io
                    }
                })?;
                fs::remove_file(&temporary).map_err(|_| IntegrationError::Io)?;
            }
            sync_parent(&self.target)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

fn ensure_safe_parent(parent: &Path) -> Result<(), IntegrationError> {
    let library = parent.parent().ok_or(IntegrationError::UnsafeTarget)?;
    if library.exists() {
        let metadata = fs::symlink_metadata(library).map_err(|_| IntegrationError::Io)?;
        if !metadata.file_type().is_dir() {
            return Err(IntegrationError::UnsafeTarget);
        }
    } else {
        fs::create_dir_all(library).map_err(|_| IntegrationError::Io)?;
    }
    if parent.exists() {
        let metadata = fs::symlink_metadata(parent).map_err(|_| IntegrationError::Io)?;
        if !metadata.file_type().is_dir() {
            return Err(IntegrationError::UnsafeTarget);
        }
    } else {
        fs::create_dir(parent).map_err(|_| IntegrationError::Io)?;
    }
    Ok(())
}

fn contains_marker(bytes: &[u8]) -> bool {
    bytes
        .windows(WORKFLOW_MARKER.len())
        .any(|window| window == WORKFLOW_MARKER.as_bytes())
}

fn embedded_version(bytes: &[u8]) -> Option<&str> {
    const PREFIX: &[u8] = b"version=";
    let start = bytes
        .windows(PREFIX.len())
        .position(|window| window == PREFIX)?
        + PREFIX.len();
    let remainder = &bytes[start..];
    let end = remainder
        .iter()
        .position(|byte| *byte == b'\n' || *byte == 0)?;
    std::str::from_utf8(&remainder[..end]).ok()
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
pub fn embedded_manager()
-> Result<IntegrationManager<SystemCodeSignatureVerifier>, IntegrationError> {
    let home = std::env::var_os("HOME").ok_or(IntegrationError::UnsafeTarget)?;
    let artifact = WorkflowArtifact::new(
        EMBEDDED_WORKFLOW.to_vec(),
        env!("CARGO_PKG_VERSION"),
        EMBEDDED_WORKFLOW_SHA256,
        WORKFLOW_SIGNATURE_IDENTITY,
    )?;
    IntegrationManager::new(PathBuf::from(home), artifact, SystemCodeSignatureVerifier)
}
