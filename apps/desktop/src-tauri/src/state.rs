use std::{
    collections::{HashMap, VecDeque},
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use thiserror::Error;
use uuid::{Uuid, Version};

use crate::jobs::{PrintJobId, PrintJobStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionAccess {
    Read,
    Write,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthorizedSelection {
    pub path: PathBuf,
    pub access: SelectionAccess,
    identity: Option<FileIdentity>,
    read_handle: Option<Arc<File>>,
    write_authority: Option<WriteAuthority>,
}

#[derive(Debug, Clone)]
struct WriteAuthority {
    #[cfg(not(any(unix, windows)))]
    parent_path: PathBuf,
    parent: Arc<File>,
    parent_identity: DirectoryIdentity,
    file_name: OsString,
    expected_target: Option<FileIdentity>,
    #[cfg(test)]
    test_faults: Arc<Mutex<VecDeque<TestFault>>>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFault {
    ReplaceTargetBeforeExchange,
    FailParentSync,
    FailRollback,
    FailAfterAssetsPublished,
    FailAssetCopyAfterCreate,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileIdentity {
    len: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
    #[cfg(unix)]
    links: u64,
    #[cfg(windows)]
    volume: u64,
    #[cfg(windows)]
    file_id: [u8; 16],
    #[cfg(windows)]
    changed: i64,
    #[cfg(windows)]
    last_write: i64,
    #[cfg(windows)]
    links: u32,
}

#[derive(Debug)]
pub(crate) struct AuthorizedInput {
    file: tempfile::NamedTempFile,
}

#[derive(Debug)]
pub(crate) struct ConversionStaging {
    directory: PathBuf,
    markdown: PathBuf,
    assets: PathBuf,
}

#[derive(Debug)]
struct PreparedFile {
    recovery_name: OsString,
    candidate_name: OsString,
    candidate_identity: FileIdentity,
}

#[derive(Debug)]
struct PreparedDirectory {
    recovery_name: OsString,
    recovery_entries: Vec<OsString>,
    candidate_name: OsString,
    candidate_entries: Vec<OsString>,
    candidate_identity: DirectoryIdentity,
}

#[cfg(windows)]
#[derive(Debug)]
struct RollbackIoError;

#[cfg(windows)]
impl std::fmt::Display for RollbackIoError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("publication rollback failed")
    }
}

#[cfg(windows)]
impl std::error::Error for RollbackIoError {}

impl ConversionStaging {
    pub(crate) fn markdown_path(&self) -> &Path {
        &self.markdown
    }

    pub(crate) fn assets_path(&self) -> &Path {
        &self.assets
    }
}

impl Drop for ConversionStaging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

impl AuthorizedInput {
    pub(crate) fn path(&self) -> &Path {
        self.file.path()
    }
}

#[derive(Debug)]
pub(crate) struct RunningConversion {
    cancelled: Arc<AtomicBool>,
    pub marker: PathBuf,
}

#[derive(Debug, Error)]
pub enum StateError {
    #[error("application state is unavailable")]
    Poisoned,
    #[error("selection token is invalid")]
    InvalidToken,
    #[error("selection does not grant this operation")]
    AccessDenied,
    #[error("selected path is invalid")]
    InvalidSelection,
    #[error("selected source changed after authorization")]
    SourceChanged,
    #[error("selected destination changed after authorization")]
    ScopeChanged,
    #[error("publication rollback failed; private recovery artifacts were preserved")]
    RecoveryRequired,
    #[error("conversion ID is invalid")]
    InvalidOperationId,
    #[error("conversion is already running")]
    AlreadyRunning,
    #[error("conversion is not running")]
    NotRunning,
    #[error("application state operation failed")]
    Io(#[source] io::Error),
}

impl StateError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Poisoned => "state_unavailable",
            Self::InvalidToken => "invalid_token",
            Self::AccessDenied => "access_denied",
            Self::InvalidSelection => "invalid_selection",
            Self::SourceChanged => "source_changed",
            Self::ScopeChanged => "scope_changed",
            Self::RecoveryRequired => "recovery_required",
            Self::InvalidOperationId => "invalid_operation_id",
            Self::AlreadyRunning => "conversion_already_running",
            Self::NotRunning => "conversion_not_running",
            Self::Io(_) => "state_io",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    jobs: PrintJobStore,
    runtime: PathBuf,
    selections: Arc<RwLock<HashMap<String, AuthorizedSelection>>>,
    conversions: Arc<Mutex<HashMap<String, RunningConversion>>>,
    warnings: Arc<RwLock<HashMap<String, Vec<String>>>>,
    pending_print_jobs: Arc<Mutex<VecDeque<PrintJobId>>>,
}

impl AppState {
    pub fn new(jobs: PrintJobStore, runtime: impl AsRef<Path>) -> Result<Self, StateError> {
        let runtime = runtime.as_ref();
        if !is_local_absolute(runtime) {
            return Err(StateError::InvalidSelection);
        }
        create_private_directory(runtime)?;
        let runtime = fs::canonicalize(runtime).map_err(StateError::Io)?;
        Ok(Self {
            jobs,
            runtime,
            selections: Arc::new(RwLock::new(HashMap::new())),
            conversions: Arc::new(Mutex::new(HashMap::new())),
            warnings: Arc::new(RwLock::new(HashMap::new())),
            pending_print_jobs: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    #[must_use]
    pub fn jobs(&self) -> &PrintJobStore {
        &self.jobs
    }

    pub fn authorize_user_selection(
        &self,
        path: impl AsRef<Path>,
        access: SelectionAccess,
    ) -> Result<String, StateError> {
        let path = validate_selected_path(path.as_ref(), access)?;
        let identity = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(StateError::InvalidSelection);
                }
                let file = open_read_no_follow(&path).map_err(|_| StateError::InvalidSelection)?;
                Some(
                    FileIdentity::from_open_file(&file, &metadata)
                        .map_err(|_| StateError::InvalidSelection)?,
                )
            }
            Err(error)
                if error.kind() == io::ErrorKind::NotFound && access == SelectionAccess::Write =>
            {
                None
            }
            Err(_) => return Err(StateError::InvalidSelection),
        };
        let write_authority = if access == SelectionAccess::Write {
            let parent_path = path
                .parent()
                .ok_or(StateError::InvalidSelection)?
                .to_path_buf();
            let file_name = path
                .file_name()
                .ok_or(StateError::InvalidSelection)?
                .to_os_string();
            let parent =
                open_directory_no_follow(&parent_path).map_err(|_| StateError::InvalidSelection)?;
            let parent_metadata = parent
                .metadata()
                .map_err(|_| StateError::InvalidSelection)?;
            if !parent_metadata.is_dir() || is_reparse_or_symlink(&parent_metadata) {
                return Err(StateError::InvalidSelection);
            }
            let parent_identity = DirectoryIdentity::from_open_directory(&parent)
                .map_err(|_| StateError::InvalidSelection)?;
            Some(WriteAuthority {
                #[cfg(not(any(unix, windows)))]
                parent_path,
                parent: Arc::new(parent),
                parent_identity,
                file_name,
                expected_target: identity.clone(),
                #[cfg(test)]
                test_faults: Arc::new(Mutex::new(VecDeque::new())),
            })
        } else {
            None
        };
        let token = Uuid::new_v4().hyphenated().to_string();
        self.selections
            .write()
            .map_err(|_| StateError::Poisoned)?
            .insert(
                token.clone(),
                AuthorizedSelection {
                    path,
                    access,
                    identity,
                    read_handle: None,
                    write_authority,
                },
            );
        Ok(token)
    }

    pub(crate) fn selection(
        &self,
        token: &str,
        access: SelectionAccess,
    ) -> Result<AuthorizedSelection, StateError> {
        validate_uuid_v4(token).map_err(|_| StateError::InvalidToken)?;
        let selection = self
            .selections
            .read()
            .map_err(|_| StateError::Poisoned)?
            .get(token)
            .cloned()
            .ok_or(StateError::InvalidToken)?;
        if selection.access != access {
            return Err(StateError::AccessDenied);
        }
        selection.verify()?;
        Ok(selection)
    }

    pub(crate) fn authorize_published_output(
        &self,
        selection: &AuthorizedSelection,
    ) -> Result<String, StateError> {
        let authority = selection
            .write_authority
            .as_ref()
            .ok_or(StateError::InvalidSelection)?;
        authority.verify_parent()?;
        let file = open_relative_read(authority, &authority.file_name).map_err(StateError::Io)?;
        let metadata = file.metadata().map_err(StateError::Io)?;
        if !metadata.is_file() || is_reparse_or_symlink(&metadata) {
            return Err(StateError::InvalidSelection);
        }
        let identity = FileIdentity::from_open_file(&file, &metadata).map_err(StateError::Io)?;
        let token = Uuid::new_v4().hyphenated().to_string();
        self.selections
            .write()
            .map_err(|_| StateError::Poisoned)?
            .insert(
                token.clone(),
                AuthorizedSelection {
                    path: selection.path.clone(),
                    access: SelectionAccess::Read,
                    identity: Some(identity),
                    read_handle: Some(Arc::new(file)),
                    write_authority: None,
                },
            );
        Ok(token)
    }

    pub(crate) fn renew_write_selection(
        &self,
        selection: &AuthorizedSelection,
    ) -> Result<String, StateError> {
        let authority = selection
            .write_authority
            .as_ref()
            .ok_or(StateError::InvalidSelection)?;
        authority.verify_parent()?;
        let file = open_relative_read(authority, &authority.file_name).map_err(StateError::Io)?;
        let metadata = file.metadata().map_err(StateError::Io)?;
        if !metadata.is_file() || is_reparse_or_symlink(&metadata) {
            return Err(StateError::InvalidSelection);
        }
        let identity = FileIdentity::from_open_file(&file, &metadata).map_err(StateError::Io)?;
        let mut renewed_authority = authority.clone();
        renewed_authority.expected_target = Some(identity.clone());
        let token = Uuid::new_v4().hyphenated().to_string();
        self.selections
            .write()
            .map_err(|_| StateError::Poisoned)?
            .insert(
                token.clone(),
                AuthorizedSelection {
                    path: selection.path.clone(),
                    access: SelectionAccess::Write,
                    identity: Some(identity),
                    read_handle: None,
                    write_authority: Some(renewed_authority),
                },
            );
        Ok(token)
    }

    pub(crate) fn take_selection(
        &self,
        token: &str,
        access: SelectionAccess,
    ) -> Result<AuthorizedSelection, StateError> {
        validate_uuid_v4(token).map_err(|_| StateError::InvalidToken)?;
        let mut selections = self.selections.write().map_err(|_| StateError::Poisoned)?;
        let selection = selections
            .get(token)
            .cloned()
            .ok_or(StateError::InvalidToken)?;
        if selection.access != access {
            return Err(StateError::AccessDenied);
        }
        selection.verify()?;
        selections.remove(token);
        Ok(selection)
    }

    pub(crate) fn snapshot_source(&self, token: &str) -> Result<AuthorizedInput, StateError> {
        validate_uuid_v4(token).map_err(|_| StateError::InvalidToken)?;
        let mut selections = self.selections.write().map_err(|_| StateError::Poisoned)?;
        let selection = selections
            .get(token)
            .cloned()
            .ok_or(StateError::InvalidToken)?;
        if selection.access != SelectionAccess::Read {
            return Err(StateError::AccessDenied);
        }

        let mut source = selection
            .open_read()
            .map_err(|_| StateError::SourceChanged)?;
        let before = source.metadata().map_err(|_| StateError::SourceChanged)?;
        selection.verify_handle(&source)?;
        let limit = mdconvert_core::ConversionLimits::default().max_input_bytes;
        if before.len() > limit {
            return Err(StateError::InvalidSelection);
        }
        let suffix = selection
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| format!(".{extension}"))
            .unwrap_or_default();
        let mut staged = tempfile::Builder::new()
            .prefix("source-")
            .suffix(&suffix)
            .tempfile_in(&self.runtime)
            .map_err(StateError::Io)?;
        let copied = io::copy(&mut Read::by_ref(&mut source).take(limit + 1), &mut staged)
            .map_err(StateError::Io)?;
        if copied != before.len() {
            return Err(StateError::SourceChanged);
        }
        staged.as_file().sync_all().map_err(StateError::Io)?;
        selection.verify_handle(&source)?;
        selections.remove(token);
        Ok(AuthorizedInput { file: staged })
    }

    pub(crate) fn conversion_staging(
        &self,
        operation_id: &str,
        requested_output: &Path,
    ) -> Result<ConversionStaging, StateError> {
        validate_uuid_v4(operation_id)?;
        let nonce = Uuid::new_v4();
        let directory = self
            .runtime
            .join(format!("conversion-{operation_id}-{nonce}"));
        create_private_directory(&directory)?;
        let file_name = requested_output
            .file_name()
            .ok_or(StateError::InvalidSelection)?;
        let markdown = directory.join(file_name);
        Ok(ConversionStaging {
            assets: markdown.with_extension("assets"),
            directory,
            markdown,
        })
    }

    pub(crate) fn begin_conversion(&self, operation_id: &str) -> Result<PathBuf, StateError> {
        validate_uuid_v4(operation_id)?;
        let marker = self.runtime.join(format!("cancel-{operation_id}"));
        let mut conversions = self.conversions.lock().map_err(|_| StateError::Poisoned)?;
        if conversions.contains_key(operation_id) {
            return Err(StateError::AlreadyRunning);
        }
        self.warnings
            .write()
            .map_err(|_| StateError::Poisoned)?
            .remove(operation_id);
        match fs::remove_file(&marker) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(StateError::Io(error)),
        }
        conversions.insert(
            operation_id.to_owned(),
            RunningConversion {
                cancelled: Arc::new(AtomicBool::new(false)),
                marker: marker.clone(),
            },
        );
        Ok(marker)
    }

    pub(crate) fn cancel_conversion(&self, operation_id: &str) -> Result<(), StateError> {
        validate_uuid_v4(operation_id)?;
        let conversions = self.conversions.lock().map_err(|_| StateError::Poisoned)?;
        let conversion = conversions
            .get(operation_id)
            .ok_or(StateError::NotRunning)?;
        conversion.cancelled.store(true, Ordering::Release);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        match options.open(&conversion.marker) {
            Ok(file) => file.sync_all().map_err(StateError::Io),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(StateError::Io(error)),
        }
    }

    pub(crate) fn end_conversion(&self, operation_id: &str) -> Result<(), StateError> {
        let conversion = self
            .conversions
            .lock()
            .map_err(|_| StateError::Poisoned)?
            .remove(operation_id)
            .ok_or(StateError::NotRunning)?;
        match fs::remove_file(conversion.marker) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StateError::Io(error)),
        }
    }

    pub(crate) fn record_warnings(
        &self,
        operation_id: &str,
        warning_codes: Vec<String>,
    ) -> Result<(), StateError> {
        self.warnings
            .write()
            .map_err(|_| StateError::Poisoned)?
            .insert(operation_id.to_owned(), warning_codes);
        Ok(())
    }

    pub(crate) fn warning_codes(&self, operation_id: &str) -> Result<Vec<String>, StateError> {
        validate_uuid_v4(operation_id)?;
        Ok(self
            .warnings
            .read()
            .map_err(|_| StateError::Poisoned)?
            .get(operation_id)
            .cloned()
            .unwrap_or_default())
    }

    pub(crate) fn queue_print_job(&self, id: PrintJobId) -> Result<(), StateError> {
        let mut pending = self
            .pending_print_jobs
            .lock()
            .map_err(|_| StateError::Poisoned)?;
        if !pending.contains(&id) {
            pending.push_back(id);
        }
        Ok(())
    }

    pub(crate) fn pending_print_jobs(&self) -> Result<Vec<PrintJobId>, StateError> {
        Ok(self
            .pending_print_jobs
            .lock()
            .map_err(|_| StateError::Poisoned)?
            .iter()
            .copied()
            .collect())
    }

    pub(crate) fn dequeue_print_job(&self, id: PrintJobId) -> Result<(), StateError> {
        self.pending_print_jobs
            .lock()
            .map_err(|_| StateError::Poisoned)?
            .retain(|pending| *pending != id);
        Ok(())
    }
}

