use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
    time::{SystemTime, UNIX_EPOCH},
};

use tempfile::{Builder, TempDir};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::{
    ConversionWarning, Document, EmitError, GfmOptions, emit_gfm_with_asset_prefix,
    is_windows_reserved_component,
    manifest::{AssetManifest, MANIFEST_FILE_NAME, SCHEMA_VERSION, sha256_hex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverwritePolicy {
    Deny,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputTarget {
    pub markdown_path: PathBuf,
    pub overwrite: OverwritePolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WriteResult {
    pub markdown_path: PathBuf,
    pub assets_dir: Option<PathBuf>,
    pub warnings: Vec<ConversionWarning>,
}

pub trait Cancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NeverCancel;

impl Cancellation for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Debug, Error)]
pub enum OutputError {
    #[error("output publication was cancelled")]
    Cancelled,
    #[error("invalid output target: {0}")]
    InvalidTarget(PathBuf),
    #[error("output already exists: {0}")]
    OutputExists(PathBuf),
    #[error("assets directory is not owned by mdviewer: {0}")]
    UnownedAssetsDirectory(PathBuf),
    #[error("invalid assets manifest at {path}: {message}")]
    InvalidManifest { path: PathBuf, message: String },
    #[error("invalid asset file name: {0:?}")]
    InvalidAssetFileName(String),
    #[error("duplicate asset file name: {0:?}")]
    DuplicateAssetFileName(String),
    #[error("could not emit Markdown: {0}")]
    Emit(#[from] EmitError),
    #[error("I/O error while attempting to {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("output transaction failed: {message}")]
    TransactionFailed { message: String },
}

pub fn publish(
    document: &Document,
    target: &OutputTarget,
    cancellation: &dyn Cancellation,
) -> Result<WriteResult, OutputError> {
    publish_with_fs(document, target, cancellation, &RealFs)
}

fn publish_with_fs(
    document: &Document,
    target: &OutputTarget,
    cancellation: &dyn Cancellation,
    fs_ops: &dyn FsOps,
) -> Result<WriteResult, OutputError> {
    if cancellation.is_cancelled() {
        return Err(OutputError::Cancelled);
    }

    let paths = PublicationPaths::new(&target.markdown_path)?;
    validate_asset_file_names(document)?;
    let initial_outputs = inspect_existing_outputs(&paths)?;
    enforce_initial_policy(target.overwrite, &paths, &initial_outputs)?;

    let markdown = emit_gfm_with_asset_prefix(
        document,
        &GfmOptions {
            final_newline: true,
        },
        &paths.assets_name,
    )?;

    let staging = Builder::new()
        .prefix(".mdviewer-output-")
        .tempdir_in(&paths.parent)
        .map_err(|source| io_error("create staging directory", &paths.parent, source))
        .map_err(|error| paths.report_error(error))?;
    let staged_markdown = staging.path().join("new.md");
    write_synced_file(
        &staged_markdown,
        markdown.as_bytes(),
        "write staged Markdown",
    )?;

    let staged_assets = if document.assets.is_empty() {
        None
    } else {
        let directory = staging.path().join("new.assets");
        create_directory(&directory, "create staged assets directory")?;
        for asset in &document.assets {
            write_synced_file(
                &directory.join(&asset.file_name),
                &asset.data,
                "write staged asset",
            )?;
        }
        let manifest = AssetManifest::new(&paths.document_name, &document.assets);
        let mut manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|error| {
            OutputError::TransactionFailed {
                message: format!("could not serialize assets manifest: {error}"),
            }
        })?;
        manifest_json.push(b'\n');
        write_synced_file(
            &directory.join(MANIFEST_FILE_NAME),
            &manifest_json,
            "write staged assets manifest",
        )?;
        sync_directory(fs_ops, &directory)?;
        Some(directory)
    };
    sync_directory(fs_ops, staging.path())?;

    if cancellation.is_cancelled() {
        return Err(OutputError::Cancelled);
    }

    let mut lock =
        TargetLock::acquire(&paths, fs_ops).map_err(|error| paths.report_error(error))?;
    let commit_result = commit(
        &paths,
        staging.path(),
        &staged_markdown,
        staged_assets.as_deref(),
        target.overwrite,
        &initial_outputs,
        fs_ops,
    );

    let commit_state = match commit_result {
        Ok(state) => state,
        Err(failure) => {
            let failure = release_after_rollback(&mut lock, fs_ops, failure);
            return finish_failure(failure, staging);
        }
    };

    if let Err(lock_error) = lock.release(fs_ops) {
        let failure = if lock.is_held() {
            let failure = abort_transaction(
                commit_state,
                &paths,
                staging.path(),
                lock_error,
                fs_ops,
                None,
            );
            release_after_rollback(&mut lock, fs_ops, failure)
        } else {
            CommitFailure::Preserve(format!(
                "{lock_error}; output lock could not be retained for rollback"
            ))
        };
        return finish_failure(failure, staging);
    }

    Ok(WriteResult {
        markdown_path: target.markdown_path.clone(),
        assets_dir: (!document.assets.is_empty()).then_some(paths.reported_assets_dir),
        warnings: document.warnings.clone(),
    })
}

fn release_after_rollback(
    lock: &mut TargetLock,
    fs_ops: &dyn FsOps,
    failure: CommitFailure,
) -> CommitFailure {
    match lock.release(fs_ops) {
        Ok(()) => failure,
        Err(first_error) if lock.is_held() => match lock.release(fs_ops) {
            Ok(()) => failure,
            Err(second_error) => failure.with_recovery(format!(
                "lock release failed twice: {first_error}; {second_error}"
            )),
        },
        Err(error) => failure.with_recovery(format!("lock release failed: {error}")),
    }
}

trait FsOps {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::hard_link(from, to)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        fs::create_dir(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn sync_directory(&self, path: &Path) -> io::Result<()> {
        sync_directory_raw(path)
    }

    fn before_commit(&self, _paths: &PublicationPaths) -> io::Result<()> {
        Ok(())
    }

    fn before_assets_backup(&self, _paths: &PublicationPaths) -> io::Result<()> {
        Ok(())
    }

    fn before_markdown_install(&self, _paths: &PublicationPaths) -> io::Result<()> {
        Ok(())
    }

    fn after_assets_directory_created(&self, _paths: &PublicationPaths) -> io::Result<()> {
        Ok(())
    }

    fn before_lock_identity(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    fn write_reacquired_lock_nonce(&self, file: &mut File, nonce: &[u8]) -> io::Result<()> {
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(nonce)?;
        file.sync_all()
    }

    fn before_lock_release(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }
}

struct RealFs;

impl FsOps for RealFs {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExistingOutputs {
    markdown_fingerprint: Option<String>,
    assets_fingerprint: Option<String>,
}

#[derive(Debug)]
enum CommitFailure {
    Restored(OutputError),
    Preserve(String),
}

impl CommitFailure {
    fn with_recovery(self, detail: String) -> Self {
        match self {
            Self::Restored(error) => Self::Preserve(format!("{error}; {detail}")),
            Self::Preserve(message) => Self::Preserve(format!("{message}; {detail}")),
        }
    }
}

fn finish_failure(failure: CommitFailure, staging: TempDir) -> Result<WriteResult, OutputError> {
    match failure {
        CommitFailure::Restored(error) => Err(error),
        CommitFailure::Preserve(message) => {
            let kept = staging.keep();
            debug_assert!(kept.is_absolute(), "staging parent must be canonical");
            Err(OutputError::TransactionFailed {
                message: format!("{message}; recovery directory: {}", kept.display()),
            })
        }
    }
}

struct PublicationPaths {
    markdown_path: PathBuf,
    assets_dir: PathBuf,
    parent: PathBuf,
    reported_parent: PathBuf,
    reported_markdown_path: PathBuf,
    reported_assets_dir: PathBuf,
    reported_lock_path: PathBuf,
    document_name: String,
    assets_name: String,
    lock_path: PathBuf,
}

impl PublicationPaths {
    fn new(markdown_path: &Path) -> Result<Self, OutputError> {
        let document_name = markdown_path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| OutputError::InvalidTarget(markdown_path.to_owned()))?
            .to_owned();
        let requested_parent = markdown_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !requested_parent.is_dir() {
            return Err(OutputError::InvalidTarget(markdown_path.to_owned()));
        }
        let parent = fs::canonicalize(requested_parent)
            .map_err(|_| OutputError::InvalidTarget(markdown_path.to_owned()))?;
        let assets_name = markdown_path
            .with_extension("assets")
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| OutputError::InvalidTarget(markdown_path.to_owned()))?
            .to_owned();
        let resolved_markdown_path = parent.join(&document_name);
        let assets_dir = parent.join(&assets_name);
        let lock_path = parent.join(format!(".{document_name}.mdviewer.lock"));
        let reported_parent = requested_parent.to_owned();
        let reported_markdown_path = markdown_path.to_owned();
        let reported_assets_dir = markdown_path.with_extension("assets");
        let reported_lock_path = reported_parent.join(format!(".{document_name}.mdviewer.lock"));

        Ok(Self {
            markdown_path: resolved_markdown_path,
            assets_dir,
            parent,
            reported_parent,
            reported_markdown_path,
            reported_assets_dir,
            reported_lock_path,
            document_name,
            assets_name,
            lock_path,
        })
    }

    fn report_path(&self, path: PathBuf) -> PathBuf {
        path.strip_prefix(&self.parent)
            .map_or(path.clone(), |relative| self.reported_parent.join(relative))
    }

    fn report_error(&self, error: OutputError) -> OutputError {
        match error {
            OutputError::InvalidTarget(path) => OutputError::InvalidTarget(self.report_path(path)),
            OutputError::OutputExists(path) => OutputError::OutputExists(self.report_path(path)),
            OutputError::UnownedAssetsDirectory(path) => {
                OutputError::UnownedAssetsDirectory(self.report_path(path))
            }
            OutputError::InvalidManifest { path, message } => OutputError::InvalidManifest {
                path: self.report_path(path),
                message,
            },
            OutputError::Io {
                operation,
                path,
                source,
            } => OutputError::Io {
                operation,
                path: self.report_path(path),
                source,
            },
            other => other,
        }
    }
}

static NEXT_LOCK_NONCE: AtomicU64 = AtomicU64::new(0);

fn next_lock_nonce() -> String {
    let sequence = NEXT_LOCK_NONCE.fetch_add(1, AtomicOrdering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}-{timestamp:x}-{sequence:x}", std::process::id())
}

#[cfg(unix)]
mod lock_identity {
    use std::{fs, io, os::unix::fs::MetadataExt, path::Path};

    use super::File;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct LockFileIdentity {
        device: u64,
        inode: u64,
    }

    impl LockFileIdentity {
        pub(super) fn from_file(file: &File) -> io::Result<Self> {
            let metadata = file.metadata()?;
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }

        pub(super) fn matches_path(self, path: &Path) -> io::Result<bool> {
            let metadata = fs::symlink_metadata(path)?;
            Ok(metadata.file_type().is_file()
                && self
                    == Self {
                        device: metadata.dev(),
                        inode: metadata.ino(),
                    })
        }
    }
}

#[cfg(windows)]
mod lock_identity {
    use std::{
        ffi::c_void,
        fs::{self, File, OpenOptions},
        io,
        mem::MaybeUninit,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        path::Path,
    };

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    #[repr(C)]
    struct FileTime {
        low_date_time: u32,
        high_date_time: u32,
    }

    #[repr(C)]
    struct ByHandleFileInformation {
        file_attributes: u32,
        creation_time: FileTime,
        last_access_time: FileTime,
        last_write_time: FileTime,
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(
            file: *mut c_void,
            information: *mut ByHandleFileInformation,
        ) -> i32;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct LockFileIdentity {
        volume: u32,
        index: u64,
    }

    impl LockFileIdentity {
        pub(super) fn from_file(file: &File) -> io::Result<Self> {
            let mut information = MaybeUninit::<ByHandleFileInformation>::uninit();
            // SAFETY: the file handle is valid for this call and Windows initializes the
            // complete output structure when the function reports success.
            let succeeded = unsafe {
                GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr())
            };
            if succeeded == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: the successful call above initialized the complete structure.
            let information = unsafe { information.assume_init() };
            Ok(Self {
                volume: information.volume_serial_number,
                index: u64::from(information.file_index_high) << 32
                    | u64::from(information.file_index_low),
            })
        }

        pub(super) fn matches_path(self, path: &Path) -> io::Result<bool> {
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.file_type().is_file() {
                return Ok(false);
            }
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            Ok(Self::from_file(&file)? == self)
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod lock_identity {
    use std::{fs::File, io, path::Path};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct LockFileIdentity;

    impl LockFileIdentity {
        pub(super) fn from_file(_file: &File) -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "stable output-lock file identity is unsupported on this platform",
            ))
        }

        pub(super) fn matches_path(self, _path: &Path) -> io::Result<bool> {
            Ok(false)
        }
    }
}

use lock_identity::LockFileIdentity;

struct TargetLock {
    path: PathBuf,
    parent: PathBuf,
    nonce: String,
    identity: LockFileIdentity,
    _file: File,
    state: LockState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LockState {
    Owned,
    Released,
    Unidentified,
}

impl TargetLock {
    fn acquire(paths: &PublicationPaths, fs_ops: &dyn FsOps) -> Result<Self, OutputError> {
        let nonce = next_lock_nonce();
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&paths.lock_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(OutputError::OutputExists(paths.reported_lock_path.clone()));
            }
            Err(source) => {
                return Err(io_error("acquire output lock", &paths.lock_path, source));
            }
        };
        let identity = match fs_ops
            .before_lock_identity(&paths.lock_path)
            .and_then(|()| LockFileIdentity::from_file(&file))
        {
            Ok(identity) => identity,
            Err(source) => {
                return Err(unidentified_lock_error(
                    &file,
                    &paths.lock_path,
                    &paths.parent,
                    source,
                ));
            }
        };
        let mut lock = Self {
            path: paths.lock_path.clone(),
            parent: paths.parent.clone(),
            nonce,
            identity,
            _file: file,
            state: LockState::Owned,
        };
        if let Err(source) = lock._file.write_all(lock.nonce.as_bytes()) {
            return Err(lock.acquisition_failure(
                fs_ops,
                io_error("write output lock nonce", &paths.lock_path, source),
            ));
        }
        if let Err(source) = lock._file.sync_all() {
            return Err(lock.acquisition_failure(
                fs_ops,
                io_error("fsync output lock", &paths.lock_path, source),
            ));
        }
        if let Err(error) = sync_directory(fs_ops, &paths.parent) {
            return Err(lock.acquisition_failure(fs_ops, error));
        }
        Ok(lock)
    }

    fn acquisition_failure(
        &mut self,
        fs_ops: &dyn FsOps,
        acquisition_error: OutputError,
    ) -> OutputError {
        match self.remove_owned_path(fs_ops) {
            Ok(()) => {
                let _ = sync_directory_raw(&self.parent);
                acquisition_error
            }
            Err(cleanup_error) => OutputError::TransactionFailed {
                message: format!(
                    "{acquisition_error}; output lock cleanup was refused: {cleanup_error}"
                ),
            },
        }
    }

    fn owns_path(&self) -> Result<bool, OutputError> {
        if self.state != LockState::Owned {
            return Ok(false);
        }
        if !self.identity_matches_path()? {
            return Ok(false);
        }
        let nonce = match fs::read(&self.path) {
            Ok(nonce) => nonce,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(source) => {
                return Err(io_error("read output lock nonce", &self.path, source));
            }
        };
        if nonce != self.nonce.as_bytes() {
            return Ok(false);
        }
        self.identity_matches_path()
    }

    fn identity_matches_path(&self) -> Result<bool, OutputError> {
        match self.identity.matches_path(&self.path) {
            Ok(matches) => Ok(matches),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(io_error("verify output lock identity", &self.path, source)),
        }
    }

    fn remove_owned_path(&mut self, fs_ops: &dyn FsOps) -> Result<(), OutputError> {
        if !self.owns_path()? {
            return Err(OutputError::TransactionFailed {
                message: format!(
                    "output lock ownership changed; refusing to remove {}",
                    self.path.display()
                ),
            });
        }
        fs_ops
            .remove_file(&self.path)
            .map_err(|source| io_error("remove output lock", &self.path, source))?;
        self.state = LockState::Released;
        Ok(())
    }

    fn release(&mut self, fs_ops: &dyn FsOps) -> Result<(), OutputError> {
        fs_ops
            .before_lock_release(&self.path)
            .map_err(|source| io_error("run pre-lock-release hook", &self.path, source))?;
        self.remove_owned_path(fs_ops)?;
        if let Err(error) = sync_directory(fs_ops, &self.parent) {
            if let Err(reacquire_error) = self.try_reacquire(fs_ops) {
                return Err(OutputError::TransactionFailed {
                    message: format!(
                        "{error}; output lock reacquisition failed: {reacquire_error}"
                    ),
                });
            }
            return Err(error);
        }
        Ok(())
    }

    fn try_reacquire(&mut self, fs_ops: &dyn FsOps) -> Result<(), OutputError> {
        let Ok(mut file) = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&self.path)
        else {
            return Ok(());
        };
        let identity = match fs_ops
            .before_lock_identity(&self.path)
            .and_then(|()| LockFileIdentity::from_file(&file))
        {
            Ok(identity) => identity,
            Err(source) => {
                let error = unidentified_lock_error(&file, &self.path, &self.parent, source);
                self._file = file;
                self.state = LockState::Unidentified;
                return Err(error);
            }
        };
        if let Err(source) = fs_ops.write_reacquired_lock_nonce(&mut file, self.nonce.as_bytes()) {
            let error = incomplete_lock_nonce_error(&file, &self.path, &self.parent, source);
            self._file = file;
            self.state = LockState::Unidentified;
            return Err(error);
        }
        self.identity = identity;
        self.state = LockState::Owned;
        self._file = file;
        Ok(())
    }

    fn is_held(&self) -> bool {
        self.owns_path().unwrap_or(false)
    }
}

impl Drop for TargetLock {
    fn drop(&mut self) {
        if self.owns_path().unwrap_or(false) {
            let _ = fs::remove_file(&self.path);
            let _ = sync_directory_raw(&self.parent);
        }
    }
}

fn unidentified_lock_error(
    file: &File,
    path: &Path,
    parent: &Path,
    source: io::Error,
) -> OutputError {
    retained_unverified_lock_error(
        file,
        path,
        parent,
        format!("could not establish identity for newly-created output lock: {source}"),
    )
}

fn incomplete_lock_nonce_error(
    file: &File,
    path: &Path,
    parent: &Path,
    source: io::Error,
) -> OutputError {
    retained_unverified_lock_error(
        file,
        path,
        parent,
        format!("could not persist the complete reacquired output-lock nonce: {source}"),
    )
}

fn retained_unverified_lock_error(
    file: &File,
    path: &Path,
    parent: &Path,
    reason: String,
) -> OutputError {
    let file_sync = file
        .sync_all()
        .err()
        .map(|error| format!("; lock fsync also failed: {error}"))
        .unwrap_or_default();
    let parent_sync = sync_directory_raw(parent)
        .err()
        .map(|error| format!("; parent fsync also failed: {error}"))
        .unwrap_or_default();
    OutputError::TransactionFailed {
        message: format!(
            "{reason} at {}; the unverified lock was retained and manual recovery/removal is required{file_sync}{parent_sync}",
            path.display(),
        ),
    }
}

fn validate_asset_file_names(document: &Document) -> Result<(), OutputError> {
    let mut names = HashSet::with_capacity(document.assets.len());
    for asset in &document.assets {
        if !is_safe_basename(&asset.file_name) {
            return Err(OutputError::InvalidAssetFileName(asset.file_name.clone()));
        }
        if !names.insert(canonical_file_name(&asset.file_name)) {
            return Err(OutputError::DuplicateAssetFileName(asset.file_name.clone()));
        }
    }
    Ok(())
}

fn is_safe_basename(name: &str) -> bool {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.contains(':')
        || name.ends_with(['.', ' '])
        || name.chars().any(|character| character.is_ascii_control())
        || canonical_file_name(name) == MANIFEST_FILE_NAME
        || is_windows_reserved_component(name)
    {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn canonical_file_name(name: &str) -> String {
    name.nfkc().flat_map(char::to_lowercase).collect()
}

fn inspect_existing_outputs(paths: &PublicationPaths) -> Result<ExistingOutputs, OutputError> {
    (|| {
        Ok(ExistingOutputs {
            markdown_fingerprint: inspect_markdown(&paths.markdown_path)?,
            assets_fingerprint: inspect_existing_assets(paths)?,
        })
    })()
    .map_err(|error| paths.report_error(error))
}

fn enforce_initial_policy(
    overwrite: OverwritePolicy,
    paths: &PublicationPaths,
    outputs: &ExistingOutputs,
) -> Result<(), OutputError> {
    if overwrite == OverwritePolicy::Deny {
        if outputs.markdown_fingerprint.is_some() {
            return Err(OutputError::OutputExists(
                paths.reported_markdown_path.clone(),
            ));
        }
        if outputs.assets_fingerprint.is_some() {
            return Err(OutputError::OutputExists(paths.reported_assets_dir.clone()));
        }
    }
    Ok(())
}

fn enforce_commit_policy(
    overwrite: OverwritePolicy,
    paths: &PublicationPaths,
    initial: &ExistingOutputs,
    current: &ExistingOutputs,
) -> Result<(), OutputError> {
    if overwrite == OverwritePolicy::Deny {
        return enforce_initial_policy(overwrite, paths, current);
    }

    if initial.markdown_fingerprint != current.markdown_fingerprint {
        if initial.markdown_fingerprint.is_none() && current.markdown_fingerprint.is_some() {
            return Err(OutputError::OutputExists(
                paths.reported_markdown_path.clone(),
            ));
        }
        return Err(OutputError::TransactionFailed {
            message: format!(
                "Markdown output changed before commit: {}",
                paths.markdown_path.display()
            ),
        });
    }
    if initial.assets_fingerprint != current.assets_fingerprint {
        if initial.assets_fingerprint.is_none() && current.assets_fingerprint.is_some() {
            return Err(OutputError::OutputExists(paths.reported_assets_dir.clone()));
        }
        return Err(OutputError::TransactionFailed {
            message: format!(
                "assets output changed before commit: {}",
                paths.assets_dir.display()
            ),
        });
    }
    Ok(())
}

fn inspect_markdown(path: &Path) -> Result<Option<String>, OutputError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(io_error("inspect Markdown output", path, source)),
    };
    if !metadata.is_file() {
        return Err(OutputError::InvalidTarget(path.to_owned()));
    }
    let bytes = fs::read(path).map_err(|source| io_error("read Markdown output", path, source))?;
    Ok(Some(sha256_hex(&bytes)))
}

fn inspect_existing_assets(paths: &PublicationPaths) -> Result<Option<String>, OutputError> {
    match fs::symlink_metadata(&paths.assets_dir) {
        Ok(_) => inspect_assets_directory(&paths.assets_dir, &paths.document_name).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(io_error(
            "inspect assets directory",
            &paths.assets_dir,
            source,
        )),
    }
}

fn inspect_assets_directory(directory: &Path, document_name: &str) -> Result<String, OutputError> {
    let directory_metadata = fs::symlink_metadata(directory)
        .map_err(|source| io_error("inspect assets directory", directory, source))?;
    if !directory_metadata.is_dir() {
        return Err(OutputError::UnownedAssetsDirectory(directory.to_owned()));
    }

    let manifest_path = directory.join(MANIFEST_FILE_NAME);
    match fs::symlink_metadata(&manifest_path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            return Err(OutputError::UnownedAssetsDirectory(directory.to_owned()));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(OutputError::UnownedAssetsDirectory(directory.to_owned()));
        }
        Err(source) => return Err(io_error("inspect assets manifest", &manifest_path, source)),
    }
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(OutputError::UnownedAssetsDirectory(directory.to_owned()));
        }
        Err(source) => return Err(io_error("read assets manifest", &manifest_path, source)),
    };
    let manifest: AssetManifest =
        serde_json::from_slice(&bytes).map_err(|error| OutputError::InvalidManifest {
            path: manifest_path.clone(),
            message: error.to_string(),
        })?;
    validate_manifest_structure(&manifest, document_name, &manifest_path)?;

    let mut expected_entries = HashSet::with_capacity(manifest.assets.len() + 1);
    expected_entries.insert(MANIFEST_FILE_NAME.to_owned());
    for asset in &manifest.assets {
        expected_entries.insert(asset.file_name.clone());
        let asset_path = directory.join(&asset.file_name);
        let metadata = match fs::symlink_metadata(&asset_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(OutputError::UnownedAssetsDirectory(directory.to_owned()));
            }
            Err(source) => return Err(io_error("inspect owned asset", &asset_path, source)),
        };
        if !metadata.is_file() {
            return Err(OutputError::UnownedAssetsDirectory(directory.to_owned()));
        }
        let data = fs::read(&asset_path)
            .map_err(|source| io_error("read owned asset", &asset_path, source))?;
        if sha256_hex(&data) != asset.sha256 {
            return Err(OutputError::UnownedAssetsDirectory(directory.to_owned()));
        }
    }

    for entry in fs::read_dir(directory)
        .map_err(|source| io_error("list assets directory", directory, source))?
    {
        let entry =
            entry.map_err(|source| io_error("read assets directory entry", directory, source))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            return Err(OutputError::UnownedAssetsDirectory(directory.to_owned()));
        };
        if !expected_entries.remove(&name) {
            return Err(OutputError::UnownedAssetsDirectory(directory.to_owned()));
        }
    }
    if !expected_entries.is_empty() {
        return Err(OutputError::UnownedAssetsDirectory(directory.to_owned()));
    }

    Ok(sha256_hex(&bytes))
}

