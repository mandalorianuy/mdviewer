use std::{
    ffi::OsString,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::ffi::OsStr;

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
    #[cfg(windows)]
    root_directory: Arc<File>,
    authorized_scopes: Vec<AuthorizedScope>,
}

#[derive(Debug, Clone)]
struct AuthorizedScope {
    path: PathBuf,
    directory: Arc<File>,
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
        #[cfg(windows)]
        let root_directory = Arc::new(open_job_root_directory(&root).map_err(JobError::Io)?);

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
            let directory = open_scope_directory(&canonical).map_err(|_| JobError::UnsafePath)?;
            let opened_metadata = directory.metadata().map_err(|_| JobError::UnsafePath)?;
            if !opened_metadata.is_dir() || is_reparse_or_symlink(&opened_metadata) {
                return Err(JobError::UnsafePath);
            }
            scopes.push(AuthorizedScope {
                path: canonical,
                directory: Arc::new(directory),
            });
        }
        scopes.sort_by(|left, right| left.path.cmp(&right.path));
        scopes.dedup_by(|left, right| left.path == right.path);

        Ok(Self {
            root,
            #[cfg(windows)]
            root_directory,
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
        let temporary_name = OsString::from(format!(".stage-{id}-{nonce}"));
        #[cfg(not(windows))]
        let temporary = self.root.join(&temporary_name);
        let final_path = self.job_path(id, JobState::Staged);
        #[cfg(windows)]
        let temporary_directory =
            create_private_windows_directory_relative(&self.root_directory, &temporary_name)?;
        #[cfg(not(windows))]
        create_private_directory_new(&temporary)?;

        let result = (|| {
            #[cfg(not(windows))]
            let input_pdf = temporary.join(INPUT_NAME);
            #[cfg(windows)]
            let mut staged =
                create_private_windows_file_relative(&temporary_directory, OsStr::new(INPUT_NAME))?;
            #[cfg(not(windows))]
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
            #[cfg(windows)]
            let mut metadata_file = create_private_windows_file_relative(
                &temporary_directory,
                OsStr::new(METADATA_NAME),
            )?;
            #[cfg(not(windows))]
            let mut metadata_file = create_private_file(&temporary.join(METADATA_NAME))?;
            metadata_file.write_all(&bytes).map_err(JobError::Io)?;
            metadata_file.sync_all().map_err(JobError::Io)?;
            #[cfg(windows)]
            {
                temporary_directory.sync_all().map_err(JobError::Io)?;
                crate::state::windows_rename_open_handle(
                    &temporary_directory,
                    &self.root_directory,
                    final_path.file_name().ok_or(JobError::UnsafePath)?,
                )
                .map_err(JobError::Io)?;
                self.root_directory.sync_all().map_err(JobError::Io)?;
            }
            #[cfg(not(windows))]
            {
                sync_directory(&temporary)?;
                rename_noreplace(&temporary, &final_path)?;
                sync_directory(&self.root)?;
            }
            self.load_job(id, JobState::Staged)
        })();

        #[cfg(windows)]
        {
            if result.is_err() {
                let _ = remove_known_windows_directory(&temporary_directory);
            }
            drop(temporary_directory);
        }
        #[cfg(not(windows))]
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
        #[cfg(windows)]
        {
            let staged_directory = crate::state::windows_open_relative_for_mutation(
                &self.root_directory,
                staged.file_name().ok_or(JobError::UnsafePath)?,
            )
            .map_err(JobError::Io)?;
            crate::state::windows_rename_open_handle(
                &staged_directory,
                &self.root_directory,
                claimed.file_name().ok_or(JobError::UnsafePath)?,
            )
            .map_err(JobError::Io)?;
            self.root_directory.sync_all().map_err(JobError::Io)?;
        }
        #[cfg(not(windows))]
        {
            rename_noreplace(&staged, &claimed)?;
            sync_directory(&self.root)?;
        }
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
        let parent = source.parent().ok_or(JobError::UnauthorizedSource)?;
        let file_name = source.file_name().ok_or(JobError::UnauthorizedSource)?;
        let normalized = fs::canonicalize(parent)
            .map_err(|_| JobError::UnauthorizedSource)?
            .join(file_name);
        let scope_and_relative = self
            .authorized_scopes
            .iter()
            .find_map(|scope| {
                normalized
                    .strip_prefix(&scope.path)
                    .ok()
                    .map(|relative| (scope, relative))
            })
            .filter(|(_, relative)| {
                relative.components().next().is_some()
                    && relative
                        .components()
                        .all(|component| matches!(component, Component::Normal(_)))
            })
            .ok_or(JobError::UnauthorizedSource)?;
        open_source_beneath(scope_and_relative.0, scope_and_relative.1)
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
        #[cfg(windows)]
        let directory_handle = crate::state::windows_open_relative_for_mutation(
            &self.root_directory,
            directory.file_name().ok_or(JobError::UnsafePath)?,
        )
        .map_err(|_| JobError::UnsafePath)?;
        #[cfg(windows)]
        {
            let metadata = directory_handle.metadata().map_err(JobError::Io)?;
            if !metadata.is_dir() || is_reparse_or_symlink(&metadata) {
                return Err(JobError::UnsafePath);
            }
            validate_private_windows_security_handle(&directory_handle, true)
                .map_err(|_| JobError::UnsafePath)?;
        }
        #[cfg(not(windows))]
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
        #[cfg(windows)]
        let mut input =
            open_private_windows_file_relative(&directory_handle, OsStr::new(INPUT_NAME)).map_err(
                |error| match error {
                    JobError::Io(error) if error.kind() == io::ErrorKind::NotFound => {
                        JobError::MissingInput
                    }
                    other => other,
                },
            )?;
        #[cfg(windows)]
        let input_metadata = input.metadata().map_err(JobError::Io)?;
        #[cfg(not(windows))]
        let input_metadata = fs::symlink_metadata(&input_pdf).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                JobError::MissingInput
            } else {
                JobError::Io(error)
            }
        })?;
        validate_regular_single_link_file(&input_metadata)?;
        #[cfg(not(windows))]
        validate_private_file_permissions(&input_pdf, &input_metadata)?;
        #[cfg(not(windows))]
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
        #[cfg(windows)]
        let metadata_file =
            open_private_windows_file_relative(&directory_handle, OsStr::new(METADATA_NAME))
                .map_err(|_| JobError::InvalidMetadata)?;
        #[cfg(windows)]
        let file_metadata = metadata_file
            .metadata()
            .map_err(|_| JobError::InvalidMetadata)?;
        #[cfg(not(windows))]
        let file_metadata =
            fs::symlink_metadata(&metadata_path).map_err(|_| JobError::InvalidMetadata)?;
        validate_regular_single_link_file(&file_metadata).map_err(|_| JobError::InvalidMetadata)?;
        #[cfg(not(windows))]
        validate_private_file_permissions(&metadata_path, &file_metadata)
            .map_err(|_| JobError::InvalidMetadata)?;
        if file_metadata.len() > MAX_METADATA_BYTES {
            return Err(JobError::InvalidMetadata);
        }
        #[cfg(not(windows))]
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

