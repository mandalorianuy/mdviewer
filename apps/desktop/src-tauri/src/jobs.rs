use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use mdconvert_core::ConversionLimits;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::{Uuid, Version};

const JOB_SCHEMA: &str = "mdviewer.print-job/v1";
const INPUT_NAME: &str = "input.pdf";
const METADATA_NAME: &str = "metadata.json";
const MAX_METADATA_BYTES: u64 = 16 * 1024;
const MAX_TITLE_CHARS: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PrintJobId(Uuid);

impl PrintJobId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn parse(value: &str) -> Result<Self, JobError> {
        let uuid = Uuid::parse_str(value).map_err(|_| JobError::InvalidId)?;
        if uuid.get_version() != Some(Version::Random) || uuid.hyphenated().to_string() != value {
            return Err(JobError::InvalidId);
        }
        Ok(Self(uuid))
    }
}

impl Default for PrintJobId {
    fn default() -> Self {
        Self::new()
    }
}

impl FromStr for PrintJobId {
    type Err = JobError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for PrintJobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Staged,
    Claimed,
}

impl JobState {
    fn suffix(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Claimed => "claimed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintJob {
    pub id: PrintJobId,
    pub state: JobState,
    pub title: String,
    pub created_unix_ms: u64,
    pub directory: PathBuf,
    pub input_pdf: PathBuf,
    pub metadata_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CleanupRejection {
    pub job_id: Option<PrintJobId>,
    pub code: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CleanupReport {
    pub removed_staged: Vec<PrintJobId>,
    pub removed_claimed: Vec<PrintJobId>,
    pub removed_incomplete: u64,
    pub skipped_recent: Vec<PrintJobId>,
    pub skipped_recent_incomplete: u64,
    pub rejected: Vec<CleanupRejection>,
}

#[derive(Debug, Error)]
pub enum JobError {
    #[error("print job root is invalid")]
    InvalidRoot,
    #[error("print source is outside an authorized scope")]
    UnauthorizedSource,
    #[error("print job path is unsafe")]
    UnsafePath,
    #[error("print source is not a supported PDF")]
    InvalidPdf,
    #[error("print source exceeds the input limit")]
    LimitExceeded,
    #[error("print job ID is invalid")]
    InvalidId,
    #[error("print job was not found")]
    NotFound,
    #[error("print job was already claimed")]
    AlreadyClaimed,
    #[error("print job has not been claimed")]
    NotClaimed,
    #[error("print job input is missing")]
    MissingInput,
    #[error("print job metadata is invalid")]
    InvalidMetadata,
    #[error("print job operation failed")]
    Io(#[source] io::Error),
}

impl JobError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidRoot => "invalid_job_root",
            Self::UnauthorizedSource => "unauthorized_source",
            Self::UnsafePath => "unsafe_job_path",
            Self::InvalidPdf => "invalid_pdf",
            Self::LimitExceeded => "limit_exceeded",
            Self::InvalidId => "invalid_job_id",
            Self::NotFound => "job_not_found",
            Self::AlreadyClaimed => "already_claimed",
            Self::NotClaimed => "job_not_claimed",
            Self::MissingInput => "missing_input",
            Self::InvalidMetadata => "invalid_job_metadata",
            Self::Io(_) => "job_io",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrintJobStore {
    root: PathBuf,
    authorized_scopes: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobMetadata {
    schema: String,
    id: String,
    title: String,
    created_unix_ms: u64,
}

impl PrintJobStore {
    pub fn new<I, P>(root: impl AsRef<Path>, authorized_scopes: I) -> Result<Self, JobError>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let requested_root = root.as_ref();
        if !is_local_absolute(requested_root) {
            return Err(JobError::InvalidRoot);
        }
        create_private_directory(requested_root)?;
        let root = fs::canonicalize(requested_root).map_err(JobError::Io)?;
        validate_private_directory(&root, true)?;

        let mut scopes = Vec::new();
        for scope in authorized_scopes {
            let scope = scope.as_ref();
            if !is_local_absolute(scope) {
                return Err(JobError::UnsafePath);
            }
            let canonical = fs::canonicalize(scope).map_err(|_| JobError::UnsafePath)?;
            let metadata = fs::metadata(&canonical).map_err(|_| JobError::UnsafePath)?;
            if !metadata.is_dir()
                || is_reparse_or_symlink(
                    &fs::symlink_metadata(scope).map_err(|_| JobError::UnsafePath)?,
                )
            {
                return Err(JobError::UnsafePath);
            }
            scopes.push(canonical);
        }
        scopes.sort();
        scopes.dedup();

        Ok(Self {
            root,
            authorized_scopes: scopes,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn stage_pdf(&self, source: &Path, title: Option<&str>) -> Result<PrintJob, JobError> {
        self.validate_root()?;
        let mut source_file = self.open_authorized_source(source)?;
        let source_metadata = source_file.metadata().map_err(JobError::Io)?;
        validate_regular_single_link_file(&source_metadata)?;
        validate_open_file_links(&source_file)?;
        let max_bytes = ConversionLimits::default().max_input_bytes;
        if source_metadata.len() > max_bytes {
            return Err(JobError::LimitExceeded);
        }

        let mut signature = [0_u8; 5];
        source_file
            .read_exact(&mut signature)
            .map_err(|_| JobError::InvalidPdf)?;
        if signature != *b"%PDF-" {
            return Err(JobError::InvalidPdf);
        }
        source_file.seek(SeekFrom::Start(0)).map_err(JobError::Io)?;

        let id = PrintJobId::new();
        let nonce = Uuid::new_v4();
        let temporary = self.root.join(format!(".stage-{id}-{nonce}"));
        let final_path = self.job_path(id, JobState::Staged);
        create_private_directory_new(&temporary)?;

        let result = (|| {
            let input_pdf = temporary.join(INPUT_NAME);
            let mut staged = create_private_file(&input_pdf)?;
            let copied = io::copy(
                &mut Read::by_ref(&mut source_file).take(max_bytes + 1),
                &mut staged,
            )
            .map_err(JobError::Io)?;
            if copied > max_bytes || copied != source_metadata.len() {
                return Err(JobError::LimitExceeded);
            }
            staged.sync_all().map_err(JobError::Io)?;

            let after = source_file.metadata().map_err(JobError::Io)?;
            validate_open_file_links(&source_file)?;
            if !same_file_snapshot(&source_metadata, &after) {
                return Err(JobError::UnsafePath);
            }

            let title = sanitize_title(title);
            let created_unix_ms = now_unix_ms()?;
            let metadata = JobMetadata {
                schema: JOB_SCHEMA.to_owned(),
                id: id.to_string(),
                title: title.clone(),
                created_unix_ms,
            };
            let mut bytes = serde_json::to_vec(&metadata).map_err(|_| JobError::InvalidMetadata)?;
            bytes.push(b'\n');
            let mut metadata_file = create_private_file(&temporary.join(METADATA_NAME))?;
            metadata_file.write_all(&bytes).map_err(JobError::Io)?;
            metadata_file.sync_all().map_err(JobError::Io)?;
            sync_directory(&temporary)?;
            rename_noreplace(&temporary, &final_path)?;
            sync_directory(&self.root)?;
            self.load_job(id, JobState::Staged)
        })();

        if result.is_err() && temporary.exists() {
            let _ = remove_known_directory(&temporary);
        }
        result
    }

    pub fn claim(&self, id: PrintJobId) -> Result<PrintJob, JobError> {
        self.validate_root()?;
        let staged = self.job_path(id, JobState::Staged);
        let claimed = self.job_path(id, JobState::Claimed);
        if path_exists_no_follow(&claimed)? {
            self.load_job(id, JobState::Claimed)?;
            return Err(JobError::AlreadyClaimed);
        }
        if !path_exists_no_follow(&staged)? {
            return Err(JobError::NotFound);
        }
        self.load_job(id, JobState::Staged)?;
        rename_noreplace(&staged, &claimed)?;
        sync_directory(&self.root)?;
        self.load_job(id, JobState::Claimed)
    }

    pub fn finish(&self, id: PrintJobId) -> Result<(), JobError> {
        self.validate_root()?;
        let claimed = self.job_path(id, JobState::Claimed);
        if !path_exists_no_follow(&claimed)? {
            if path_exists_no_follow(&self.job_path(id, JobState::Staged))? {
                return Err(JobError::NotClaimed);
            }
            return Err(JobError::NotFound);
        }
        self.load_job(id, JobState::Claimed)?;
        remove_known_job_directory(&claimed)?;
        sync_directory(&self.root)
    }

    pub fn cleanup_older_than(&self, age: Duration) -> Result<CleanupReport, JobError> {
        self.validate_root()?;
        let cutoff = SystemTime::now().checked_sub(age).unwrap_or(UNIX_EPOCH);
        let mut entries = fs::read_dir(&self.root)
            .map_err(JobError::Io)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(JobError::Io)?;
        entries.sort_by_key(fs::DirEntry::file_name);
        let mut report = CleanupReport::default();

        for entry in entries {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                report.rejected.push(CleanupRejection {
                    job_id: None,
                    code: "invalid_entry_name",
                });
                continue;
            };
            let Some((id, state)) = parse_job_directory_name(&name) else {
                if parse_incomplete_directory_name(&name) {
                    let metadata = match fs::symlink_metadata(entry.path()) {
                        Ok(metadata) if !is_reparse_or_symlink(&metadata) && metadata.is_dir() => {
                            metadata
                        }
                        _ => {
                            report.rejected.push(CleanupRejection {
                                job_id: None,
                                code: "unsafe_job_path",
                            });
                            continue;
                        }
                    };
                    let modified = metadata.modified().unwrap_or(SystemTime::now());
                    if modified >= cutoff {
                        report.skipped_recent_incomplete += 1;
                        continue;
                    }
                    match remove_incomplete_directory(&self.root, &entry.path()) {
                        Ok(()) => report.removed_incomplete += 1,
                        Err(error) => report.rejected.push(CleanupRejection {
                            job_id: None,
                            code: error.code(),
                        }),
                    }
                } else {
                    report.rejected.push(CleanupRejection {
                        job_id: None,
                        code: "unexpected_entry",
                    });
                }
                continue;
            };
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) if !is_reparse_or_symlink(&metadata) && metadata.is_dir() => metadata,
                _ => {
                    report.rejected.push(CleanupRejection {
                        job_id: Some(id),
                        code: "unsafe_job_path",
                    });
                    continue;
                }
            };
            let modified = metadata.modified().unwrap_or(SystemTime::now());
            if modified >= cutoff {
                report.skipped_recent.push(id);
                continue;
            }
            match self
                .load_job(id, state)
                .and_then(|_| remove_known_job_directory(&entry.path()))
            {
                Ok(()) => match state {
                    JobState::Staged => report.removed_staged.push(id),
                    JobState::Claimed => report.removed_claimed.push(id),
                },
                Err(error) => report.rejected.push(CleanupRejection {
                    job_id: Some(id),
                    code: error.code(),
                }),
            }
        }
        sync_directory(&self.root)?;
        Ok(report)
    }

    fn open_authorized_source(&self, source: &Path) -> Result<File, JobError> {
        if !is_local_absolute(source) {
            return Err(JobError::UnauthorizedSource);
        }
        let lexical_metadata =
            fs::symlink_metadata(source).map_err(|_| JobError::UnauthorizedSource)?;
        if is_reparse_or_symlink(&lexical_metadata) {
            return Err(JobError::UnsafePath);
        }
        let canonical = fs::canonicalize(source).map_err(|_| JobError::UnauthorizedSource)?;
        if !self
            .authorized_scopes
            .iter()
            .any(|scope| canonical.starts_with(scope) && canonical != *scope)
        {
            return Err(JobError::UnauthorizedSource);
        }
        open_read_no_follow(source).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
                JobError::UnauthorizedSource
            }
            _ => JobError::Io(error),
        })
    }

    fn validate_root(&self) -> Result<(), JobError> {
        validate_private_directory(&self.root, true)?;
        let canonical = fs::canonicalize(&self.root).map_err(|_| JobError::InvalidRoot)?;
        if canonical != self.root {
            return Err(JobError::InvalidRoot);
        }
        Ok(())
    }

    fn job_path(&self, id: PrintJobId, state: JobState) -> PathBuf {
        self.root.join(format!("{id}.{}", state.suffix()))
    }

    fn load_job(&self, id: PrintJobId, state: JobState) -> Result<PrintJob, JobError> {
        let directory = self.job_path(id, state);
        validate_job_directory(&self.root, &directory)?;

        let mut entry_names = fs::read_dir(&directory)
            .map_err(JobError::Io)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(JobError::Io)?;
        entry_names.sort();
        if entry_names
            != [
                std::ffi::OsString::from(INPUT_NAME),
                std::ffi::OsString::from(METADATA_NAME),
            ]
        {
            if !entry_names.iter().any(|name| name == INPUT_NAME) {
                return Err(JobError::MissingInput);
            }
            if !entry_names.iter().any(|name| name == METADATA_NAME) {
                return Err(JobError::InvalidMetadata);
            }
            return Err(JobError::UnsafePath);
        }

        let input_pdf = directory.join(INPUT_NAME);
        let input_metadata = fs::symlink_metadata(&input_pdf).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                JobError::MissingInput
            } else {
                JobError::Io(error)
            }
        })?;
        validate_regular_single_link_file(&input_metadata)?;
        validate_private_file_permissions(&input_metadata)?;
        let mut input = open_read_no_follow(&input_pdf).map_err(JobError::Io)?;
        validate_open_file_links(&input)?;
        let mut signature = [0_u8; 5];
        input
            .read_exact(&mut signature)
            .map_err(|_| JobError::InvalidPdf)?;
        if signature != *b"%PDF-"
            || input_metadata.len() > ConversionLimits::default().max_input_bytes
        {
            return Err(JobError::InvalidPdf);
        }

        let metadata_path = directory.join(METADATA_NAME);
        let file_metadata =
            fs::symlink_metadata(&metadata_path).map_err(|_| JobError::InvalidMetadata)?;
        validate_regular_single_link_file(&file_metadata).map_err(|_| JobError::InvalidMetadata)?;
        validate_private_file_permissions(&file_metadata).map_err(|_| JobError::InvalidMetadata)?;
        if file_metadata.len() > MAX_METADATA_BYTES {
            return Err(JobError::InvalidMetadata);
        }
        let metadata_file =
            open_read_no_follow(&metadata_path).map_err(|_| JobError::InvalidMetadata)?;
        validate_open_file_links(&metadata_file).map_err(|_| JobError::InvalidMetadata)?;
        let mut raw = Vec::with_capacity(file_metadata.len() as usize);
        metadata_file
            .take(MAX_METADATA_BYTES + 1)
            .read_to_end(&mut raw)
            .map_err(|_| JobError::InvalidMetadata)?;
        let metadata: JobMetadata =
            serde_json::from_slice(&raw).map_err(|_| JobError::InvalidMetadata)?;
        if metadata.schema != JOB_SCHEMA
            || metadata.id != id.to_string()
            || metadata.title != sanitize_title(Some(&metadata.title))
            || metadata.created_unix_ms > now_unix_ms()?.saturating_add(300_000)
        {
            return Err(JobError::InvalidMetadata);
        }

        Ok(PrintJob {
            id,
            state,
            title: metadata.title,
            created_unix_ms: metadata.created_unix_ms,
            directory,
            input_pdf,
            metadata_path,
        })
    }
}