fn validate_manifest_structure(
    manifest: &AssetManifest,
    document_name: &str,
    manifest_path: &Path,
) -> Result<(), OutputError> {
    if manifest.schema_version != SCHEMA_VERSION {
        return invalid_manifest(manifest_path, "schema version does not match");
    }
    if manifest.document != document_name {
        return invalid_manifest(manifest_path, "document name does not match");
    }
    let mut names = HashSet::with_capacity(manifest.assets.len());
    for asset in &manifest.assets {
        if !is_safe_basename(&asset.file_name) {
            return invalid_manifest(manifest_path, "asset file name is unsafe");
        }
        if !names.insert(canonical_file_name(&asset.file_name)) {
            return invalid_manifest(manifest_path, "asset file name is duplicated");
        }
    }
    Ok(())
}

fn invalid_manifest<T>(path: &Path, message: &str) -> Result<T, OutputError> {
    Err(OutputError::InvalidManifest {
        path: path.to_owned(),
        message: message.into(),
    })
}

fn create_directory(path: &Path, operation: &'static str) -> Result<(), OutputError> {
    fs::create_dir(path).map_err(|source| io_error(operation, path, source))
}

fn write_synced_file(
    path: &Path,
    bytes: &[u8],
    operation: &'static str,
) -> Result<(), OutputError> {
    let mut file = File::create(path).map_err(|source| io_error(operation, path, source))?;
    file.write_all(bytes)
        .map_err(|source| io_error(operation, path, source))?;
    file.sync_all()
        .map_err(|source| io_error("fsync staged file", path, source))
}

