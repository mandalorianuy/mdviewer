use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use tempfile::Builder;
use thiserror::Error;

use crate::{
    ConversionWarning, Document, EmitError, GfmOptions, emit_gfm,
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
    publish_with_renamer(document, target, cancellation, &StdRenamer)
}

fn publish_with_renamer(
    document: &Document,
    target: &OutputTarget,
    cancellation: &dyn Cancellation,
    renamer: &dyn Renamer,
) -> Result<WriteResult, OutputError> {
    if cancellation.is_cancelled() {
        return Err(OutputError::Cancelled);
    }

    let paths = PublicationPaths::new(&target.markdown_path)?;
    validate_asset_file_names(document)?;
    let existing_assets = inspect_existing_assets(&paths)?;
    inspect_existing_markdown(&paths.markdown_path)?;

    if target.overwrite == OverwritePolicy::Deny {
        if paths.markdown_path.exists() {
            return Err(OutputError::OutputExists(paths.markdown_path));
        }
        if existing_assets {
            return Err(OutputError::OutputExists(paths.assets_dir));
        }
    }

    let mut rendered_document = document.clone();
    for asset in &mut rendered_document.assets {
        asset.file_name = format!("{}/{}", paths.assets_name, asset.file_name);
    }
    let markdown = emit_gfm(
        &rendered_document,
        &GfmOptions {
            final_newline: true,
        },
    )?;

    let staging = Builder::new()
        .prefix(".mdviewer-output-")
        .tempdir_in(&paths.parent)
        .map_err(|source| io_error("create staging directory", &paths.parent, source))?;
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
        sync_directory(&directory)?;
        Some(directory)
    };
    sync_directory(staging.path())?;

    if cancellation.is_cancelled() {
        return Err(OutputError::Cancelled);
    }

    commit(
        &paths,
        staging.path(),
        &staged_markdown,
        staged_assets.as_deref(),
        existing_assets,
        renamer,
    )?;

    Ok(WriteResult {
        markdown_path: target.markdown_path.clone(),
        assets_dir: (!document.assets.is_empty()).then_some(paths.assets_dir),
        warnings: document.warnings.clone(),
    })
}

trait Renamer {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
}

struct StdRenamer;

impl Renamer for StdRenamer {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
}

struct PublicationPaths {
    markdown_path: PathBuf,
    assets_dir: PathBuf,
    parent: PathBuf,
    document_name: String,
    assets_name: String,
}

impl PublicationPaths {
    fn new(markdown_path: &Path) -> Result<Self, OutputError> {
        let document_name = markdown_path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| OutputError::InvalidTarget(markdown_path.to_owned()))?
            .to_owned();
        let parent = markdown_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_owned();
        if !parent.is_dir() {
            return Err(OutputError::InvalidTarget(markdown_path.to_owned()));
        }
        let assets_dir = markdown_path.with_extension("assets");
        let assets_name = assets_dir
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| OutputError::InvalidTarget(markdown_path.to_owned()))?
            .to_owned();

        Ok(Self {
            markdown_path: markdown_path.to_owned(),
            assets_dir,
            parent,
            document_name,
            assets_name,
        })
    }
}

fn validate_asset_file_names(document: &Document) -> Result<(), OutputError> {
    let mut names = HashSet::with_capacity(document.assets.len());
    for asset in &document.assets {
        if !is_safe_basename(&asset.file_name) {
            return Err(OutputError::InvalidAssetFileName(asset.file_name.clone()));
        }
        if !names.insert(asset.file_name.as_str()) {
            return Err(OutputError::DuplicateAssetFileName(asset.file_name.clone()));
        }
    }
    Ok(())
}

