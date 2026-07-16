use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{self, Read},
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
            Self::InvalidOperationId => "invalid_operation_id",
            Self::AlreadyRunning => "conversion_already_running",
            Self::NotRunning => "conversion_not_running",
            Self::Io(_) => "state_io",
        }
    }
}

#[derive(Debug)]
pub struct AppState {
    jobs: PrintJobStore,
    runtime: PathBuf,
    selections: RwLock<HashMap<String, AuthorizedSelection>>,
    conversions: Mutex<HashMap<String, RunningConversion>>,
    warnings: RwLock<HashMap<String, Vec<String>>>,
    pending_print_jobs: Mutex<VecDeque<PrintJobId>>,
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
            selections: RwLock::new(HashMap::new()),
            conversions: Mutex::new(HashMap::new()),
            warnings: RwLock::new(HashMap::new()),
            pending_print_jobs: Mutex::new(VecDeque::new()),
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

        let mut source =
            open_read_no_follow(&selection.path).map_err(|_| StateError::SourceChanged)?;
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

    pub(crate) fn begin_conversion(&self, operation_id: &str) -> Result<PathBuf, StateError> {
        validate_uuid_v4(operation_id)?;
        let marker = self.runtime.join(format!("cancel-{operation_id}"));
        let mut conversions = self.conversions.lock().map_err(|_| StateError::Poisoned)?;
        if conversions.contains_key(operation_id) {
            return Err(StateError::AlreadyRunning);
        }
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
}

impl AuthorizedSelection {
    fn verify(&self) -> Result<(), StateError> {
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
}

impl FileIdentity {
    fn from_open_file(file: &File, metadata: &fs::Metadata) -> io::Result<Self> {
        platform_file_identity(file, metadata)
    }
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
    if !path.exists() {
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
}