fn sync_directory(fs_ops: &dyn FsOps, path: &Path) -> Result<(), OutputError> {
    fs_ops
        .sync_directory(path)
        .map_err(|source| io_error("fsync directory", path, source))
}

fn sync_directory_raw(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};

        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .and_then(|directory| directory.sync_all())
    }

    #[cfg(not(windows))]
    File::open(path).and_then(|directory| directory.sync_all())
}

#[derive(Debug, Default)]
struct TransactionState {
    backed_markdown: bool,
    backed_assets: bool,
    installed_assets: bool,
    installed_markdown: bool,
}

fn commit(
    paths: &PublicationPaths,
    staging: &Path,
    staged_markdown: &Path,
    staged_assets: Option<&Path>,
    overwrite: OverwritePolicy,
    initial_outputs: &ExistingOutputs,
    fs_ops: &dyn FsOps,
) -> Result<TransactionState, CommitFailure> {
    let backup_markdown = staging.join("previous.md");
    let backup_assets = staging.join("previous.assets");
    let mut state = TransactionState::default();

    fs_ops.before_commit(paths).map_err(|source| {
        CommitFailure::Restored(io_error("run pre-commit hook", &paths.parent, source))
    })?;
    let current_outputs = inspect_existing_outputs(paths).map_err(CommitFailure::Restored)?;
    enforce_commit_policy(overwrite, paths, initial_outputs, &current_outputs)
        .map_err(CommitFailure::Restored)?;

    if current_outputs.markdown_fingerprint.is_some() {
        if let Err(source) = fs_ops.rename(&paths.markdown_path, &backup_markdown) {
            return Err(CommitFailure::Restored(io_error(
                "back up existing Markdown",
                &paths.markdown_path,
                source,
            )));
        }
        state.backed_markdown = true;
        if let Err(error) = sync_phase(fs_ops, staging, &paths.parent) {
            return Err(abort_transaction(
                state, paths, staging, error, fs_ops, None,
            ));
        }
    }

    if current_outputs.assets_fingerprint.is_some() {
        if let Err(source) = fs_ops.before_assets_backup(paths) {
            return Err(abort_transaction(
                state,
                paths,
                staging,
                io_error("run pre-assets-backup hook", &paths.assets_dir, source),
                fs_ops,
                None,
            ));
        }
        if let Err(source) = fs_ops.rename(&paths.assets_dir, &backup_assets) {
            return Err(abort_transaction(
                state,
                paths,
                staging,
                io_error(
                    "back up existing assets directory",
                    &paths.assets_dir,
                    source,
                ),
                fs_ops,
                None,
            ));
        }
        state.backed_assets = true;
        if let Err(error) = sync_phase(fs_ops, staging, &paths.parent) {
            return Err(abort_transaction(
                state, paths, staging, error, fs_ops, None,
            ));
        }

        let moved_fingerprint = inspect_assets_directory(&backup_assets, &paths.document_name);
        if moved_fingerprint.as_ref().ok() != current_outputs.assets_fingerprint.as_ref() {
            let detail = match moved_fingerprint {
                Ok(_) => "moved assets changed after ownership validation".to_owned(),
                Err(error) => format!("moved assets failed ownership validation: {error}"),
            };
            return Err(abort_transaction(
                state,
                paths,
                staging,
                OutputError::TransactionFailed {
                    message: detail.clone(),
                },
                fs_ops,
                Some(RecoveryRequest::preserve_assets(detail)),
            ));
        }
    }

    if let Some(staged_assets) = staged_assets {
        if let Err(source) = fs_ops.create_dir(&paths.assets_dir) {
            let cause = if source.kind() == io::ErrorKind::AlreadyExists {
                OutputError::OutputExists(paths.assets_dir.clone())
            } else {
                io_error("install assets directory", &paths.assets_dir, source)
            };
            return Err(abort_transaction(
                state, paths, staging, cause, fs_ops, None,
            ));
        }
        state.installed_assets = true;

        if let Err(source) = fs_ops.after_assets_directory_created(paths) {
            return Err(abort_transaction(
                state,
                paths,
                staging,
                io_error("run post-assets-create hook", &paths.assets_dir, source),
                fs_ops,
                None,
            ));
        }

        if let Err(error) = link_directory_contents(
            staged_assets,
            &paths.assets_dir,
            fs_ops,
            "install staged asset",
        ) {
            let recovery = match &error {
                OutputError::Io { source, .. } if source.kind() == io::ErrorKind::AlreadyExists => {
                    Some(RecoveryRequest::keep(
                        "concurrent asset entry was preserved",
                    ))
                }
                _ => None,
            };
            return Err(abort_transaction(
                state, paths, staging, error, fs_ops, recovery,
            ));
        }
        if let Err(error) = sync_directory(fs_ops, &paths.assets_dir)
            .and_then(|()| sync_directory(fs_ops, &paths.parent))
        {
            return Err(abort_transaction(
                state, paths, staging, error, fs_ops, None,
            ));
        }
        if let Err(error) = inspect_assets_directory(&paths.assets_dir, &paths.document_name) {
            return Err(abort_transaction(
                state,
                paths,
                staging,
                error,
                fs_ops,
                Some(RecoveryRequest::keep(
                    "installed assets changed before Markdown commit",
                )),
            ));
        }
    }

    if let Err(source) = fs_ops.before_markdown_install(paths) {
        return Err(abort_transaction(
            state,
            paths,
            staging,
            io_error(
                "run pre-Markdown-install hook",
                &paths.markdown_path,
                source,
            ),
            fs_ops,
            None,
        ));
    }
    if let Err(source) = fs_ops.hard_link(staged_markdown, &paths.markdown_path) {
        let cause = if source.kind() == io::ErrorKind::AlreadyExists {
            OutputError::OutputExists(paths.markdown_path.clone())
        } else {
            io_error("install Markdown", &paths.markdown_path, source)
        };

        if state.backed_markdown && paths.markdown_path.exists() {
            let concurrent_markdown = staging.join("concurrent.md");
            if let Err(move_error) = fs_ops.rename(&paths.markdown_path, &concurrent_markdown) {
                return Err(abort_transaction(
                    state,
                    paths,
                    staging,
                    cause,
                    fs_ops,
                    Some(RecoveryRequest::keep(format!(
                        "could not preserve concurrent Markdown: {move_error}"
                    ))),
                ));
            }
            if let Err(sync_error) = sync_phase(fs_ops, staging, &paths.parent) {
                return Err(abort_transaction(
                    state,
                    paths,
                    staging,
                    sync_error,
                    fs_ops,
                    Some(RecoveryRequest::keep("concurrent Markdown was preserved")),
                ));
            }
            return Err(abort_transaction(
                state,
                paths,
                staging,
                cause,
                fs_ops,
                Some(RecoveryRequest::keep("concurrent Markdown was preserved")),
            ));
        }
        return Err(abort_transaction(
            state, paths, staging, cause, fs_ops, None,
        ));
    }
    state.installed_markdown = true;

    if let Err(error) = sync_directory(fs_ops, &paths.parent) {
        return Err(abort_transaction(
            state, paths, staging, error, fs_ops, None,
        ));
    }

    Ok(state)
}