impl AuthorizedSelection {
    #[cfg(test)]
    fn inject_test_fault(&self, fault: TestFault) {
        self.write_authority
            .as_ref()
            .expect("test write authority")
            .test_faults
            .lock()
            .expect("test fault lock")
            .push_back(fault);
    }

    fn verify(&self) -> Result<(), StateError> {
        if let Some(authority) = &self.write_authority {
            return authority.verify_current_target();
        }
        if let Some(file) = &self.read_handle {
            return self.verify_handle(file);
        }
        match (&self.identity, fs::symlink_metadata(&self.path)) {
            (Some(expected), Ok(metadata))
                if !metadata.file_type().is_symlink() && metadata.is_file() =>
            {
                let file = open_read_no_follow(&self.path).map_err(|_| self.changed_error())?;
                let actual = FileIdentity::from_open_file(&file, &metadata)
                    .map_err(|_| self.changed_error())?;
                if *expected == actual {
                    Ok(())
                } else {
                    Err(self.changed_error())
                }
            }
            (None, Err(error)) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            _ => Err(self.changed_error()),
        }
    }

    pub(crate) fn open_read(&self) -> Result<File, StateError> {
        if self.access != SelectionAccess::Read {
            return Err(StateError::AccessDenied);
        }
        if let Some(file) = &self.read_handle {
            return file.try_clone().map_err(StateError::Io);
        }
        open_read_no_follow(&self.path).map_err(StateError::Io)
    }

    pub(crate) fn verify_handle(&self, file: &File) -> Result<(), StateError> {
        let metadata = file.metadata().map_err(|_| StateError::SourceChanged)?;
        match &self.identity {
            Some(expected)
                if metadata.is_file()
                    && *expected
                        == FileIdentity::from_open_file(file, &metadata)
                            .map_err(|_| StateError::SourceChanged)? =>
            {
                Ok(())
            }
            _ => Err(StateError::SourceChanged),
        }
    }

    fn changed_error(&self) -> StateError {
        if self.access == SelectionAccess::Read {
            StateError::SourceChanged
        } else {
            StateError::ScopeChanged
        }
    }

    pub(crate) fn persist_content(&self, content: &[u8]) -> Result<(), StateError> {
        if self.access != SelectionAccess::Write {
            return Err(StateError::AccessDenied);
        }
        self.write_authority
            .as_ref()
            .ok_or(StateError::InvalidSelection)?
            .persist_content(content)
    }