#[cfg(windows)]
struct OwnedSid {
    storage: Vec<usize>,
}

#[cfg(windows)]
impl OwnedSid {
    fn as_ptr(&self) -> windows_sys::Win32::Security::PSID {
        self.storage.as_ptr().cast_mut().cast()
    }
}

#[cfg(windows)]
fn current_user_sid() -> Result<OwnedSid, JobError> {
    use std::{mem::size_of, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token: HANDLE = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(JobError::Io(io::Error::last_os_error()));
    }
    let result = (|| {
        let mut needed = 0_u32;
        unsafe {
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
        }
        if needed == 0 {
            return Err(JobError::Io(io::Error::last_os_error()));
        }
        let words = (needed as usize).div_ceil(size_of::<usize>());
        let mut token_information = vec![0_usize; words];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                token_information.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        } == 0
        {
            return Err(JobError::Io(io::Error::last_os_error()));
        }
        let user = unsafe { &*token_information.as_ptr().cast::<TOKEN_USER>() };
        let sid_length = unsafe { GetLengthSid(user.User.Sid) } as usize;
        if sid_length == 0 {
            return Err(JobError::Io(io::Error::last_os_error()));
        }
        let mut storage = vec![0_usize; sid_length.div_ceil(size_of::<usize>())];
        unsafe {
            ptr::copy_nonoverlapping(
                user.User.Sid.cast::<u8>(),
                storage.as_mut_ptr().cast::<u8>(),
                sid_length,
            );
        }
        Ok(OwnedSid { storage })
    })();
    unsafe {
        CloseHandle(token);
    }
    result
}

#[cfg(windows)]
pub(crate) fn apply_private_windows_security(path: &Path, directory: bool) -> Result<(), JobError> {
    let file = open_windows_security_handle(path, directory, true)?;
    apply_private_windows_security_to_handle(&file, directory)
}