struct RecoveryRequest {
    reason: String,
    preserve_assets_backup: bool,
}

impl RecoveryRequest {
    fn keep(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            preserve_assets_backup: false,
        }
    }

    fn preserve_assets(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            preserve_assets_backup: true,
        }
    }
}

fn abort_transaction(
    state: TransactionState,
    paths: &PublicationPaths,
    staging: &Path,
    cause: OutputError,
    fs_ops: &dyn FsOps,
    recovery: Option<RecoveryRequest>,
) -> CommitFailure {
    let preserve_assets_backup = recovery
        .as_ref()
        .is_some_and(|request| request.preserve_assets_backup);
    let mut failures = rollback(state, paths, staging, fs_ops, preserve_assets_backup);
    if let Some(request) = recovery {
        failures.push(request.reason);
    }
    if failures.is_empty() {
        CommitFailure::Restored(cause)
    } else {
        CommitFailure::Preserve(format!(
            "{cause}; recovery incomplete: {}",
            failures.join("; ")
        ))
    }
}

fn rollback(
    state: TransactionState,
    paths: &PublicationPaths,
    staging: &Path,
    fs_ops: &dyn FsOps,
    preserve_assets_backup: bool,
) -> Vec<String> {
    let mut failures = Vec::new();
    let backup_markdown = staging.join("previous.md");
    let backup_assets = staging.join("previous.assets");

    if state.installed_markdown && paths.markdown_path.exists() {
        let failed_new_markdown = staging.join("failed-new.md");
        if let Err(error) = fs_ops.rename(&paths.markdown_path, &failed_new_markdown) {
            failures.push(format!("preserve new Markdown: {error}"));
        } else if let Err(error) = sync_phase(fs_ops, staging, &paths.parent) {
            failures.push(error.to_string());
        }
    }

    if state.installed_assets && paths.assets_dir.exists() {
        let failed_new_assets = staging.join("failed-new.assets");
        if let Err(error) = fs_ops.rename(&paths.assets_dir, &failed_new_assets) {
            failures.push(format!("preserve new assets: {error}"));
        } else if let Err(error) = sync_phase(fs_ops, staging, &paths.parent) {
            failures.push(error.to_string());
        }
    }

    if state.backed_assets && !preserve_assets_backup {
        if paths.assets_dir.exists() {
            failures.push("restore assets: destination reappeared".into());
        } else if let Err(error) =
            restore_directory(&backup_assets, &paths.assets_dir, staging, fs_ops)
        {
            failures.push(format!("restore assets: {error}"));
        }
    }

    if state.backed_markdown {
        if paths.markdown_path.exists() {
            failures.push("restore Markdown: destination reappeared".into());
        } else if let Err(source) = fs_ops.hard_link(&backup_markdown, &paths.markdown_path) {
            failures.push(format!("restore Markdown: {source}"));
        } else if let Err(error) = sync_directory(fs_ops, &paths.parent) {
            failures.push(error.to_string());
        }
    }

    failures
}