fn sanitize_title(title: Option<&str>) -> String {
    let candidate = title.unwrap_or("Untitled PDF");
    let mut output = String::new();
    let mut pending_space = false;
    for character in candidate.chars() {
        let allowed = !character.is_control() && !matches!(character, '/' | '\\' | ':' | '\0');
        if !allowed || character.is_whitespace() {
            pending_space = !output.is_empty();
            continue;
        }
        if pending_space && output.chars().count() < MAX_TITLE_CHARS {
            output.push(' ');
        }
        pending_space = false;
        if output.chars().count() >= MAX_TITLE_CHARS {
            break;
        }
        output.push(character);
    }
    let output = output.trim_matches([' ', '.']).to_owned();
    if output.is_empty() {
        "Untitled PDF".to_owned()
    } else {
        output
    }
}

fn parse_job_directory_name(name: &str) -> Option<(PrintJobId, JobState)> {
    let (id, state) = name.rsplit_once('.')?;
    let state = match state {
        "staged" => JobState::Staged,
        "claimed" => JobState::Claimed,
        _ => return None,
    };
    Some((PrintJobId::parse(id).ok()?, state))
}

fn parse_incomplete_directory_name(name: &str) -> bool {
    let Some(value) = name.strip_prefix(".stage-") else {
        return false;
    };
    let Some((id, nonce)) = value
        .as_bytes()
        .get(36)
        .filter(|byte| **byte == b'-')
        .map(|_| {
            let (id, suffix) = value.split_at(36);
            (id, &suffix[1..])
        })
    else {
        return false;
    };
    PrintJobId::parse(id).is_ok()
        && Uuid::parse_str(nonce).is_ok_and(|uuid| {
            uuid.get_version() == Some(Version::Random) && uuid.hyphenated().to_string() == nonce
        })
}