    pub(crate) fn publish_conversion(
        &self,
        staging: &ConversionStaging,
    ) -> Result<Option<PathBuf>, StateError> {
        if self.access != SelectionAccess::Write {
            return Err(StateError::AccessDenied);
        }
        let authority = self
            .write_authority
            .as_ref()
            .ok_or(StateError::InvalidSelection)?;
        authority.publish_new_conversion(staging, &self.path)
    }
}

impl WriteAuthority {
    #[cfg(test)]
    fn take_test_fault(&self, fault: TestFault) -> bool {
        let mut faults = self.test_faults.lock().expect("test fault lock");
        let Some(position) = faults.iter().position(|candidate| *candidate == fault) else {
            return false;
        };
        faults.remove(position);
        true
    }

    #[cfg(not(test))]
    fn fail_test_point(&self, _point: &str) -> io::Result<()> {
        Ok(())
    }

    #[cfg(test)]
    fn fail_test_point(&self, point: &str) -> io::Result<()> {
        let fault = match point {
            "parent_sync" => TestFault::FailParentSync,
            "rollback" => TestFault::FailRollback,
            "after_assets" => TestFault::FailAfterAssetsPublished,
            "asset_copy" => TestFault::FailAssetCopyAfterCreate,
            _ => return Ok(()),
        };
        if self.take_test_fault(fault) {
            Err(io::Error::other(format!("injected {point} failure")))
        } else {
            Ok(())
        }
    }

    fn verify_parent(&self) -> Result<(), StateError> {
        let actual = DirectoryIdentity::from_open_directory(&self.parent)
            .map_err(|_| StateError::ScopeChanged)?;
        if actual == self.parent_identity {
            Ok(())
        } else {
            Err(StateError::ScopeChanged)
        }
    }

    fn verify_current_target(&self) -> Result<(), StateError> {
        self.verify_parent()?;
        match (
            &self.expected_target,
            open_relative_read(self, &self.file_name),
        ) {
            (Some(expected), Ok(file)) => {
                let metadata = file.metadata().map_err(|_| StateError::ScopeChanged)?;
                let actual = FileIdentity::from_open_file(&file, &metadata)
                    .map_err(|_| StateError::ScopeChanged)?;
                if metadata.is_file() && *expected == actual {
                    Ok(())
                } else {
                    Err(StateError::ScopeChanged)
                }
            }
            (None, Err(error)) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            _ => Err(StateError::ScopeChanged),
        }
    }

    fn persist_content(&self, content: &[u8]) -> Result<(), StateError> {
        self.verify_parent()?;
        let prepared = self.prepare_file(|staged| staged.write_all(content))?;
        match self.expected_target.as_ref() {
            None => self.publish_new_file_transaction(prepared),
            Some(expected) => self.replace_file_transaction(prepared, expected),
        }
    }

    fn prepare_file(
        &self,
        write_recovery: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> Result<PreparedFile, StateError> {
        let (mut recovery, recovery_name) = create_relative_private_file(self)?;
        let recovery_result = write_recovery(&mut recovery).and_then(|_| recovery.sync_all());
        drop(recovery);
        if let Err(error) = recovery_result {
            let _ = remove_relative(self, &recovery_name);
            return Err(StateError::Io(error));
        }

        let mut source = open_relative_read(self, &recovery_name).map_err(StateError::Io)?;
        let (mut candidate, candidate_name) = create_relative_private_file(self)?;
        let candidate_result = io::copy(&mut source, &mut candidate)
            .and_then(|_| candidate.sync_all())
            .map_err(StateError::Io);
        drop(candidate);
        if let Err(error) = candidate_result {
            let _ = remove_relative(self, &candidate_name);
            let _ = remove_relative(self, &recovery_name);
            return Err(error);
        }
        let candidate_identity = match relative_file_identity(self, &candidate_name) {
            Ok(identity) => identity,
            Err(error) => {
                let _ = remove_relative(self, &candidate_name);
                let _ = remove_relative(self, &recovery_name);
                return Err(error);
            }
        };
        Ok(PreparedFile {
            recovery_name,
            candidate_name,
            candidate_identity,
        })
    }

    fn prepare_file_from_path(&self, source: &Path) -> Result<PreparedFile, StateError> {
        let mut source = open_read_no_follow(source).map_err(StateError::Io)?;
        self.prepare_file(|destination| io::copy(&mut source, destination).map(|_| ()))
    }

    fn publish_new_file_transaction(&self, prepared: PreparedFile) -> Result<(), StateError> {
        if let Err(error) = publish_new_relative(self, &prepared.candidate_name) {
            self.cleanup_prepared_file(&prepared);
            return Err(if error.kind() == io::ErrorKind::AlreadyExists {
                StateError::ScopeChanged
            } else {
                StateError::Io(error)
            });
        }

        if let Err(error) = self.sync_parent_for_commit() {
            if self
                .rollback_new_file(&prepared.candidate_name, &prepared.candidate_identity)
                .is_err()
            {
                return Err(StateError::RecoveryRequired);
            }
            self.cleanup_prepared_file(&prepared);
            return Err(StateError::Io(error));
        }

        self.cleanup_prepared_file(&prepared);
        Ok(())
    }

    fn replace_file_transaction(
        &self,
        prepared: PreparedFile,
        expected: &FileIdentity,
    ) -> Result<(), StateError> {
        self.replace_target_before_exchange_for_test()?;
        let captured = match replace_and_capture_relative(self, &prepared.candidate_name) {
            Ok(captured) => captured,
            Err(error) => {
                if is_rollback_io_error(&error) {
                    return Err(StateError::RecoveryRequired);
                }
                self.cleanup_prepared_file(&prepared);
                return Err(if error.kind() == io::ErrorKind::NotFound {
                    StateError::ScopeChanged
                } else {
                    StateError::Io(error)
                });
            }
        };
        let captured_identity = match relative_file_identity(self, &captured) {
            Ok(identity) => identity,
            Err(_) => return Err(StateError::RecoveryRequired),
        };
        if !expected.matches_after_atomic_rename(&captured_identity) {
            if self
                .rollback_replacement(&captured, &captured_identity, &prepared.candidate_identity)
                .is_err()
            {
                return Err(StateError::RecoveryRequired);
            }
            self.cleanup_prepared_file(&prepared);
            return Err(StateError::ScopeChanged);
        }

        if let Err(error) = self.sync_parent_for_commit() {
            if self
                .rollback_replacement(&captured, &captured_identity, &prepared.candidate_identity)
                .is_err()
            {
                return Err(StateError::RecoveryRequired);
            }
            self.cleanup_prepared_file(&prepared);
            return Err(StateError::Io(error));
        }

        self.cleanup_prepared_file(&prepared);
        Ok(())
    }

    fn sync_parent_for_commit(&self) -> io::Result<()> {
        self.fail_test_point("parent_sync")?;
        self.parent.sync_all()
    }

    fn rollback_new_file(
        &self,
        candidate_name: &std::ffi::OsStr,
        expected_published: &FileIdentity,
    ) -> Result<(), StateError> {
        self.fail_test_point("rollback")
            .map_err(|_| StateError::RecoveryRequired)?;
        retract_new_relative(self, candidate_name).map_err(|_| StateError::RecoveryRequired)?;
        let retracted = relative_file_identity(self, candidate_name)
            .map_err(|_| StateError::RecoveryRequired)?;
        if !expected_published.matches_after_atomic_rename(&retracted) {
            return Err(StateError::RecoveryRequired);
        }
        self.parent
            .sync_all()
            .map_err(|_| StateError::RecoveryRequired)
    }

    fn rollback_replacement(
        &self,
        captured_name: &std::ffi::OsStr,
        expected_restored: &FileIdentity,
        expected_retracted: &FileIdentity,
    ) -> Result<(), StateError> {
        self.fail_test_point("rollback")
            .map_err(|_| StateError::RecoveryRequired)?;
        rollback_replace_relative(self, captured_name).map_err(|_| StateError::RecoveryRequired)?;
        let restored = relative_file_identity(self, &self.file_name)
            .map_err(|_| StateError::RecoveryRequired)?;
        let retracted = relative_file_identity(self, captured_name)
            .map_err(|_| StateError::RecoveryRequired)?;
        if !expected_restored.matches_after_atomic_rename(&restored)
            || !expected_retracted.matches_after_atomic_rename(&retracted)
        {
            return Err(StateError::RecoveryRequired);
        }
        self.parent
            .sync_all()
            .map_err(|_| StateError::RecoveryRequired)
    }

    fn cleanup_prepared_file(&self, prepared: &PreparedFile) {
        let _ = remove_relative_if_exists(self, &prepared.candidate_name);
        let _ = remove_relative_if_exists(self, &prepared.recovery_name);
        let _ = self.parent.sync_all();
    }

    fn publish_new_conversion(
        &self,
        staging: &ConversionStaging,
        requested_path: &Path,
    ) -> Result<Option<PathBuf>, StateError> {
        self.verify_parent()?;
        if self.expected_target.is_some() {
            return Err(StateError::ScopeChanged);
        }
        let markdown = self.prepare_file_from_path(staging.markdown_path())?;
        let assets_target_name = requested_path
            .with_extension("assets")
            .file_name()
            .ok_or(StateError::InvalidSelection)?
            .to_os_string();
        let assets_authority = Self {
            #[cfg(not(any(unix, windows)))]
            parent_path: self.parent_path.clone(),
            parent: Arc::clone(&self.parent),
            parent_identity: self.parent_identity.clone(),
            file_name: assets_target_name,
            expected_target: None,
            #[cfg(test)]
            test_faults: Arc::clone(&self.test_faults),
        };
        let prepared_assets = if staging.assets_path().exists() {
            let (recovery_name, recovery_entries, _) =
                match stage_asset_directory(&assets_authority, staging.assets_path()) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.cleanup_prepared_file(&markdown);
                        return Err(error);
                    }
                };
            let (candidate_name, candidate_entries, candidate_identity) =
                match stage_asset_directory(&assets_authority, staging.assets_path()) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        let recovery_authority = WriteAuthority {
                            file_name: recovery_name,
                            ..assets_authority.clone()
                        };
                        let _ =
                            remove_known_relative_directory(&recovery_authority, &recovery_entries);
                        self.cleanup_prepared_file(&markdown);
                        return Err(error);
                    }
                };
            Some(PreparedDirectory {
                recovery_name,
                recovery_entries,
                candidate_name,
                candidate_entries,
                candidate_identity,
            })
        } else {
            None
        };

        let mut assets_published = false;
        let mut markdown_published = false;
        let result = (|| {
            if let Some(assets) = &prepared_assets {
                publish_new_relative(&assets_authority, &assets.candidate_name)
                    .map_err(|_| StateError::ScopeChanged)?;
                assets_published = true;
                self.fail_test_point("after_assets")
                    .map_err(StateError::Io)?;
            }
            publish_new_relative(self, &markdown.candidate_name)
                .map_err(|_| StateError::ScopeChanged)?;
            markdown_published = true;
            self.sync_parent_for_commit().map_err(StateError::Io)
        })();
        if let Err(error) = result {
            let mut rollback_failed = false;
            if markdown_published
                && self
                    .rollback_new_file(&markdown.candidate_name, &markdown.candidate_identity)
                    .is_err()
            {
                rollback_failed = true;
            }
            if assets_published
                && let Some(assets) = &prepared_assets
                && assets_authority
                    .rollback_new_directory(&assets.candidate_name, &assets.candidate_identity)
                    .is_err()
            {
                rollback_failed = true;
            }
            if rollback_failed {
                return Err(StateError::RecoveryRequired);
            }
            self.cleanup_prepared_file(&markdown);
            if let Some(assets) = &prepared_assets {
                assets_authority.cleanup_prepared_directory(assets);
            }
            return Err(error);
        }
        self.cleanup_prepared_file(&markdown);
        if let Some(assets) = &prepared_assets {
            assets_authority.cleanup_prepared_directory(assets);
        }
        Ok(prepared_assets.map(|_| requested_path.with_extension("assets")))
    }

    fn rollback_new_directory(
        &self,
        candidate_name: &std::ffi::OsStr,
        expected_published: &DirectoryIdentity,
    ) -> Result<(), StateError> {
        self.fail_test_point("rollback")
            .map_err(|_| StateError::RecoveryRequired)?;
        retract_new_relative(self, candidate_name).map_err(|_| StateError::RecoveryRequired)?;
        let directory = open_relative_directory(self, candidate_name)
            .map_err(|_| StateError::RecoveryRequired)?;
        let actual = DirectoryIdentity::from_open_directory(&directory)
            .map_err(|_| StateError::RecoveryRequired)?;
        if &actual != expected_published {
            return Err(StateError::RecoveryRequired);
        }
        self.parent
            .sync_all()
            .map_err(|_| StateError::RecoveryRequired)
    }

    fn cleanup_prepared_directory(&self, prepared: &PreparedDirectory) {
        for (name, entries) in [
            (&prepared.candidate_name, &prepared.candidate_entries),
            (&prepared.recovery_name, &prepared.recovery_entries),
        ] {
            let authority = WriteAuthority {
                file_name: name.clone(),
                ..self.clone()
            };
            let _ = remove_known_relative_directory(&authority, entries);
        }
        let _ = self.parent.sync_all();
    }

    #[cfg(not(test))]
    fn replace_target_before_exchange_for_test(&self) -> Result<(), StateError> {
        Ok(())
    }

    #[cfg(test)]
    fn replace_target_before_exchange_for_test(&self) -> Result<(), StateError> {
        if !self.take_test_fault(TestFault::ReplaceTargetBeforeExchange) {
            return Ok(());
        }
        let (mut replacement, replacement_name) = create_relative_private_file(self)?;
        replacement
            .write_all(b"concurrent replacement")
            .and_then(|_| replacement.sync_all())
            .map_err(StateError::Io)?;
        drop(replacement);
        replace_and_capture_relative(self, &replacement_name).map_err(StateError::Io)?;
        remove_relative(self, &replacement_name).map_err(StateError::Io)
    }
}