fn restore_directory(
    backup: &Path,
    destination: &Path,
    staging: &Path,
    fs_ops: &dyn FsOps,
) -> Result<(), OutputError> {
    fs_ops
        .create_dir(destination)
        .map_err(|source| io_error("restore assets directory", destination, source))?;
    if let Err(error) = link_directory_contents(backup, destination, fs_ops, "restore owned asset")
        .and_then(|()| sync_directory(fs_ops, destination))
        .and_then(|()| {
            destination
                .parent()
                .map_or(Ok(()), |parent| sync_directory(fs_ops, parent))
        })
    {
        let failed_restore = staging.join("failed-restore.assets");
        let _ = fs_ops.rename(destination, &failed_restore);
        let _ = sync_phase(
            fs_ops,
            staging,
            destination.parent().unwrap_or_else(|| Path::new(".")),
        );
        return Err(error);
    }
    Ok(())
}

fn link_directory_contents(
    source: &Path,
    destination: &Path,
    fs_ops: &dyn FsOps,
    operation: &'static str,
) -> Result<(), OutputError> {
    for entry in fs::read_dir(source)
        .map_err(|source_error| io_error("list staged directory", source, source_error))?
    {
        let entry = entry
            .map_err(|source_error| io_error("read staged directory", source, source_error))?;
        let metadata = entry
            .metadata()
            .map_err(|source_error| io_error("inspect staged file", &entry.path(), source_error))?;
        if !metadata.is_file() {
            return Err(OutputError::TransactionFailed {
                message: format!(
                    "staged entry is not a regular file: {}",
                    entry.path().display()
                ),
            });
        }
        let target = destination.join(entry.file_name());
        fs_ops
            .hard_link(&entry.path(), &target)
            .map_err(|source_error| io_error(operation, &target, source_error))?;
    }
    Ok(())
}