fn now_unix_ms() -> Result<u64, JobError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| JobError::InvalidMetadata)?
        .as_millis();
    u64::try_from(millis).map_err(|_| JobError::InvalidMetadata)
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

fn create_private_directory(path: &Path) -> Result<(), JobError> {
    if path.exists() {
        return validate_private_directory(path, true);
    }
    create_private_directory_new(path)
}

fn create_private_directory_new(path: &Path) -> Result<(), JobError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(path).map_err(JobError::Io)?;
    }
    #[cfg(not(unix))]
    fs::create_dir(path).map_err(JobError::Io)?;
    validate_private_directory(path, true)
}

fn validate_private_directory(path: &Path, require_owner: bool) -> Result<(), JobError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| JobError::InvalidRoot)?;
    if !metadata.is_dir() || is_reparse_or_symlink(&metadata) {
        return Err(JobError::UnsafePath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o7777 != 0o700 {
            return Err(JobError::UnsafePath);
        }
        if require_owner && metadata.uid() != unsafe { libc::geteuid() } {
            return Err(JobError::UnsafePath);
        }
    }
    #[cfg(not(unix))]
    let _ = require_owner;
    Ok(())
}

fn validate_job_directory(root: &Path, directory: &Path) -> Result<(), JobError> {
    validate_private_directory(directory, true).map_err(|error| match error {
        JobError::InvalidRoot => JobError::UnsafePath,
        other => other,
    })?;
    let canonical = fs::canonicalize(directory).map_err(|_| JobError::UnsafePath)?;
    if canonical.parent() != Some(root) || canonical != directory {
        return Err(JobError::UnsafePath);
    }
    Ok(())
}