fn is_safe_basename(name: &str) -> bool {
    let bytes = name.as_bytes();
    let has_windows_drive_prefix =
        bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || has_windows_drive_prefix
    {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn inspect_existing_markdown(path: &Path) -> Result<(), OutputError> {
    if path.exists() && !path.is_file() {
        return Err(OutputError::InvalidTarget(path.to_owned()));
    }
    Ok(())
}

fn inspect_existing_assets(paths: &PublicationPaths) -> Result<bool, OutputError> {
    if !paths.assets_dir.exists() {
        return Ok(false);
    }
    let directory_metadata = fs::symlink_metadata(&paths.assets_dir)
        .map_err(|source| io_error("inspect assets directory", &paths.assets_dir, source))?;
    if !directory_metadata.is_dir() {
        return Err(OutputError::UnownedAssetsDirectory(
            paths.assets_dir.clone(),
        ));
    }

    let manifest_path = paths.assets_dir.join(MANIFEST_FILE_NAME);
    match fs::symlink_metadata(&manifest_path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            return Err(OutputError::UnownedAssetsDirectory(
                paths.assets_dir.clone(),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(OutputError::UnownedAssetsDirectory(
                paths.assets_dir.clone(),
            ));
        }
        Err(source) => return Err(io_error("inspect assets manifest", &manifest_path, source)),
    }
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(OutputError::UnownedAssetsDirectory(
                paths.assets_dir.clone(),
            ));
        }
        Err(source) => return Err(io_error("read assets manifest", &manifest_path, source)),
    };
    let manifest: AssetManifest =
        serde_json::from_slice(&bytes).map_err(|error| OutputError::InvalidManifest {
            path: manifest_path.clone(),
            message: error.to_string(),
        })?;
    validate_manifest_structure(&manifest, paths, &manifest_path)?;

    let mut expected_entries = HashSet::with_capacity(manifest.assets.len() + 1);
    expected_entries.insert(MANIFEST_FILE_NAME.to_owned());
    for asset in &manifest.assets {
        expected_entries.insert(asset.file_name.clone());
        let asset_path = paths.assets_dir.join(&asset.file_name);
        let metadata = match fs::symlink_metadata(&asset_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(OutputError::UnownedAssetsDirectory(
                    paths.assets_dir.clone(),
                ));
            }
            Err(source) => return Err(io_error("inspect owned asset", &asset_path, source)),
        };
        if !metadata.is_file() {
            return Err(OutputError::UnownedAssetsDirectory(
                paths.assets_dir.clone(),
            ));
        }
        let data = fs::read(&asset_path)
            .map_err(|source| io_error("read owned asset", &asset_path, source))?;
        if sha256_hex(&data) != asset.sha256 {
            return Err(OutputError::UnownedAssetsDirectory(
                paths.assets_dir.clone(),
            ));
        }
    }

    for entry in fs::read_dir(&paths.assets_dir)
        .map_err(|source| io_error("list assets directory", &paths.assets_dir, source))?
    {
        let entry = entry
            .map_err(|source| io_error("read assets directory entry", &paths.assets_dir, source))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            return Err(OutputError::UnownedAssetsDirectory(
                paths.assets_dir.clone(),
            ));
        };
        if !expected_entries.remove(&name) {
            return Err(OutputError::UnownedAssetsDirectory(
                paths.assets_dir.clone(),
            ));
        }
    }
    if !expected_entries.is_empty() {
        return Err(OutputError::UnownedAssetsDirectory(
            paths.assets_dir.clone(),
        ));
    }

    Ok(true)
}

fn validate_manifest_structure(
    manifest: &AssetManifest,
    paths: &PublicationPaths,
    manifest_path: &Path,
) -> Result<(), OutputError> {
    if manifest.schema_version != SCHEMA_VERSION {
        return invalid_manifest(manifest_path, "schema version does not match");
    }
    if manifest.document != paths.document_name {
        return invalid_manifest(manifest_path, "document name does not match");
    }
    let mut names = HashSet::with_capacity(manifest.assets.len());
    for asset in &manifest.assets {
        if !is_safe_basename(&asset.file_name) {
            return invalid_manifest(manifest_path, "asset file name is unsafe");
        }
        if !names.insert(asset.file_name.as_str()) {
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

fn sync_directory(path: &Path) -> Result<(), OutputError> {
    #[cfg(windows)]
    {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};

        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        return OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| io_error("fsync directory", path, source));
    }

    #[cfg(not(windows))]
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| io_error("fsync directory", path, source))
}