fn sync_phase(fs_ops: &dyn FsOps, staging: &Path, parent: &Path) -> Result<(), OutputError> {
    sync_directory(fs_ops, staging)?;
    sync_directory(fs_ops, parent)
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> OutputError {
    OutputError::Io {
        operation,
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Seek, SeekFrom},
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use super::*;
    use crate::{Asset, AssetId, Block, DocumentMetadata};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Fault {
        AssetsBackup,
        AssetsInstall,
        MarkdownInstall,
        MarkdownRestore,
        DirectorySync,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Hook {
        None,
        DenyLateMarkdown,
        SwapAssetsBeforeBackup,
        ReplaceLateAssetEntry,
        ReplaceLateMarkdown,
        ReplaceLockBeforeRelease,
        ReplaceLockDuringAcquireCleanup,
        FailLockIdentityQuery,
        FailReacquiredLockIdentity,
        FailReacquiredNonceWrite,
    }

    struct TestFs {
        faults: Mutex<Vec<Fault>>,
        hook: Hook,
        hook_ran: AtomicBool,
    }

    impl TestFs {
        fn new(faults: &[Fault], hook: Hook) -> Self {
            Self {
                faults: Mutex::new(faults.to_vec()),
                hook,
                hook_ran: AtomicBool::new(false),
            }
        }

        fn take_fault(&self, fault: Fault) -> bool {
            let mut faults = self.faults.lock().unwrap();
            let Some(index) = faults.iter().position(|candidate| *candidate == fault) else {
                return false;
            };
            faults.remove(index);
            true
        }
    }

    impl FsOps for TestFs {
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if to.file_name().is_some_and(|name| name == "previous.assets")
                && self.take_fault(Fault::AssetsBackup)
            {
                return Err(io::Error::other("injected assets backup failure"));
            }
            fs::rename(from, to)
        }

        fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
            if from.file_name().is_some_and(|name| name == "new.md")
                && to.extension().is_some_and(|extension| extension == "md")
                && self.take_fault(Fault::MarkdownInstall)
            {
                return Err(io::Error::other("injected Markdown install failure"));
            }
            if from.file_name().is_some_and(|name| name == "previous.md")
                && self.take_fault(Fault::MarkdownRestore)
            {
                return Err(io::Error::other("injected Markdown restore failure"));
            }
            fs::hard_link(from, to)
        }

        fn create_dir(&self, path: &Path) -> io::Result<()> {
            if path
                .extension()
                .is_some_and(|extension| extension == "assets")
                && self.take_fault(Fault::AssetsInstall)
            {
                return Err(io::Error::other("injected assets install failure"));
            }
            fs::create_dir(path)
        }

        fn sync_directory(&self, path: &Path) -> io::Result<()> {
            if self.hook == Hook::ReplaceLockDuringAcquireCleanup {
                let lock_path = path.join(".foo.md.mdviewer.lock");
                if lock_path.exists() && !self.hook_ran.swap(true, Ordering::SeqCst) {
                    fs::remove_file(&lock_path)?;
                    fs::write(&lock_path, b"foreign acquisition lock")?;
                    return Err(io::Error::other("injected lock acquisition sync failure"));
                }
            }
            if path.join("previous.md").exists() && self.take_fault(Fault::DirectorySync) {
                return Err(io::Error::other("injected directory sync failure"));
            }
            if matches!(
                self.hook,
                Hook::FailReacquiredLockIdentity | Hook::FailReacquiredNonceWrite
            ) && self.hook_ran.load(Ordering::SeqCst)
            {
                return Err(io::Error::other("injected lock release sync failure"));
            }
            sync_directory_raw(path)
        }

        fn before_commit(&self, paths: &PublicationPaths) -> io::Result<()> {
            if self.hook == Hook::DenyLateMarkdown && !self.hook_ran.swap(true, Ordering::SeqCst) {
                fs::write(&paths.markdown_path, b"late collision")?;
            }
            Ok(())
        }

        fn before_assets_backup(&self, paths: &PublicationPaths) -> io::Result<()> {
            if self.hook == Hook::SwapAssetsBeforeBackup
                && !self.hook_ran.swap(true, Ordering::SeqCst)
            {
                fs::remove_dir_all(&paths.assets_dir)?;
                fs::create_dir(&paths.assets_dir)?;
                fs::write(paths.assets_dir.join("personal.txt"), b"must survive")?;
            }
            Ok(())
        }

        fn before_markdown_install(&self, paths: &PublicationPaths) -> io::Result<()> {
            if self.hook == Hook::ReplaceLateMarkdown && !self.hook_ran.swap(true, Ordering::SeqCst)
            {
                fs::write(&paths.markdown_path, b"concurrent markdown")?;
            }
            Ok(())
        }

        fn after_assets_directory_created(&self, paths: &PublicationPaths) -> io::Result<()> {
            if self.hook == Hook::ReplaceLateAssetEntry
                && !self.hook_ran.swap(true, Ordering::SeqCst)
            {
                fs::write(paths.assets_dir.join("new.png"), b"concurrent asset")?;
            }
            Ok(())
        }

        fn before_lock_release(&self, path: &Path) -> io::Result<()> {
            if matches!(
                self.hook,
                Hook::FailReacquiredLockIdentity | Hook::FailReacquiredNonceWrite
            ) {
                self.hook_ran.store(true, Ordering::SeqCst);
            }
            if self.hook == Hook::ReplaceLockBeforeRelease
                && !self.hook_ran.swap(true, Ordering::SeqCst)
            {
                let nonce = fs::read(path)?;
                fs::remove_file(path)?;
                fs::write(path, nonce)?;
            }
            Ok(())
        }

        fn before_lock_identity(&self, _path: &Path) -> io::Result<()> {
            if self.hook == Hook::FailLockIdentityQuery
                || (self.hook == Hook::FailReacquiredLockIdentity
                    && self.hook_ran.load(Ordering::SeqCst))
            {
                return Err(io::Error::other("injected lock identity query failure"));
            }
            Ok(())
        }

        fn write_reacquired_lock_nonce(&self, file: &mut File, nonce: &[u8]) -> io::Result<()> {
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            if self.hook == Hook::FailReacquiredNonceWrite {
                file.write_all(b"partial")?;
                return Err(io::Error::other("injected reacquired nonce write failure"));
            }
            file.write_all(nonce)?;
            file.sync_all()
        }
    }

    fn document(file_name: &str, bytes: &[u8]) -> Document {
        let id = AssetId::new("image").unwrap();
        Document {
            metadata: DocumentMetadata::default(),
            blocks: vec![Block::Image {
                asset_id: id.clone(),
                alt: "image".into(),
            }],
            assets: vec![Asset {
                id,
                file_name: file_name.into(),
                media_type: "image/png".into(),
                data: bytes.to_vec(),
            }],
            warnings: vec![],
        }
    }

    fn target(directory: &Path, overwrite: OverwritePolicy) -> OutputTarget {
        OutputTarget {
            markdown_path: directory.join("foo.md"),
            overwrite,
        }
    }

    fn assert_old_outputs(directory: &Path, old_markdown: &[u8]) {
        assert_eq!(fs::read(directory.join("foo.md")).unwrap(), old_markdown);
        assert_eq!(
            fs::read(directory.join("foo.assets/old.png")).unwrap(),
            b"old"
        );
        assert!(!directory.join("foo.assets/new.png").exists());
        assert!(!directory.join(".foo.md.mdviewer.lock").exists());
    }

    fn publish_old(directory: &Path) -> Vec<u8> {
        publish(
            &document("old.png", b"old"),
            &target(directory, OverwritePolicy::Deny),
            &NeverCancel,
        )
        .unwrap();
        fs::read(directory.join("foo.md")).unwrap()
    }

    fn recovery_path(error: OutputError) -> PathBuf {
        let OutputError::TransactionFailed { message } = error else {
            panic!("expected recovery failure, got {error:?}");
        };
        let marker = "recovery directory: ";
        let path = message
            .split_once(marker)
            .unwrap_or_else(|| panic!("missing recovery path in {message:?}"))
            .1;
        PathBuf::from(path)
    }

    #[test]
    fn relative_target_parent_is_resolved_to_absolute_before_staging() {
        let paths = PublicationPaths::new(Path::new("relative-output.md")).unwrap();

        assert!(paths.parent.is_absolute());
        assert!(paths.markdown_path.is_absolute());
        assert!(paths.assets_dir.is_absolute());
        assert!(paths.lock_path.is_absolute());
    }

    #[test]
    fn same_nonce_foreign_lock_replacement_survives_release_and_blocks_next_writer() {
        let directory = tempfile::tempdir().unwrap();
        let output_target = target(directory.path(), OverwritePolicy::Deny);

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &output_target,
            &NeverCancel,
            &TestFs::new(&[], Hook::ReplaceLockBeforeRelease),
        )
        .unwrap_err();
        let recovery = recovery_path(error);
        let lock_path = directory.path().join(".foo.md.mdviewer.lock");
        let foreign_contents = fs::read(&lock_path).unwrap();

        assert!(!foreign_contents.is_empty());
        let second_error = publish(
            &document("next.png", b"next"),
            &target(directory.path(), OverwritePolicy::Replace),
            &NeverCancel,
        )
        .unwrap_err();
        assert!(
            matches!(second_error, OutputError::OutputExists(ref path) if path == &lock_path),
            "unexpected second publication error: {second_error:?}"
        );
        assert_eq!(fs::read(&lock_path).unwrap(), foreign_contents);
        fs::remove_dir_all(recovery).unwrap();
    }

    #[test]
    fn foreign_lock_replacement_survives_acquisition_error_cleanup_and_blocks_next_writer() {
        let directory = tempfile::tempdir().unwrap();
        let output_target = target(directory.path(), OverwritePolicy::Deny);

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &output_target,
            &NeverCancel,
            &TestFs::new(&[], Hook::ReplaceLockDuringAcquireCleanup),
        )
        .unwrap_err();
        let lock_path = directory.path().join(".foo.md.mdviewer.lock");

        assert!(matches!(error, OutputError::TransactionFailed { .. }));
        assert_eq!(fs::read(&lock_path).unwrap(), b"foreign acquisition lock");
        let second_error = publish(
            &document("next.png", b"next"),
            &target(directory.path(), OverwritePolicy::Deny),
            &NeverCancel,
        )
        .unwrap_err();
        assert!(
            matches!(second_error, OutputError::OutputExists(ref path) if path == &lock_path),
            "unexpected second publication error: {second_error:?}"
        );
        assert_eq!(fs::read(&lock_path).unwrap(), b"foreign acquisition lock");
    }

    #[test]
    fn identity_query_failure_retains_absolute_lock_for_manual_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let output_target = target(directory.path(), OverwritePolicy::Deny);

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &output_target,
            &NeverCancel,
            &TestFs::new(&[], Hook::FailLockIdentityQuery),
        )
        .unwrap_err();
        let absolute_lock_path = fs::canonicalize(directory.path())
            .unwrap()
            .join(".foo.md.mdviewer.lock");
        let OutputError::TransactionFailed { message } = error else {
            panic!("expected transaction failure, got {error:?}");
        };

        assert!(message.contains(&absolute_lock_path.display().to_string()));
        assert!(message.contains("manual recovery/removal is required"));
        assert!(absolute_lock_path.is_file());
        assert!(!directory.path().join("foo.md").exists());
        assert!(!directory.path().join("foo.assets").exists());

        let requested_lock_path = directory.path().join(".foo.md.mdviewer.lock");
        let second_error = publish(
            &document("next.png", b"next"),
            &target(directory.path(), OverwritePolicy::Deny),
            &NeverCancel,
        )
        .unwrap_err();
        assert!(
            matches!(second_error, OutputError::OutputExists(ref path) if path == &requested_lock_path),
            "unexpected second publication error: {second_error:?}"
        );
        fs::remove_file(absolute_lock_path).unwrap();
    }

    #[test]
    fn reacquired_identity_failure_retains_lock_outputs_and_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let output_target = target(directory.path(), OverwritePolicy::Deny);

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &output_target,
            &NeverCancel,
            &TestFs::new(&[], Hook::FailReacquiredLockIdentity),
        )
        .unwrap_err();
        let absolute_lock_path = fs::canonicalize(directory.path())
            .unwrap()
            .join(".foo.md.mdviewer.lock");
        let OutputError::TransactionFailed { message } = error else {
            panic!("expected transaction failure, got {error:?}");
        };

        assert!(message.contains(&absolute_lock_path.display().to_string()));
        assert!(message.contains("manual recovery/removal is required"));
        let recovery = PathBuf::from(
            message
                .split_once("recovery directory: ")
                .unwrap_or_else(|| panic!("missing recovery path in {message:?}"))
                .1,
        );
        assert!(absolute_lock_path.is_file());
        assert!(directory.path().join("foo.md").is_file());
        assert_eq!(
            fs::read(directory.path().join("foo.assets/new.png")).unwrap(),
            b"new"
        );
        assert!(recovery.is_absolute());
        assert!(recovery.join("new.md").is_file());
        assert!(recovery.join("new.assets/new.png").is_file());

        let requested_lock_path = directory.path().join(".foo.md.mdviewer.lock");
        let second_error = publish(
            &document("next.png", b"next"),
            &target(directory.path(), OverwritePolicy::Replace),
            &NeverCancel,
        )
        .unwrap_err();
        assert!(
            matches!(second_error, OutputError::OutputExists(ref path) if path == &requested_lock_path),
            "unexpected second publication error: {second_error:?}"
        );
        fs::remove_file(absolute_lock_path).unwrap();
        fs::remove_dir_all(recovery).unwrap();
    }

    #[test]
    fn reacquired_nonce_write_failure_retains_lock_outputs_and_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let output_target = target(directory.path(), OverwritePolicy::Deny);

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &output_target,
            &NeverCancel,
            &TestFs::new(&[], Hook::FailReacquiredNonceWrite),
        )
        .unwrap_err();
        let absolute_lock_path = fs::canonicalize(directory.path())
            .unwrap()
            .join(".foo.md.mdviewer.lock");
        let OutputError::TransactionFailed { message } = error else {
            panic!("expected transaction failure, got {error:?}");
        };

        assert!(message.contains(&absolute_lock_path.display().to_string()));
        assert!(message.contains("manual recovery/removal is required"));
        let recovery = PathBuf::from(
            message
                .split_once("recovery directory: ")
                .unwrap_or_else(|| panic!("missing recovery path in {message:?}"))
                .1,
        );
        assert_eq!(fs::read(&absolute_lock_path).unwrap(), b"partial");
        assert!(directory.path().join("foo.md").is_file());
        assert_eq!(
            fs::read(directory.path().join("foo.assets/new.png")).unwrap(),
            b"new"
        );
        assert!(recovery.is_absolute());
        assert!(recovery.join("new.md").is_file());
        assert!(recovery.join("new.assets/new.png").is_file());

        let requested_lock_path = directory.path().join(".foo.md.mdviewer.lock");
        let second_error = publish(
            &document("next.png", b"next"),
            &target(directory.path(), OverwritePolicy::Replace),
            &NeverCancel,
        )
        .unwrap_err();
        assert!(
            matches!(second_error, OutputError::OutputExists(ref path) if path == &requested_lock_path),
            "unexpected second publication error: {second_error:?}"
        );
        fs::remove_file(absolute_lock_path).unwrap();
        fs::remove_dir_all(recovery).unwrap();
    }

    #[test]
    fn assets_backup_failure_restores_prior_outputs() {
        let directory = tempfile::tempdir().unwrap();
        let old_markdown = publish_old(directory.path());

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &target(directory.path(), OverwritePolicy::Replace),
            &NeverCancel,
            &TestFs::new(&[Fault::AssetsBackup], Hook::None),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            OutputError::Io {
                operation: "back up existing assets directory",
                ..
            }
        ));
        assert_old_outputs(directory.path(), &old_markdown);
    }

    #[test]
    fn assets_install_failure_restores_prior_outputs() {
        let directory = tempfile::tempdir().unwrap();
        let old_markdown = publish_old(directory.path());

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &target(directory.path(), OverwritePolicy::Replace),
            &NeverCancel,
            &TestFs::new(&[Fault::AssetsInstall], Hook::None),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            OutputError::Io {
                operation: "install assets directory",
                ..
            }
        ));
        assert_old_outputs(directory.path(), &old_markdown);
    }

    #[test]
    fn final_markdown_failure_restores_prior_outputs() {
        let directory = tempfile::tempdir().unwrap();
        let old_markdown = publish_old(directory.path());

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &target(directory.path(), OverwritePolicy::Replace),
            &NeverCancel,
            &TestFs::new(&[Fault::MarkdownInstall], Hook::None),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            OutputError::Io {
                operation: "install Markdown",
                ..
            }
        ));
        assert_old_outputs(directory.path(), &old_markdown);
    }

    #[test]
    fn restore_failure_preserves_absolute_recovery_directory_and_backups() {
        let directory = tempfile::tempdir().unwrap();
        publish_old(directory.path());

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &target(directory.path(), OverwritePolicy::Replace),
            &NeverCancel,
            &TestFs::new(
                &[Fault::MarkdownInstall, Fault::MarkdownRestore],
                Hook::None,
            ),
        )
        .unwrap_err();
        let recovery = recovery_path(error);

        assert!(recovery.is_absolute());
        assert!(recovery.is_dir());
        assert!(recovery.join("previous.md").is_file());
        assert!(recovery.join("previous.assets").is_dir());
        assert!(recovery.join("new.md").is_file());
        assert!(recovery.join("new.assets").is_dir());
        fs::remove_dir_all(recovery).unwrap();
    }

    #[test]
    fn directory_sync_failure_rolls_back_exactly() {
        let directory = tempfile::tempdir().unwrap();
        let old_markdown = publish_old(directory.path());

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &target(directory.path(), OverwritePolicy::Replace),
            &NeverCancel,
            &TestFs::new(&[Fault::DirectorySync], Hook::None),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            OutputError::Io {
                operation: "fsync directory",
                ..
            }
        ));
        assert_old_outputs(directory.path(), &old_markdown);
    }

    #[test]
    fn deny_rechecks_at_commit_and_preserves_a_late_collision() {
        let directory = tempfile::tempdir().unwrap();

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &target(directory.path(), OverwritePolicy::Deny),
            &NeverCancel,
            &TestFs::new(&[], Hook::DenyLateMarkdown),
        )
        .unwrap_err();

        assert!(
            matches!(error, OutputError::OutputExists(path) if path == directory.path().join("foo.md"))
        );
        assert_eq!(
            fs::read(directory.path().join("foo.md")).unwrap(),
            b"late collision"
        );
        assert!(!directory.path().join("foo.assets").exists());
    }

    #[test]
    fn moved_unowned_assets_are_preserved_in_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let old_markdown = publish_old(directory.path());

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &target(directory.path(), OverwritePolicy::Replace),
            &NeverCancel,
            &TestFs::new(&[], Hook::SwapAssetsBeforeBackup),
        )
        .unwrap_err();
        let recovery = recovery_path(error);

        assert_eq!(
            fs::read(directory.path().join("foo.md")).unwrap(),
            old_markdown
        );
        assert_eq!(
            fs::read(recovery.join("previous.assets/personal.txt")).unwrap(),
            b"must survive"
        );
        fs::remove_dir_all(recovery).unwrap();
    }

    #[test]
    fn replace_no_clobber_preserves_concurrent_markdown_in_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let old_markdown = publish_old(directory.path());

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &target(directory.path(), OverwritePolicy::Replace),
            &NeverCancel,
            &TestFs::new(&[], Hook::ReplaceLateMarkdown),
        )
        .unwrap_err();
        let recovery = recovery_path(error);

        assert_eq!(
            fs::read(directory.path().join("foo.md")).unwrap(),
            old_markdown
        );
        assert_eq!(
            fs::read(recovery.join("concurrent.md")).unwrap(),
            b"concurrent markdown"
        );
        fs::remove_dir_all(recovery).unwrap();
    }

    #[test]
    fn replace_no_clobber_preserves_concurrent_asset_entry_in_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let old_markdown = publish_old(directory.path());

        let error = publish_with_fs(
            &document("new.png", b"new"),
            &target(directory.path(), OverwritePolicy::Replace),
            &NeverCancel,
            &TestFs::new(&[], Hook::ReplaceLateAssetEntry),
        )
        .unwrap_err();
        let recovery = recovery_path(error);

        assert_old_outputs(directory.path(), &old_markdown);
        assert_eq!(
            fs::read(recovery.join("failed-new.assets/new.png")).unwrap(),
            b"concurrent asset"
        );
        fs::remove_dir_all(recovery).unwrap();
    }
}