#[cfg(windows)]
fn open_windows_security_handle(
    path: &Path,
    directory: bool,
    write_security: bool,
) -> Result<File, JobError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        FILE_SHARE_WRITE, READ_CONTROL, WRITE_DAC, WRITE_OWNER,
    };
    let mut options = OpenOptions::new();
    let mut access = READ_CONTROL;
    if write_security {
        access |= WRITE_DAC | WRITE_OWNER;
    }
    options
        .access_mode(access)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    if directory {
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    }
    options.open(path).map_err(JobError::Io)
}

#[cfg(windows)]
pub(crate) fn apply_private_windows_security_to_handle(
    file: &File,
    directory: bool,
) -> Result<(), JobError> {
    use std::{mem::size_of, os::windows::io::AsRawHandle};
    use windows_sys::Win32::{
        Security::{
            ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAceEx, CONTAINER_INHERIT_ACE,
            DACL_SECURITY_INFORMATION, InitializeAcl, InitializeSecurityDescriptor,
            OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
            SECURITY_DESCRIPTOR, SetKernelObjectSecurity, SetSecurityDescriptorDacl,
            SetSecurityDescriptorOwner,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
        System::SystemServices::SECURITY_DESCRIPTOR_REVISION,
    };

    let sid = current_user_sid()?;
    let sid_length = unsafe { windows_sys::Win32::Security::GetLengthSid(sid.as_ptr()) } as usize;
    let acl_length =
        size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>() + sid_length;
    let mut acl_storage = vec![0_usize; acl_length.div_ceil(size_of::<usize>())];
    let acl = acl_storage.as_mut_ptr().cast::<ACL>();
    if unsafe { InitializeAcl(acl, acl_length as u32, ACL_REVISION) } == 0 {
        return Err(JobError::Io(io::Error::last_os_error()));
    }
    let ace_flags = if directory {
        CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE
    } else {
        0
    };
    if unsafe { AddAccessAllowedAceEx(acl, ACL_REVISION, ace_flags, FILE_ALL_ACCESS, sid.as_ptr()) }
        == 0
    {
        return Err(JobError::Io(io::Error::last_os_error()));
    }

    let mut descriptor = SECURITY_DESCRIPTOR::default();
    if unsafe {
        InitializeSecurityDescriptor((&raw mut descriptor).cast(), SECURITY_DESCRIPTOR_REVISION)
    } == 0
        || unsafe { SetSecurityDescriptorOwner((&raw mut descriptor).cast(), sid.as_ptr(), 0) } == 0
        || unsafe { SetSecurityDescriptorDacl((&raw mut descriptor).cast(), 1, acl, 0) } == 0
    {
        return Err(JobError::Io(io::Error::last_os_error()));
    }
    let information = OWNER_SECURITY_INFORMATION
        | DACL_SECURITY_INFORMATION
        | PROTECTED_DACL_SECURITY_INFORMATION;
    if unsafe {
        SetKernelObjectSecurity(
            file.as_raw_handle().cast(),
            information,
            (&raw mut descriptor).cast(),
        )
    } == 0
    {
        return Err(JobError::Io(io::Error::last_os_error()));
    }
    validate_private_windows_security_handle(file, directory)
}

#[cfg(windows)]
pub(crate) fn validate_private_windows_security(
    path: &Path,
    directory: bool,
) -> Result<(), JobError> {
    let file = open_windows_security_handle(path, directory, false)?;
    validate_private_windows_security_handle(&file, directory)
}

#[cfg(windows)]
pub(crate) fn validate_private_windows_security_handle(
    file: &File,
    directory: bool,
) -> Result<(), JobError> {
    use std::{mem::size_of, os::windows::io::AsRawHandle, ptr};
    use windows_sys::Win32::{
        Security::{
            ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
            CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation,
            GetKernelObjectSecurity, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
            GetSecurityDescriptorOwner, OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION, PSID,
            SE_DACL_PROTECTED,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
        System::SystemServices::ACCESS_ALLOWED_ACE_TYPE,
    };

    let sid = current_user_sid()?;
    let information = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut needed = 0_u32;
    unsafe {
        GetKernelObjectSecurity(
            file.as_raw_handle().cast(),
            information,
            ptr::null_mut(),
            0,
            &mut needed,
        );
    }
    if needed == 0 {
        return Err(JobError::Io(io::Error::last_os_error()));
    }
    let mut descriptor_storage = vec![0_usize; (needed as usize).div_ceil(size_of::<usize>())];
    let descriptor = descriptor_storage.as_mut_ptr().cast();
    if unsafe {
        GetKernelObjectSecurity(
            file.as_raw_handle().cast(),
            information,
            descriptor,
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(JobError::Io(io::Error::last_os_error()));
    }

    let mut owner: PSID = ptr::null_mut();
    let mut owner_defaulted = 0;
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut dacl_present = 0;
    let mut dacl_defaulted = 0;
    let mut control = 0_u16;
    let mut revision = 0_u32;
    if unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut owner_defaulted) } == 0
        || owner.is_null()
        || unsafe { EqualSid(owner, sid.as_ptr()) } == 0
        || unsafe {
            GetSecurityDescriptorDacl(
                descriptor,
                &mut dacl_present,
                &mut dacl,
                &mut dacl_defaulted,
            )
        } == 0
        || dacl_present == 0
        || dacl.is_null()
        || unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0
        || control & SE_DACL_PROTECTED == 0
    {
        return Err(JobError::UnsafePath);
    }

    let mut acl_information = ACL_SIZE_INFORMATION::default();
    if unsafe {
        GetAclInformation(
            dacl,
            (&raw mut acl_information).cast(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
        || acl_information.AceCount != 1
    {
        return Err(JobError::UnsafePath);
    }
    let mut raw_ace = ptr::null_mut();
    if unsafe { GetAce(dacl, 0, &mut raw_ace) } == 0 || raw_ace.is_null() {
        return Err(JobError::UnsafePath);
    }
    let ace = raw_ace.cast::<ACCESS_ALLOWED_ACE>();
    let expected_flags = if directory {
        (CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE) as u8
    } else {
        0
    };
    let ace_sid = unsafe { (&raw mut (*ace).SidStart).cast() };
    if unsafe { (*ace).Header.AceType } != ACCESS_ALLOWED_ACE_TYPE as u8
        || unsafe { (*ace).Header.AceFlags } != expected_flags
        || unsafe { (*ace).Mask } != FILE_ALL_ACCESS
        || unsafe { EqualSid(ace_sid, sid.as_ptr()) } == 0
    {
        return Err(JobError::UnsafePath);
    }
    Ok(())
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
    #[cfg(windows)]
    apply_private_windows_security(path, true)?;
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
    #[cfg(windows)]
    validate_private_windows_security(path, true)?;
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

#[cfg(not(windows))]
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
    let file = options.open(path).map_err(JobError::Io)?;
    #[cfg(windows)]
    apply_private_windows_security(path, false)?;
    Ok(file)
}

#[cfg(windows)]
fn open_job_root_directory(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
        FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
    };
    OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(windows)]
fn create_private_windows_directory_relative(
    parent: &File,
    name: &OsStr,
) -> Result<File, JobError> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_CREATE,
        Win32::Storage::FileSystem::{
            DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, READ_CONTROL, SYNCHRONIZE, WRITE_DAC,
            WRITE_OWNER,
        },
    };
    let directory = crate::state::windows_nt_open_relative(
        parent,
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
    .map_err(JobError::Io)?;
    if let Err(error) = apply_private_windows_security_to_handle(&directory, true) {
        let _ = crate::state::windows_delete_open_handle(&directory);
        return Err(error);
    }
    Ok(directory)
}

#[cfg(windows)]
fn create_private_windows_file_relative(parent: &File, name: &OsStr) -> Result<File, JobError> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_CREATE,
        Win32::Storage::FileSystem::{
            DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, READ_CONTROL, SYNCHRONIZE, WRITE_DAC,
            WRITE_OWNER,
        },
    };
    let file = crate::state::windows_nt_open_relative(
        parent,
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
    .map_err(JobError::Io)?;
    if let Err(error) = apply_private_windows_security_to_handle(&file, false) {
        let _ = crate::state::windows_delete_open_handle(&file);
        return Err(error);
    }
    Ok(file)
}

#[cfg(windows)]
fn open_private_windows_file_relative(parent: &File, name: &OsStr) -> Result<File, JobError> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_OPEN,
        Win32::Storage::FileSystem::{FILE_GENERIC_READ, SYNCHRONIZE},
    };
    let file = crate::state::windows_nt_open_relative(
        parent,
        name,
        FILE_GENERIC_READ | SYNCHRONIZE,
        FILE_OPEN,
        Some(false),
    )
    .map_err(JobError::Io)?;
    let metadata = file.metadata().map_err(JobError::Io)?;
    if !metadata.is_file() || is_reparse_or_symlink(&metadata) {
        return Err(JobError::UnsafePath);
    }
    validate_private_windows_security_handle(&file, false)?;
    Ok(file)
}