fn commit(
    paths: &PublicationPaths,
    staging: &Path,
    staged_markdown: &Path,
    staged_assets: Option<&Path>,
    existing_assets: bool,
    renamer: &dyn Renamer,
) -> Result<(), OutputError> {
    let backup_markdown = staging.join("previous.md");
    let backup_assets = staging.join("previous.assets");
    let had_markdown = paths.markdown_path.exists();

    if had_markdown {
        renamer
            .rename(&paths.markdown_path, &backup_markdown)
            .map_err(|source| {
                io_error("back up existing Markdown", &paths.markdown_path, source)
            })?;
    }
    if existing_assets && let Err(source) = renamer.rename(&paths.assets_dir, &backup_assets) {
        if had_markdown {
            renamer
                .rename(&backup_markdown, &paths.markdown_path)
                .map_err(|restore_error| OutputError::TransactionFailed {
                    message: format!(
                        "assets backup failed ({source}); could not restore Markdown: {restore_error}"
                    ),
                })?;
        }
        return Err(io_error(
            "back up existing assets directory",
            &paths.assets_dir,
            source,
        ));
    }

    if let Some(staged_assets) = staged_assets
        && let Err(source) = renamer.rename(staged_assets, &paths.assets_dir)
    {
        restore_backups(
            paths,
            &backup_markdown,
            &backup_assets,
            had_markdown,
            existing_assets,
            renamer,
        )?;
        return Err(io_error(
            "install assets directory",
            &paths.assets_dir,
            source,
        ));
    }

    if let Err(source) = renamer.rename(staged_markdown, &paths.markdown_path) {
        let mut rollback_failures = Vec::new();
        if staged_assets.is_some() {
            let failed_new_assets = staging.join("failed-new.assets");
            if let Err(rename_error) = renamer.rename(&paths.assets_dir, &failed_new_assets)
                && let Err(remove_error) = fs::remove_dir_all(&paths.assets_dir)
            {
                rollback_failures.push(format!(
                    "remove new assets (rename: {rename_error}; delete: {remove_error})"
                ));
            }
        }
        if let Err(rollback_error) = restore_backups(
            paths,
            &backup_markdown,
            &backup_assets,
            had_markdown,
            existing_assets,
            renamer,
        ) {
            rollback_failures.push(rollback_error.to_string());
        }
        if !rollback_failures.is_empty() {
            return Err(OutputError::TransactionFailed {
                message: format!(
                    "Markdown install failed ({source}); rollback failed: {}",
                    rollback_failures.join("; ")
                ),
            });
        }
        return Err(io_error("install Markdown", &paths.markdown_path, source));
    }

    sync_directory(&paths.parent)
}

fn restore_backups(
    paths: &PublicationPaths,
    backup_markdown: &Path,
    backup_assets: &Path,
    had_markdown: bool,
    had_assets: bool,
    renamer: &dyn Renamer,
) -> Result<(), OutputError> {
    let mut failures = Vec::new();
    if had_assets && let Err(error) = renamer.rename(backup_assets, &paths.assets_dir) {
        failures.push(format!("restore assets: {error}"));
    }
    if had_markdown && let Err(error) = renamer.rename(backup_markdown, &paths.markdown_path) {
        failures.push(format!("restore Markdown: {error}"));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(OutputError::TransactionFailed {
            message: failures.join("; "),
        })
    }
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
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::{Asset, AssetId, Block, DocumentMetadata};

    struct FailFinalMarkdownRename(AtomicBool);

    impl Renamer for FailFinalMarkdownRename {
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if from.file_name().is_some_and(|name| name == "new.md")
                && !self.0.swap(true, Ordering::SeqCst)
            {
                return Err(io::Error::other("injected final Markdown rename failure"));
            }
            fs::rename(from, to)
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

    #[test]
    fn final_markdown_rename_failure_removes_new_assets_and_restores_both_outputs() {
        let directory = tempfile::tempdir().unwrap();
        let markdown_path = directory.path().join("foo.md");
        let target = OutputTarget {
            markdown_path: markdown_path.clone(),
            overwrite: OverwritePolicy::Replace,
        };
        publish(&document("old.png", b"old"), &target, &NeverCancel).unwrap();
        let prior_markdown = fs::read(&markdown_path).unwrap();

        let error = publish_with_renamer(
            &document("new.png", b"new"),
            &target,
            &NeverCancel,
            &FailFinalMarkdownRename(AtomicBool::new(false)),
        )
        .expect_err("injected final rename should fail");

        assert!(matches!(
            error,
            OutputError::Io {
                operation: "install Markdown",
                ..
            }
        ));
        assert_eq!(fs::read(&markdown_path).unwrap(), prior_markdown);
        assert_eq!(
            fs::read(directory.path().join("foo.assets/old.png")).unwrap(),
            b"old"
        );
        assert!(!directory.path().join("foo.assets/new.png").exists());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}