impl DirectoryIdentity {
    fn from_open_directory(directory: &File) -> io::Result<Self> {
        directory_identity(directory)
    }
}

impl FileIdentity {
    fn from_open_file(file: &File, metadata: &fs::Metadata) -> io::Result<Self> {
        platform_file_identity(file, metadata)
    }

    fn matches_after_atomic_rename(&self, actual: &Self) -> bool {
        self.len == actual.len
            && self.modified == actual.modified
            && same_platform_identity_after_rename(self, actual)
    }
}

#[cfg(unix)]
fn same_platform_identity_after_rename(expected: &FileIdentity, actual: &FileIdentity) -> bool {
    expected.device == actual.device
        && expected.inode == actual.inode
        && expected.links == actual.links
}

#[cfg(windows)]
fn same_platform_identity_after_rename(expected: &FileIdentity, actual: &FileIdentity) -> bool {
    expected.volume == actual.volume
        && expected.file_id == actual.file_id
        && expected.last_write == actual.last_write
        && expected.links == actual.links
}

#[cfg(not(any(unix, windows)))]
fn same_platform_identity_after_rename(_expected: &FileIdentity, _actual: &FileIdentity) -> bool {
    true
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
    let state = windows_file_state(directory)?;
    Ok(DirectoryIdentity {
        volume: state.volume_serial_number,
        file_id: state.file_id,
    })
}

#[cfg(not(any(unix, windows)))]
fn directory_identity(_directory: &File) -> io::Result<DirectoryIdentity> {
    Ok(DirectoryIdentity {})
}

fn open_directory_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
            FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
        };
        options
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | SYNCHRONIZE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    options.open(path)
}

#[cfg(unix)]
fn relative_c_string(name: &std::ffi::OsStr) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid relative name"))
}

#[cfg(windows)]
pub(crate) fn windows_nt_open_relative(
    root: &File,
    name: &std::ffi::OsStr,
    desired_access: u32,
    create_disposition: u32,
    directory: Option<bool>,
) -> io::Result<File> {
    use std::{mem::size_of, os::windows::ffi::OsStrExt, os::windows::io::AsRawHandle};
    use windows_sys::Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT,
            FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    };
    use windows_sys::Win32::{
        Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING},
        Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE},
        System::IO::IO_STATUS_BLOCK,
    };

    let mut wide = name.encode_wide().collect::<Vec<_>>();
    if wide.is_empty()
        || wide
            .iter()
            .any(|character| matches!(*character, 0 | 47 | 92))
        || wide.len().saturating_mul(size_of::<u16>()) > u16::MAX as usize
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid relative Windows name",
        ));
    }
    let name_bytes = (wide.len() * size_of::<u16>()) as u16;
    let unicode = UNICODE_STRING {
        Length: name_bytes,
        MaximumLength: name_bytes,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: root.as_raw_handle().cast(),
        ObjectName: &raw const unicode,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut io_status = IO_STATUS_BLOCK::default();
    let mut handle: HANDLE = std::ptr::null_mut();
    let type_options = match directory {
        Some(true) => FILE_DIRECTORY_FILE,
        Some(false) => FILE_NON_DIRECTORY_FILE,
        None => 0,
    };
    let status = unsafe {
        NtCreateFile(
            &raw mut handle,
            desired_access,
            &raw const attributes,
            &raw mut io_status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            create_disposition,
            FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT | type_options,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    if handle.is_null() {
        return Err(io::Error::other("NtCreateFile returned a null handle"));
    }
    use std::os::windows::io::{FromRawHandle, RawHandle};
    Ok(unsafe { File::from_raw_handle(handle as RawHandle) })
}

#[cfg(windows)]
pub(crate) fn windows_open_relative_for_mutation(
    root: &File,
    name: &std::ffi::OsStr,
) -> io::Result<File> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_OPEN,
        Win32::Storage::FileSystem::{DELETE, FILE_READ_ATTRIBUTES, SYNCHRONIZE},
    };
    let file = windows_nt_open_relative(
        root,
        name,
        DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        None,
    )?;
    let metadata = file.metadata()?;
    if is_reparse_or_symlink(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reparse point rejected",
        ));
    }
    Ok(file)
}