#[cfg(windows)]
fn remove_known_windows_directory(directory: &File) -> Result<(), JobError> {
    for name in [INPUT_NAME, METADATA_NAME] {
        match crate::state::windows_open_relative_for_mutation(directory, OsStr::new(name)) {
            Ok(file) => crate::state::windows_delete_open_handle(&file).map_err(JobError::Io)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(JobError::Io(error)),
        }
    }
    crate::state::windows_delete_open_handle(directory).map_err(JobError::Io)
}

fn open_scope_directory(path: &Path) -> io::Result<File> {
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
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        options
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    options.open(path)
}

#[cfg(unix)]
fn open_source_beneath(scope: &AuthorizedScope, relative: &Path) -> Result<File, JobError> {
    use std::{
        ffi::CString,
        os::fd::{AsRawFd, FromRawFd},
        os::unix::ffi::OsStrExt,
    };
    let scope_metadata = scope.directory.metadata().map_err(JobError::Io)?;
    if !scope_metadata.is_dir() || is_reparse_or_symlink(&scope_metadata) {
        return Err(JobError::UnsafePath);
    }
    let components = relative
        .components()
        .map(|component| match component {
            Component::Normal(name) => {
                CString::new(name.as_bytes()).map_err(|_| JobError::UnsafePath)
            }
            _ => Err(JobError::UnsafePath),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut opened_directories = Vec::<File>::new();
    for (index, component) in components.iter().enumerate() {
        let parent_descriptor = opened_directories
            .last()
            .map_or_else(|| scope.directory.as_raw_fd(), AsRawFd::as_raw_fd);
        let final_component = index + 1 == components.len();
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if final_component {
                0
            } else {
                libc::O_DIRECTORY
            };
        let descriptor = unsafe { libc::openat(parent_descriptor, component.as_ptr(), flags) };
        if descriptor < 0 {
            let error = io::Error::last_os_error();
            return Err(match error.kind() {
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
                    JobError::UnauthorizedSource
                }
                _ => JobError::UnsafePath,
            });
        }
        let opened = unsafe { File::from_raw_fd(descriptor) };
        let metadata = opened.metadata().map_err(JobError::Io)?;
        if final_component {
            if !metadata.is_file() || is_reparse_or_symlink(&metadata) {
                return Err(JobError::UnsafePath);
            }
            return Ok(opened);
        }
        if !metadata.is_dir() || is_reparse_or_symlink(&metadata) {
            return Err(JobError::UnsafePath);
        }
        opened_directories.push(opened);
    }
    Err(JobError::UnauthorizedSource)
}

#[cfg(windows)]
fn open_source_beneath(scope: &AuthorizedScope, relative: &Path) -> Result<File, JobError> {
    use windows_sys::{
        Wdk::Storage::FileSystem::FILE_OPEN,
        Win32::Storage::FileSystem::{FILE_GENERIC_READ, SYNCHRONIZE},
    };
    let scope_metadata = scope.directory.metadata().map_err(JobError::Io)?;
    if !scope_metadata.is_dir() || is_reparse_or_symlink(&scope_metadata) {
        return Err(JobError::UnsafePath);
    }
    let components = relative
        .components()
        .map(|component| match component {
            Component::Normal(name) => Ok(name.to_os_string()),
            _ => Err(JobError::UnsafePath),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut opened_directories = Vec::<File>::new();
    for (index, component) in components.iter().enumerate() {
        let final_component = index + 1 == components.len();
        let parent = opened_directories.last().unwrap_or(&scope.directory);
        let opened = crate::state::windows_nt_open_relative(
            parent,
            component,
            FILE_GENERIC_READ | SYNCHRONIZE,
            FILE_OPEN,
            Some(!final_component),
        )
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
                JobError::UnauthorizedSource
            }
            _ => JobError::Io(error),
        })?;
        let metadata = opened.metadata().map_err(JobError::Io)?;
        if is_reparse_or_symlink(&metadata) {
            return Err(JobError::UnsafePath);
        }
        if final_component {
            if !metadata.is_file() {
                return Err(JobError::UnsafePath);
            }
            return Ok(opened);
        }
        if !metadata.is_dir() {
            return Err(JobError::UnsafePath);
        }
        opened_directories.push(opened);
    }
    Err(JobError::UnauthorizedSource)
}

#[cfg(not(any(unix, windows)))]
fn open_source_beneath(scope: &AuthorizedScope, relative: &Path) -> Result<File, JobError> {
    open_read_no_follow(&scope.path.join(relative)).map_err(JobError::Io)
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

fn validate_private_file_permissions(path: &Path, metadata: &fs::Metadata) -> Result<(), JobError> {
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
    #[cfg(windows)]
    validate_private_windows_security(path, false)?;
    #[cfg(not(windows))]
    let _ = path;
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

#[cfg(not(windows))]
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
        validate_private_file_permissions(&entry.path(), &metadata)?;
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