fn create_private_file(path: &Path) -> Result<File, JobError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    options.open(path).map_err(JobError::Io)
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

fn validate_regular_single_link_file(metadata: &fs::Metadata) -> Result<(), JobError> {
    if !metadata.is_file() || is_reparse_or_symlink(metadata) {
        return Err(JobError::UnsafePath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(JobError::UnsafePath);
        }
    }
    Ok(())
}

fn validate_private_file_permissions(metadata: &fs::Metadata) -> Result<(), JobError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o7777 != 0o600
            || metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err(JobError::UnsafePath);
        }
    }
    #[cfg(not(unix))]
    let _ = metadata;
    Ok(())
}

#[cfg(windows)]
fn validate_open_file_links(file: &File) -> Result<(), JobError> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    let result =
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) };
    if result == 0 {
        return Err(JobError::Io(io::Error::last_os_error()));
    }
    let information = unsafe { information.assume_init() };
    if information.nNumberOfLinks != 1 {
        return Err(JobError::UnsafePath);
    }
    Ok(())
}

#[cfg(not(windows))]
fn validate_open_file_links(_file: &File) -> Result<(), JobError> {
    Ok(())
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

fn same_file_snapshot(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    if before.len() != after.len() {
        return false;
    }
    same_platform_file_snapshot(before, after)
}

#[cfg(unix)]
fn same_platform_file_snapshot(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

#[cfg(windows)]
fn same_platform_file_snapshot(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    before.last_write_time() == after.last_write_time()
}

#[cfg(not(any(unix, windows)))]
fn same_platform_file_snapshot(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    before.modified().ok() == after.modified().ok()
}

fn path_exists_no_follow(path: &Path) -> Result<bool, JobError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(JobError::Io(error)),
    }
}