#[cfg(windows)]
pub(crate) fn windows_rename_open_handle(
    source: &File,
    root: &File,
    destination: &std::ffi::OsStr,
) -> io::Result<()> {
    use std::{mem::size_of, os::windows::ffi::OsStrExt, os::windows::io::AsRawHandle, ptr};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_RENAME_INFO, FileRenameInfo, SetFileInformationByHandle,
    };

    let wide = destination.encode_wide().collect::<Vec<_>>();
    if wide.is_empty()
        || wide
            .iter()
            .any(|character| matches!(*character, 0 | 47 | 92))
        || wide.len().saturating_mul(size_of::<u16>()) > u32::MAX as usize
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid relative Windows rename target",
        ));
    }
    let header = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let bytes = header + wide.len() * size_of::<u16>();
    let mut storage = vec![0_usize; bytes.div_ceil(size_of::<usize>())];
    let information = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*information).Anonymous.ReplaceIfExists = false;
        (*information).RootDirectory = root.as_raw_handle().cast();
        (*information).FileNameLength = (wide.len() * size_of::<u16>()) as u32;
        ptr::copy_nonoverlapping(
            wide.as_ptr(),
            (&raw mut (*information).FileName).cast(),
            wide.len(),
        );
    }
    if unsafe {
        SetFileInformationByHandle(
            source.as_raw_handle().cast(),
            FileRenameInfo,
            information.cast(),
            bytes as u32,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
pub(crate) fn windows_delete_open_handle(file: &File) -> io::Result<()> {
    use std::{mem::size_of, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
    };
    let information = FILE_DISPOSITION_INFO { DeleteFile: true };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            FileDispositionInfo,
            (&raw const information).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn windows_rename_relative(
    authority: &WriteAuthority,
    source: &std::ffi::OsStr,
    destination: &std::ffi::OsStr,
) -> io::Result<()> {
    let source = windows_open_relative_for_mutation(&authority.parent, source)?;
    windows_rename_open_handle(&source, &authority.parent, destination)
}

#[cfg(unix)]
fn open_relative_read(authority: &WriteAuthority, name: &std::ffi::OsStr) -> io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = relative_c_string(name)?;
    let descriptor = unsafe {
        libc::openat(
            authority.parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

#[cfg(unix)]
fn open_relative_directory(authority: &WriteAuthority, name: &std::ffi::OsStr) -> io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = relative_c_string(name)?;
    let descriptor = unsafe {
        libc::openat(
            authority.parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

#[cfg(windows)]
fn open_relative_read(authority: &WriteAuthority, name: &std::ffi::OsStr) -> io::Result<File> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_OPEN,
        Win32::Storage::FileSystem::{FILE_GENERIC_READ, SYNCHRONIZE},
    };
    let file = windows_nt_open_relative(
        &authority.parent,
        name,
        FILE_GENERIC_READ | SYNCHRONIZE,
        FILE_OPEN,
        Some(false),
    )?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_reparse_or_symlink(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "relative file is unsafe",
        ));
    }
    Ok(file)
}

#[cfg(windows)]
fn open_relative_directory(authority: &WriteAuthority, name: &std::ffi::OsStr) -> io::Result<File> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_OPEN,
        Win32::Storage::FileSystem::{FILE_GENERIC_READ, SYNCHRONIZE},
    };
    let directory = windows_nt_open_relative(
        &authority.parent,
        name,
        FILE_GENERIC_READ | SYNCHRONIZE,
        FILE_OPEN,
        Some(true),
    )?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || is_reparse_or_symlink(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "relative directory is unsafe",
        ));
    }
    Ok(directory)
}

#[cfg(not(any(unix, windows)))]
fn open_relative_read(authority: &WriteAuthority, name: &std::ffi::OsStr) -> io::Result<File> {
    open_read_no_follow(&authority.parent_path.join(name))
}

#[cfg(not(any(unix, windows)))]
fn open_relative_directory(authority: &WriteAuthority, name: &std::ffi::OsStr) -> io::Result<File> {
    open_directory_no_follow(&authority.parent_path.join(name))
}

fn relative_file_identity(
    authority: &WriteAuthority,
    name: &std::ffi::OsStr,
) -> Result<FileIdentity, StateError> {
    let file = open_relative_read(authority, name).map_err(StateError::Io)?;
    let metadata = file.metadata().map_err(StateError::Io)?;
    if !metadata.is_file() {
        return Err(StateError::ScopeChanged);
    }
    FileIdentity::from_open_file(&file, &metadata).map_err(StateError::Io)
}

#[cfg(unix)]
fn create_relative_private_file(
    authority: &WriteAuthority,
) -> Result<(File, OsString), StateError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    for _ in 0..16 {
        let name = OsString::from(format!(".mdviewer-save-{}", Uuid::new_v4()));
        let encoded = relative_c_string(&name).map_err(StateError::Io)?;
        let descriptor = unsafe {
            libc::openat(
                authority.parent.as_raw_fd(),
                encoded.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor >= 0 {
            return Ok((unsafe { File::from_raw_fd(descriptor) }, name));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(StateError::Io(error));
        }
    }
    Err(StateError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate private save file",
    )))
}

fn stage_asset_directory(
    authority: &WriteAuthority,
    source: &Path,
) -> Result<(OsString, Vec<OsString>, DirectoryIdentity), StateError> {
    let directory_name = OsString::from(format!(".mdviewer-assets-{}", Uuid::new_v4()));
    let mut source_entries = fs::read_dir(source)
        .map_err(StateError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StateError::Io)?;
    source_entries.sort_by_key(fs::DirEntry::file_name);
    let directory = create_relative_private_directory(authority, &directory_name)?;
    let mut copied_names = Vec::new();
    let result = (|| {
        for entry in source_entries {
            let metadata = fs::symlink_metadata(entry.path()).map_err(StateError::Io)?;
            if !metadata.is_file() || is_reparse_or_symlink(&metadata) {
                return Err(StateError::InvalidSelection);
            }
            let name = entry.file_name();
            let mut source_file = open_read_no_follow(&entry.path()).map_err(StateError::Io)?;
            let mut destination =
                create_file_in_directory(&directory, authority, &directory_name, &name)?;
            copied_names.push(name);
            authority
                .fail_test_point("asset_copy")
                .map_err(StateError::Io)?;
            io::copy(&mut source_file, &mut destination).map_err(StateError::Io)?;
            destination.sync_all().map_err(StateError::Io)?;
        }
        directory.sync_all().map_err(StateError::Io)
    })();
    if let Err(error) = result {
        drop(directory);
        let staging_authority = WriteAuthority {
            file_name: directory_name.clone(),
            ..authority.clone()
        };
        let _ = remove_known_relative_directory(&staging_authority, &copied_names);
        return Err(error);
    }
    let identity = match DirectoryIdentity::from_open_directory(&directory) {
        Ok(identity) => identity,
        Err(error) => {
            drop(directory);
            let staging_authority = WriteAuthority {
                file_name: directory_name.clone(),
                ..authority.clone()
            };
            let _ = remove_known_relative_directory(&staging_authority, &copied_names);
            return Err(StateError::Io(error));
        }
    };
    Ok((directory_name, copied_names, identity))
}

#[cfg(unix)]
fn create_relative_private_directory(
    authority: &WriteAuthority,
    name: &std::ffi::OsStr,
) -> Result<File, StateError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = relative_c_string(name).map_err(StateError::Io)?;
    if unsafe { libc::mkdirat(authority.parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        return Err(StateError::Io(io::Error::last_os_error()));
    }
    let descriptor = unsafe {
        libc::openat(
            authority.parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(StateError::Io(io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(windows)]
fn create_relative_private_directory(
    authority: &WriteAuthority,
    name: &std::ffi::OsStr,
) -> Result<File, StateError> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_CREATE,
        Win32::Storage::FileSystem::{
            DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, READ_CONTROL, SYNCHRONIZE, WRITE_DAC,
            WRITE_OWNER,
        },
    };
    let directory = windows_nt_open_relative(
        &authority.parent,
        name,
        FILE_GENERIC_READ
            | FILE_GENERIC_WRITE
            | DELETE
            | SYNCHRONIZE
            | READ_CONTROL
            | WRITE_DAC
            | WRITE_OWNER,
        FILE_CREATE,
        Some(true),
    )
    .map_err(StateError::Io)?;
    if let Err(error) = crate::jobs::apply_private_windows_security_to_handle(&directory, true) {
        let _ = windows_delete_open_handle(&directory);
        return Err(match error {
            crate::jobs::JobError::Io(error) => StateError::Io(error),
            _ => StateError::InvalidSelection,
        });
    }
    Ok(directory)
}

#[cfg(not(any(unix, windows)))]
fn create_relative_private_directory(
    authority: &WriteAuthority,
    name: &std::ffi::OsStr,
) -> Result<File, StateError> {
    let path = authority.parent_path.join(name);
    fs::create_dir(&path).map_err(StateError::Io)?;
    open_directory_no_follow(&path).map_err(StateError::Io)
}

#[cfg(windows)]
fn map_windows_security_error(error: crate::jobs::JobError) -> StateError {
    match error {
        crate::jobs::JobError::Io(error) => StateError::Io(error),
        _ => StateError::InvalidSelection,
    }
}

#[cfg(unix)]
fn create_file_in_directory(
    directory: &File,
    _authority: &WriteAuthority,
    _directory_name: &std::ffi::OsStr,
    name: &std::ffi::OsStr,
) -> Result<File, StateError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = relative_c_string(name).map_err(StateError::Io)?;
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if descriptor < 0 {
        Err(StateError::Io(io::Error::last_os_error()))
    } else {
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

#[cfg(windows)]
fn create_file_in_directory(
    directory: &File,
    _authority: &WriteAuthority,
    _directory_name: &std::ffi::OsStr,
    name: &std::ffi::OsStr,
) -> Result<File, StateError> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_CREATE,
        Win32::Storage::FileSystem::{
            DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, READ_CONTROL, SYNCHRONIZE, WRITE_DAC,
            WRITE_OWNER,
        },
    };
    let file = windows_nt_open_relative(
        directory,
        name,
        FILE_GENERIC_READ
            | FILE_GENERIC_WRITE
            | DELETE
            | SYNCHRONIZE
            | READ_CONTROL
            | WRITE_DAC
            | WRITE_OWNER,
        FILE_CREATE,
        Some(false),
    )
    .map_err(StateError::Io)?;
    if let Err(error) = crate::jobs::apply_private_windows_security_to_handle(&file, false) {
        let _ = windows_delete_open_handle(&file);
        return Err(map_windows_security_error(error));
    }
    Ok(file)
}

#[cfg(not(any(unix, windows)))]
fn create_file_in_directory(
    _directory: &File,
    authority: &WriteAuthority,
    directory_name: &std::ffi::OsStr,
    name: &std::ffi::OsStr,
) -> Result<File, StateError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(authority.parent_path.join(directory_name).join(name))
        .map_err(StateError::Io)
}