fn remove_known_job_directory(path: &Path) -> Result<(), JobError> {
    let input = path.join(INPUT_NAME);
    let metadata = path.join(METADATA_NAME);
    validate_regular_single_link_file(&fs::symlink_metadata(&input).map_err(JobError::Io)?)?;
    validate_regular_single_link_file(&fs::symlink_metadata(&metadata).map_err(JobError::Io)?)?;
    validate_open_file_links(&open_read_no_follow(&input).map_err(JobError::Io)?)?;
    validate_open_file_links(&open_read_no_follow(&metadata).map_err(JobError::Io)?)?;
    fs::remove_file(input).map_err(JobError::Io)?;
    fs::remove_file(metadata).map_err(JobError::Io)?;
    fs::remove_dir(path).map_err(JobError::Io)
}

fn remove_known_directory(path: &Path) -> Result<(), JobError> {
    for name in [INPUT_NAME, METADATA_NAME] {
        let child = path.join(name);
        if child.exists() {
            fs::remove_file(child).map_err(JobError::Io)?;
        }
    }
    fs::remove_dir(path).map_err(JobError::Io)
}

fn remove_incomplete_directory(root: &Path, path: &Path) -> Result<(), JobError> {
    validate_job_directory(root, path)?;
    let entries = fs::read_dir(path)
        .map_err(JobError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(JobError::Io)?;
    for entry in &entries {
        let name = entry.file_name();
        if name != INPUT_NAME && name != METADATA_NAME {
            return Err(JobError::UnsafePath);
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(JobError::Io)?;
        validate_regular_single_link_file(&metadata)?;
        validate_private_file_permissions(&metadata)?;
    }
    for entry in entries {
        fs::remove_file(entry.path()).map_err(JobError::Io)?;
    }
    fs::remove_dir(path).map_err(JobError::Io)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), JobError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(JobError::Io)
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), JobError> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn rename_noreplace(from: &Path, to: &Path) -> Result<(), JobError> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(from.as_os_str().as_bytes()).map_err(|_| JobError::UnsafePath)?;
    let to = CString::new(to.as_os_str().as_bytes()).map_err(|_| JobError::UnsafePath)?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(JobError::Io(io::Error::last_os_error()))
    }
}

#[cfg(target_os = "macos")]
fn rename_noreplace(from: &Path, to: &Path) -> Result<(), JobError> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(from.as_os_str().as_bytes()).map_err(|_| JobError::UnsafePath)?;
    let to = CString::new(to.as_os_str().as_bytes()).map_err(|_| JobError::UnsafePath)?;
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(JobError::Io(io::Error::last_os_error()))
    }
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn rename_noreplace(from: &Path, to: &Path) -> Result<(), JobError> {
    if path_exists_no_follow(to)? {
        return Err(JobError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination exists",
        )));
    }
    fs::rename(from, to).map_err(JobError::Io)
}

#[cfg(windows)]
fn rename_noreplace(from: &Path, to: &Path) -> Result<(), JobError> {
    use std::os::windows::ffi::OsStrExt;
    let from = from
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let to = to
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
        )
    };
    if result != 0 {
        Ok(())
    } else {
        Err(JobError::Io(io::Error::last_os_error()))
    }
}