#[cfg(unix)]
fn remove_known_relative_directory(
    authority: &WriteAuthority,
    entries: &[OsString],
) -> io::Result<()> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let directory_name = relative_c_string(&authority.file_name)?;
    let descriptor = unsafe {
        libc::openat(
            authority.parent.as_raw_fd(),
            directory_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    let directory = unsafe { File::from_raw_fd(descriptor) };
    for entry in entries {
        let entry = relative_c_string(entry)?;
        if unsafe { libc::unlinkat(directory.as_raw_fd(), entry.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    if unsafe {
        libc::unlinkat(
            authority.parent.as_raw_fd(),
            directory_name.as_ptr(),
            libc::AT_REMOVEDIR,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn remove_known_relative_directory(
    authority: &WriteAuthority,
    entries: &[OsString],
) -> io::Result<()> {
    let directory = windows_open_relative_for_mutation(&authority.parent, &authority.file_name)?;
    for entry in entries {
        let file = windows_open_relative_for_mutation(&directory, entry)?;
        windows_delete_open_handle(&file)?;
    }
    windows_delete_open_handle(&directory)
}

#[cfg(not(any(unix, windows)))]
fn remove_known_relative_directory(
    authority: &WriteAuthority,
    entries: &[OsString],
) -> io::Result<()> {
    let directory = authority.parent_path.join(&authority.file_name);
    for entry in entries {
        fs::remove_file(directory.join(entry))?;
    }
    fs::remove_dir(directory)
}

#[cfg(windows)]
fn create_relative_private_file(
    authority: &WriteAuthority,
) -> Result<(File, OsString), StateError> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_CREATE,
        Win32::Storage::FileSystem::{
            DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, READ_CONTROL, SYNCHRONIZE, WRITE_DAC,
            WRITE_OWNER,
        },
    };
    for _ in 0..16 {
        let name = OsString::from(format!(".mdviewer-save-{}", Uuid::new_v4()));
        match windows_nt_open_relative(
            &authority.parent,
            &name,
            FILE_GENERIC_READ
                | FILE_GENERIC_WRITE
                | DELETE
                | SYNCHRONIZE
                | READ_CONTROL
                | WRITE_DAC
                | WRITE_OWNER,
            FILE_CREATE,
            Some(false),
        ) {
            Ok(file) => {
                if let Err(error) =
                    crate::jobs::apply_private_windows_security_to_handle(&file, false)
                {
                    let _ = windows_delete_open_handle(&file);
                    return Err(map_windows_security_error(error));
                }
                return Ok((file, name));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(StateError::Io(error)),
        }
    }
    Err(StateError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate private save file",
    )))
}

#[cfg(not(any(unix, windows)))]
fn create_relative_private_file(
    authority: &WriteAuthority,
) -> Result<(File, OsString), StateError> {
    for _ in 0..16 {
        let name = OsString::from(format!(".mdviewer-save-{}", Uuid::new_v4()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(authority.parent_path.join(&name))
        {
            Ok(file) => return Ok((file, name)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(StateError::Io(error)),
        }
    }
    Err(StateError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate private save file",
    )))
}

#[cfg(unix)]
fn remove_relative(authority: &WriteAuthority, name: &std::ffi::OsStr) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let name = relative_c_string(name)?;
    let result = unsafe { libc::unlinkat(authority.parent.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn remove_relative_if_exists(authority: &WriteAuthority, name: &std::ffi::OsStr) -> io::Result<()> {
    match remove_relative(authority, name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn remove_relative(authority: &WriteAuthority, name: &std::ffi::OsStr) -> io::Result<()> {
    let file = windows_open_relative_for_mutation(&authority.parent, name)?;
    windows_delete_open_handle(&file)
}

#[cfg(not(any(unix, windows)))]
fn remove_relative(authority: &WriteAuthority, name: &std::ffi::OsStr) -> io::Result<()> {
    fs::remove_file(authority.parent_path.join(name))
}

#[cfg(target_os = "linux")]
fn publish_new_relative(authority: &WriteAuthority, staged: &std::ffi::OsStr) -> io::Result<()> {
    renameat2_relative(
        authority,
        staged,
        &authority.file_name,
        libc::RENAME_NOREPLACE,
    )
}

#[cfg(target_os = "linux")]
fn replace_and_capture_relative(
    authority: &WriteAuthority,
    staged: &std::ffi::OsStr,
) -> io::Result<OsString> {
    renameat2_relative(
        authority,
        staged,
        &authority.file_name,
        libc::RENAME_EXCHANGE,
    )?;
    Ok(staged.to_os_string())
}

#[cfg(target_os = "linux")]
fn rollback_replace_relative(
    authority: &WriteAuthority,
    captured: &std::ffi::OsStr,
) -> io::Result<()> {
    renameat2_relative(
        authority,
        captured,
        &authority.file_name,
        libc::RENAME_EXCHANGE,
    )
}

#[cfg(windows)]
fn is_rollback_io_error(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|source| source.is::<RollbackIoError>())
}

#[cfg(not(windows))]
fn is_rollback_io_error(_error: &io::Error) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn retract_new_relative(
    authority: &WriteAuthority,
    private_name: &std::ffi::OsStr,
) -> io::Result<()> {
    renameat2_relative(
        authority,
        &authority.file_name,
        private_name,
        libc::RENAME_NOREPLACE,
    )
}

#[cfg(target_os = "linux")]
fn renameat2_relative(
    authority: &WriteAuthority,
    from: &std::ffi::OsStr,
    to: &std::ffi::OsStr,
    flags: u32,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let from = relative_c_string(from)?;
    let to = relative_c_string(to)?;
    let result = unsafe {
        libc::renameat2(
            authority.parent.as_raw_fd(),
            from.as_ptr(),
            authority.parent.as_raw_fd(),
            to.as_ptr(),
            flags,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn publish_new_relative(authority: &WriteAuthority, staged: &std::ffi::OsStr) -> io::Result<()> {
    renameatx_relative(authority, staged, &authority.file_name, libc::RENAME_EXCL)
}

#[cfg(target_os = "macos")]
fn replace_and_capture_relative(
    authority: &WriteAuthority,
    staged: &std::ffi::OsStr,
) -> io::Result<OsString> {
    renameatx_relative(authority, staged, &authority.file_name, libc::RENAME_SWAP)?;
    Ok(staged.to_os_string())
}

#[cfg(target_os = "macos")]
fn rollback_replace_relative(
    authority: &WriteAuthority,
    captured: &std::ffi::OsStr,
) -> io::Result<()> {
    renameatx_relative(authority, captured, &authority.file_name, libc::RENAME_SWAP)
}

#[cfg(target_os = "macos")]
fn retract_new_relative(
    authority: &WriteAuthority,
    private_name: &std::ffi::OsStr,
) -> io::Result<()> {
    renameatx_relative(
        authority,
        &authority.file_name,
        private_name,
        libc::RENAME_EXCL,
    )
}

#[cfg(target_os = "macos")]
fn renameatx_relative(
    authority: &WriteAuthority,
    from: &std::ffi::OsStr,
    to: &std::ffi::OsStr,
    flags: u32,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let from = relative_c_string(from)?;
    let to = relative_c_string(to)?;
    let result = unsafe {
        libc::renameatx_np(
            authority.parent.as_raw_fd(),
            from.as_ptr(),
            authority.parent.as_raw_fd(),
            to.as_ptr(),
            flags,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn publish_new_relative(authority: &WriteAuthority, staged: &std::ffi::OsStr) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let staged = relative_c_string(staged)?;
    let target = relative_c_string(&authority.file_name)?;
    let result = unsafe {
        libc::linkat(
            authority.parent.as_raw_fd(),
            staged.as_ptr(),
            authority.parent.as_raw_fd(),
            target.as_ptr(),
            0,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    remove_relative(authority, staged.as_c_str().to_bytes().as_ref())
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn replace_and_capture_relative(
    _authority: &WriteAuthority,
    _staged: &std::ffi::OsStr,
) -> io::Result<OsString> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic replacement is unavailable",
    ))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn rollback_replace_relative(
    _authority: &WriteAuthority,
    _captured: &std::ffi::OsStr,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic replacement is unavailable",
    ))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn retract_new_relative(
    authority: &WriteAuthority,
    private_name: &std::ffi::OsStr,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let target = relative_c_string(&authority.file_name)?;
    let private_name = relative_c_string(private_name)?;
    if unsafe {
        libc::linkat(
            authority.parent.as_raw_fd(),
            target.as_ptr(),
            authority.parent.as_raw_fd(),
            private_name.as_ptr(),
            0,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    remove_relative(authority, &authority.file_name)
}

#[cfg(windows)]
fn publish_new_relative(authority: &WriteAuthority, staged: &std::ffi::OsStr) -> io::Result<()> {
    windows_rename_relative(authority, staged, &authority.file_name)
}

#[cfg(windows)]
fn replace_and_capture_relative(
    authority: &WriteAuthority,
    staged: &std::ffi::OsStr,
) -> io::Result<OsString> {
    let captured = OsString::from(format!(".mdviewer-backup-{}", Uuid::new_v4()));
    let target = windows_open_relative_for_mutation(&authority.parent, &authority.file_name)?;
    windows_rename_open_handle(&target, &authority.parent, &captured)?;
    if let Err(error) = windows_rename_relative(authority, staged, &authority.file_name) {
        if windows_rename_open_handle(&target, &authority.parent, &authority.file_name).is_err() {
            return Err(io::Error::other(RollbackIoError));
        }
        return Err(error);
    }
    Ok(captured)
}

#[cfg(windows)]
fn rollback_replace_relative(
    authority: &WriteAuthority,
    captured: &std::ffi::OsStr,
) -> io::Result<()> {
    let displaced = OsString::from(format!(".mdviewer-rollback-{}", Uuid::new_v4()));
    let published = windows_open_relative_for_mutation(&authority.parent, &authority.file_name)?;
    windows_rename_open_handle(&published, &authority.parent, &displaced)?;
    let original = windows_open_relative_for_mutation(&authority.parent, captured)?;
    if let Err(error) =
        windows_rename_open_handle(&original, &authority.parent, &authority.file_name)
    {
        let _ = windows_rename_open_handle(&published, &authority.parent, &authority.file_name);
        return Err(error);
    }
    windows_rename_open_handle(&published, &authority.parent, captured)
}

#[cfg(windows)]
fn retract_new_relative(
    authority: &WriteAuthority,
    private_name: &std::ffi::OsStr,
) -> io::Result<()> {
    windows_rename_relative(authority, &authority.file_name, private_name)
}

#[cfg(not(any(unix, windows)))]
fn publish_new_relative(authority: &WriteAuthority, staged: &std::ffi::OsStr) -> io::Result<()> {
    fs::rename(
        authority.parent_path.join(staged),
        authority.parent_path.join(&authority.file_name),
    )
}

#[cfg(not(any(unix, windows)))]
fn replace_and_capture_relative(
    _authority: &WriteAuthority,
    _staged: &std::ffi::OsStr,
) -> io::Result<OsString> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic replacement is unavailable",
    ))
}

#[cfg(not(any(unix, windows)))]
fn rollback_replace_relative(
    _authority: &WriteAuthority,
    _captured: &std::ffi::OsStr,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic replacement is unavailable",
    ))
}

#[cfg(not(any(unix, windows)))]
fn retract_new_relative(
    authority: &WriteAuthority,
    private_name: &std::ffi::OsStr,
) -> io::Result<()> {
    fs::rename(
        authority.parent_path.join(&authority.file_name),
        authority.parent_path.join(private_name),
    )
}

#[cfg(unix)]
fn platform_file_identity(_file: &File, metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    Ok(FileIdentity {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        device: metadata.dev(),
        inode: metadata.ino(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
        links: metadata.nlink(),
    })
}

#[cfg(windows)]
fn platform_file_identity(file: &File, metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    let state = windows_file_state(file)?;
    Ok(FileIdentity {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        volume: state.volume_serial_number,
        file_id: state.file_id,
        changed: state.change_time,
        last_write: state.last_write_time,
        links: state.links,
    })
}

#[cfg(not(any(unix, windows)))]
fn platform_file_identity(_file: &File, metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    Ok(FileIdentity {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

#[cfg(windows)]
struct WindowsFileState {
    volume_serial_number: u64,
    file_id: [u8; 16],
    change_time: i64,
    last_write_time: i64,
    links: u32,
}

#[cfg(windows)]
fn windows_file_state(file: &File) -> io::Result<WindowsFileState> {
    use std::{mem::size_of, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FILE_ID_INFO, FileBasicInfo, FileIdInfo,
        GetFileInformationByHandle, GetFileInformationByHandleEx,
    };

    let handle = file.as_raw_handle();
    let mut id = FILE_ID_INFO::default();
    let mut basic = FILE_BASIC_INFO::default();
    let mut links = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    let id_ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&raw mut id).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    let basic_ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileBasicInfo,
            (&raw mut basic).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    let links_ok = unsafe { GetFileInformationByHandle(handle, links.as_mut_ptr()) };
    if id_ok == 0 || basic_ok == 0 || links_ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let links = unsafe { links.assume_init() };
    Ok(WindowsFileState {
        volume_serial_number: id.VolumeSerialNumber,
        file_id: id.FileId.Identifier,
        change_time: basic.ChangeTime,
        last_write_time: basic.LastWriteTime,
        links: links.nNumberOfLinks,
    })
}

fn open_read_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    options.open(path)
}

fn validate_uuid_v4(value: &str) -> Result<(), StateError> {
    let uuid = Uuid::parse_str(value).map_err(|_| StateError::InvalidOperationId)?;
    if uuid.get_version() != Some(Version::Random) || uuid.hyphenated().to_string() != value {
        return Err(StateError::InvalidOperationId);
    }
    Ok(())
}

fn validate_selected_path(path: &Path, access: SelectionAccess) -> Result<PathBuf, StateError> {
    if !is_local_absolute(path) {
        return Err(StateError::InvalidSelection);
    }
    if path.file_name().is_none() {
        return Err(StateError::InvalidSelection);
    }
    if access == SelectionAccess::Read {
        let metadata = fs::symlink_metadata(path).map_err(|_| StateError::InvalidSelection)?;
        if !metadata.is_file() || is_reparse_or_symlink(&metadata) {
            return Err(StateError::InvalidSelection);
        }
    }
    let parent = path.parent().ok_or(StateError::InvalidSelection)?;
    let parent = fs::canonicalize(parent).map_err(|_| StateError::InvalidSelection)?;
    let metadata = fs::metadata(&parent).map_err(|_| StateError::InvalidSelection)?;
    if !metadata.is_dir() {
        return Err(StateError::InvalidSelection);
    }
    let name = path.file_name().ok_or(StateError::InvalidSelection)?;
    let normalized = parent.join(name);
    if access == SelectionAccess::Read {
        fs::canonicalize(&normalized).map_err(|_| StateError::InvalidSelection)
    } else {
        Ok(normalized)
    }
}

fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }
    #[cfg(not(windows))]
    false
}

fn is_local_absolute(path: &Path) -> bool {
    if !path.is_absolute() || path.to_str().is_none() {
        return false;
    }
    let text = path.to_string_lossy();
    if text.starts_with("//") || text.starts_with("\\\\") || text.contains("://") {
        return false;
    }
    !path.components().any(|component| match component {
        Component::Prefix(prefix) => !matches!(prefix.kind(), std::path::Prefix::Disk(_)),
        _ => false,
    })
}

fn create_private_directory(path: &Path) -> Result<(), StateError> {
    let created = !path.exists();
    if created {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            builder.create(path).map_err(StateError::Io)?;
        }
        #[cfg(not(unix))]
        fs::create_dir(path).map_err(StateError::Io)?;
    }
    let metadata = fs::symlink_metadata(path).map_err(StateError::Io)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(StateError::InvalidSelection);
    }
    #[cfg(windows)]
    if created {
        crate::jobs::apply_private_windows_security(path, true).map_err(|error| match error {
            crate::jobs::JobError::Io(error) => StateError::Io(error),
            _ => StateError::InvalidSelection,
        })?;
    }
    #[cfg(windows)]
    crate::jobs::validate_private_windows_security(path, true)
        .map_err(|_| StateError::InvalidSelection)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o7777 != 0o700 {
            return Err(StateError::InvalidSelection);
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn _sync_marker(file: &File) -> Result<(), StateError> {
    file.sync_all().map_err(StateError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_conversion_does_not_remove_the_active_cancel_marker() {
        let temp = std::env::temp_dir().join(format!(
            "mdviewer-task12-state-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        let scope = temp.join("scope");
        fs::create_dir_all(&scope).unwrap();
        let jobs = PrintJobStore::new(temp.join("jobs"), [&scope]).unwrap();
        let state = AppState::new(jobs, temp.join("runtime")).unwrap();
        let operation_id = Uuid::new_v4().to_string();

        let marker = state.begin_conversion(&operation_id).unwrap();
        state.cancel_conversion(&operation_id).unwrap();
        assert!(marker.exists());

        assert_eq!(
            state.begin_conversion(&operation_id).unwrap_err().code(),
            "conversion_already_running"
        );
        assert!(marker.exists(), "duplicate request removed cancellation");

        state.end_conversion(&operation_id).unwrap();
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn restarting_an_operation_clears_stale_warning_codes() {
        let temp = std::env::temp_dir().join(format!(
            "mdviewer-task12-warning-state-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        let scope = temp.join("scope");
        fs::create_dir_all(&scope).unwrap();
        let jobs = PrintJobStore::new(temp.join("jobs"), [&scope]).unwrap();
        let state = AppState::new(jobs, temp.join("runtime")).unwrap();
        let operation_id = Uuid::new_v4().to_string();

        state.begin_conversion(&operation_id).unwrap();
        state
            .record_warnings(&operation_id, vec!["old_warning".to_owned()])
            .unwrap();
        state.end_conversion(&operation_id).unwrap();
        state.begin_conversion(&operation_id).unwrap();

        assert!(state.warning_codes(&operation_id).unwrap().is_empty());
        state.end_conversion(&operation_id).unwrap();
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn successful_claim_dequeues_only_the_matching_pending_job() {
        let temp = std::env::temp_dir().join(format!(
            "mdviewer-task12-pending-state-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        let scope = temp.join("scope");
        fs::create_dir_all(&scope).unwrap();
        let source = scope.join("input.pdf");
        fs::write(&source, b"%PDF-1.7\n%%EOF\n").unwrap();
        let jobs = PrintJobStore::new(temp.join("jobs"), [&scope]).unwrap();
        let state = AppState::new(jobs, temp.join("runtime")).unwrap();
        let pending = state.jobs().stage_pdf(&source, None).unwrap();
        let other = PrintJobId::new();
        state.queue_print_job(pending.id).unwrap();
        state.queue_print_job(other).unwrap();

        crate::commands::claim_print_job(&state, &pending.id.to_string()).unwrap();

        assert_eq!(state.pending_print_jobs().unwrap(), vec![other]);
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn already_claimed_job_is_terminal_and_dequeued_when_deep_link_repeats() {
        let temp = std::env::temp_dir().join(format!(
            "mdviewer-task12-terminal-pending-state-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        let scope = temp.join("scope");
        fs::create_dir_all(&scope).unwrap();
        let source = scope.join("input.pdf");
        fs::write(&source, b"%PDF-1.7\n%%EOF\n").unwrap();
        let jobs = PrintJobStore::new(temp.join("jobs"), [&scope]).unwrap();
        let state = AppState::new(jobs, temp.join("runtime")).unwrap();
        let pending = state.jobs().stage_pdf(&source, None).unwrap();

        crate::commands::claim_print_job(&state, &pending.id.to_string()).unwrap();
        state.queue_print_job(pending.id).unwrap();
        assert_eq!(
            crate::commands::claim_print_job(&state, &pending.id.to_string())
                .unwrap_err()
                .code,
            "already_claimed"
        );

        assert!(state.pending_print_jobs().unwrap().is_empty());
        fs::remove_dir_all(temp).unwrap();
    }

    fn private_transaction_entries(directory: &Path) -> Vec<PathBuf> {
        let mut entries = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(".mdviewer-"))
            })
            .collect::<Vec<_>>();
        entries.sort();
        entries
    }

    fn writable_selection(temp: &Path, target: &Path) -> AuthorizedSelection {
        let scope = temp.join("scope");
        fs::create_dir_all(&scope).unwrap();
        let jobs = PrintJobStore::new(temp.join("jobs"), [&scope]).unwrap();
        let state = AppState::new(jobs, temp.join("runtime")).unwrap();
        let token = state
            .authorize_user_selection(target, SelectionAccess::Write)
            .unwrap();
        state.selection(&token, SelectionAccess::Write).unwrap()
    }

    #[test]
    fn existing_save_rolls_back_exchange_when_target_changes_after_initial_verification() {
        let temp = std::env::temp_dir().join(format!(
            "mdviewer-task12-exchange-rollback-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(temp.join("scope")).unwrap();
        let target = temp.join("scope/note.md");
        fs::write(&target, "authorized original").unwrap();
        let selection = writable_selection(&temp, &target);
        selection.inject_test_fault(TestFault::ReplaceTargetBeforeExchange);

        assert_eq!(
            selection
                .persist_content(b"new content")
                .unwrap_err()
                .code(),
            "scope_changed"
        );
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "concurrent replacement"
        );
        assert!(private_transaction_entries(target.parent().unwrap()).is_empty());
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn existing_save_restores_original_when_parent_sync_fails() {
        let temp = std::env::temp_dir().join(format!(
            "mdviewer-task12-save-sync-rollback-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(temp.join("scope")).unwrap();
        let target = temp.join("scope/note.md");
        fs::write(&target, "authorized original").unwrap();
        let selection = writable_selection(&temp, &target);
        selection.inject_test_fault(TestFault::FailParentSync);

        assert_eq!(
            selection
                .persist_content(b"new content")
                .unwrap_err()
                .code(),
            "state_io"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "authorized original");
        assert!(private_transaction_entries(target.parent().unwrap()).is_empty());
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn new_save_retracts_published_target_when_parent_sync_fails() {
        let temp = std::env::temp_dir().join(format!(
            "mdviewer-task12-new-save-sync-rollback-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(temp.join("scope")).unwrap();
        let target = temp.join("scope/note.md");
        let selection = writable_selection(&temp, &target);
        selection.inject_test_fault(TestFault::FailParentSync);

        assert_eq!(
            selection
                .persist_content(b"new content")
                .unwrap_err()
                .code(),
            "state_io"
        );
        assert!(!target.exists());
        assert!(private_transaction_entries(target.parent().unwrap()).is_empty());
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn failed_rollback_preserves_private_recovery_artifacts_and_is_reported() {
        let temp = std::env::temp_dir().join(format!(
            "mdviewer-task12-save-recovery-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(temp.join("scope")).unwrap();
        let target = temp.join("scope/note.md");
        fs::write(&target, "authorized original").unwrap();
        let selection = writable_selection(&temp, &target);
        selection.inject_test_fault(TestFault::FailParentSync);
        selection.inject_test_fault(TestFault::FailRollback);

        assert_eq!(
            selection
                .persist_content(b"new content")
                .unwrap_err()
                .code(),
            "recovery_required"
        );
        let recovery = private_transaction_entries(target.parent().unwrap());
        assert!(
            recovery.len() >= 2,
            "missing recovery artifacts: {recovery:?}"
        );
        assert!(recovery.iter().any(|path| {
            path.is_file()
                && fs::read_to_string(path).is_ok_and(|content| content == "authorized original")
        }));
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn conversion_rolls_assets_back_when_markdown_publication_fails() {
        let temp = std::env::temp_dir().join(format!(
            "mdviewer-task12-convert-rollback-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(temp.join("scope")).unwrap();
        let target = temp.join("scope/result.md");
        let selection = writable_selection(&temp, &target);
        let staging_root = temp.join("converter-staging");
        fs::create_dir_all(staging_root.join("result.assets")).unwrap();
        fs::write(staging_root.join("result.md"), "# staged").unwrap();
        fs::write(staging_root.join("result.assets/image.png"), b"image").unwrap();
        let staging = ConversionStaging {
            markdown: staging_root.join("result.md"),
            assets: staging_root.join("result.assets"),
            directory: staging_root,
        };
        selection.inject_test_fault(TestFault::FailAfterAssetsPublished);

        assert_eq!(
            selection.publish_conversion(&staging).unwrap_err().code(),
            "state_io"
        );
        assert!(!target.exists());
        assert!(!target.with_extension("assets").exists());
        assert!(private_transaction_entries(target.parent().unwrap()).is_empty());
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn conversion_retracts_all_outputs_when_parent_sync_fails() {
        let temp = std::env::temp_dir().join(format!(
            "mdviewer-task12-convert-sync-rollback-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(temp.join("scope")).unwrap();
        let target = temp.join("scope/result.md");
        let selection = writable_selection(&temp, &target);
        let staging_root = temp.join("converter-staging");
        fs::create_dir_all(staging_root.join("result.assets")).unwrap();
        fs::write(staging_root.join("result.md"), "# staged").unwrap();
        fs::write(staging_root.join("result.assets/image.png"), b"image").unwrap();
        let staging = ConversionStaging {
            markdown: staging_root.join("result.md"),
            assets: staging_root.join("result.assets"),
            directory: staging_root,
        };
        selection.inject_test_fault(TestFault::FailParentSync);

        assert_eq!(
            selection.publish_conversion(&staging).unwrap_err().code(),
            "state_io"
        );
        assert!(!target.exists());
        assert!(!target.with_extension("assets").exists());
        assert!(private_transaction_entries(target.parent().unwrap()).is_empty());
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn partially_created_asset_is_registered_before_copy_failure_cleanup() {
        let temp = std::env::temp_dir().join(format!(
            "mdviewer-task12-assets-partial-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(temp.join("scope")).unwrap();
        let target = temp.join("scope/result.md");
        let selection = writable_selection(&temp, &target);
        let staging_root = temp.join("converter-staging");
        fs::create_dir_all(staging_root.join("result.assets")).unwrap();
        fs::write(staging_root.join("result.md"), "# staged").unwrap();
        fs::write(staging_root.join("result.assets/image.png"), b"image").unwrap();
        let staging = ConversionStaging {
            markdown: staging_root.join("result.md"),
            assets: staging_root.join("result.assets"),
            directory: staging_root,
        };
        selection.inject_test_fault(TestFault::FailAssetCopyAfterCreate);

        assert_eq!(
            selection.publish_conversion(&staging).unwrap_err().code(),
            "state_io"
        );
        assert!(private_transaction_entries(target.parent().unwrap()).is_empty());
        fs::remove_dir_all(temp).unwrap();
    }
}
