use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
};

use mdconvert_core::{
    Asset, AssetId, Block, Cancellation, ConversionWarning, Document, DocumentMetadata, Inline,
    NeverCancel, OutputError, OutputTarget, OverwritePolicy, WarningCode, publish,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "mdviewer-output-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn document_with_asset(file_name: &str, data: &[u8]) -> Document {
    let asset_id = AssetId::new("figure").expect("asset ID should be valid");
    Document {
        metadata: DocumentMetadata::default(),
        blocks: vec![
            Block::Paragraph {
                content: vec![Inline::Text("Hello".into())],
            },
            Block::Image {
                asset_id: asset_id.clone(),
                alt: "Figure".into(),
            },
        ],
        assets: vec![Asset {
            id: asset_id,
            file_name: file_name.into(),
            media_type: "image/png".into(),
            data: data.to_vec(),
        }],
        warnings: vec![ConversionWarning {
            code: WarningCode::MissingImageAlt,
            message: "preserved warning".into(),
            page: Some(1),
        }],
    }
}

fn document_without_assets() -> Document {
    Document {
        metadata: DocumentMetadata::default(),
        blocks: vec![Block::Paragraph {
            content: vec![Inline::Text("Only markdown".into())],
        }],
        assets: vec![],
        warnings: vec![],
    }
}

fn target(path: PathBuf, overwrite: OverwritePolicy) -> OutputTarget {
    OutputTarget {
        markdown_path: path,
        overwrite,
    }
}

#[test]
fn publishes_markdown_assets_and_sorted_manifest_without_mutating_document() {
    let temp = TestDir::new();
    let markdown_path = temp.path().join("foo.md");
    let mut document = document_with_asset("z.png", b"abc");
    let second_id = AssetId::new("second").expect("asset ID should be valid");
    document.assets.push(Asset {
        id: second_id,
        file_name: "a.png".into(),
        media_type: "image/png".into(),
        data: b"second".to_vec(),
    });
    let original = document.clone();

    let result = publish(
        &document,
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .expect("publication should succeed");

    assert_eq!(document, original);
    assert_eq!(result.markdown_path, markdown_path);
    assert_eq!(result.assets_dir, Some(temp.path().join("foo.assets")));
    assert_eq!(result.warnings, document.warnings);
    assert_eq!(
        fs::read_to_string(&result.markdown_path).expect("markdown should be readable"),
        "Hello\n\n![Figure](foo.assets/z.png)\n"
    );
    assert_eq!(
        fs::read(temp.path().join("foo.assets/z.png")).expect("asset should be readable"),
        b"abc"
    );

    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(temp.path().join("foo.assets/.mdviewer-assets.json"))
            .expect("manifest should be readable"),
    )
    .expect("manifest should be JSON");
    assert_eq!(manifest.as_object().unwrap().len(), 3);
    assert_eq!(manifest["schema_version"], "mdviewer.assets/v1");
    assert_eq!(manifest["document"], "foo.md");
    assert_eq!(manifest["assets"][0]["file_name"], "a.png");
    assert_eq!(manifest["assets"][1]["file_name"], "z.png");
    assert_eq!(manifest["assets"][1]["media_type"], "image/png");
    assert_eq!(manifest["assets"][1].as_object().unwrap().len(), 3);
    assert_eq!(
        manifest["assets"][1]["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn no_assets_writes_only_markdown() {
    let temp = TestDir::new();
    let markdown_path = temp.path().join("plain.md");

    let result = publish(
        &document_without_assets(),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .expect("publication should succeed");

    assert_eq!(result.assets_dir, None);
    assert_eq!(
        fs::read_to_string(markdown_path).expect("markdown should be readable"),
        "Only markdown\n"
    );
    assert!(!temp.path().join("plain.assets").exists());
}

struct CancelOnSecondCheck(AtomicUsize);

impl Cancellation for CancelOnSecondCheck {
    fn is_cancelled(&self) -> bool {
        self.0.fetch_add(1, Ordering::SeqCst) >= 1
    }
}

#[test]
fn cancellation_before_staging_leaves_no_output() {
    struct AlreadyCancelled;
    impl Cancellation for AlreadyCancelled {
        fn is_cancelled(&self) -> bool {
            true
        }
    }

    let temp = TestDir::new();
    let markdown_path = temp.path().join("cancelled.md");
    let error = publish(
        &document_with_asset("image.png", b"asset"),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &AlreadyCancelled,
    )
    .expect_err("publication should be cancelled");

    assert!(matches!(error, OutputError::Cancelled));
    assert!(!markdown_path.exists());
    assert!(!temp.path().join("cancelled.assets").exists());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn cancellation_before_commit_cleans_staging_and_leaves_no_output() {
    let temp = TestDir::new();
    let markdown_path = temp.path().join("cancelled.md");
    let cancellation = CancelOnSecondCheck(AtomicUsize::new(0));

    let error = publish(
        &document_with_asset("image.png", b"asset"),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &cancellation,
    )
    .expect_err("publication should be cancelled before commit");

    assert!(matches!(error, OutputError::Cancelled));
    assert!(!markdown_path.exists());
    assert!(!temp.path().join("cancelled.assets").exists());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn reports_an_unwritable_destination_without_leaving_staging() {
    let temp = TestDir::new();
    let parent_file = temp.path().join("not-a-directory");
    fs::write(&parent_file, b"file").unwrap();
    let markdown_path = parent_file.join("output.md");

    let error = publish(
        &document_without_assets(),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .expect_err("invalid destination should fail");

    assert!(matches!(error, OutputError::InvalidTarget(path) if path == markdown_path));
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn reports_io_error_when_destination_directory_cannot_be_written() {
    use std::os::unix::fs::PermissionsExt;

    struct RestorePermissions(PathBuf);
    impl Drop for RestorePermissions {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        }
    }

    let temp = TestDir::new();
    let destination = temp.path().join("locked");
    fs::create_dir(&destination).unwrap();
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o500)).unwrap();
    let _restore = RestorePermissions(destination.clone());
    let probe = destination.join("privilege-probe");
    if fs::write(&probe, b"probe").is_ok() {
        fs::remove_file(probe).unwrap();
        return;
    }
    let markdown_path = destination.join("output.md");

    let error = publish(
        &document_without_assets(),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .expect_err("unwritable destination should fail");

    assert!(matches!(
        error,
        OutputError::Io {
            operation: "create staging directory",
            path,
            ..
        } if path == destination
    ));
    assert!(!markdown_path.exists());
}

#[test]
fn refuses_to_replace_an_unowned_assets_directory() {
    let temp = TestDir::new();
    let assets_dir = temp.path().join("foo.assets");
    fs::create_dir(&assets_dir).unwrap();
    fs::write(assets_dir.join("personal.txt"), b"keep me").unwrap();
    let markdown_path = temp.path().join("foo.md");

    let error = publish(
        &document_with_asset("image.png", b"new"),
        &target(markdown_path, OverwritePolicy::Replace),
        &NeverCancel,
    )
    .expect_err("unowned directory should be rejected");

    assert!(matches!(error, OutputError::UnownedAssetsDirectory(path) if path == assets_dir));
    assert_eq!(
        fs::read(assets_dir.join("personal.txt")).unwrap(),
        b"keep me"
    );
}

#[test]
fn replace_accepts_owned_outputs_and_removes_stale_assets() {
    let temp = TestDir::new();
    let markdown_path = temp.path().join("foo.md");
    publish(
        &document_with_asset("old.png", b"old"),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .unwrap();

    let result = publish(
        &document_with_asset("new.png", b"new"),
        &target(markdown_path.clone(), OverwritePolicy::Replace),
        &NeverCancel,
    )
    .expect("owned output should be replaceable");

    let assets_dir = result.assets_dir.unwrap();
    assert!(!assets_dir.join("old.png").exists());
    assert_eq!(fs::read(assets_dir.join("new.png")).unwrap(), b"new");
    assert!(
        fs::read_to_string(markdown_path)
            .unwrap()
            .contains("foo.assets/new.png")
    );
}

#[test]
fn replace_with_no_assets_removes_a_previous_owned_assets_directory() {
    let temp = TestDir::new();
    let markdown_path = temp.path().join("foo.md");
    publish(
        &document_with_asset("old.png", b"old"),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .unwrap();

    let result = publish(
        &document_without_assets(),
        &target(markdown_path, OverwritePolicy::Replace),
        &NeverCancel,
    )
    .expect("owned output should be replaceable");

    assert_eq!(result.assets_dir, None);
    assert!(!temp.path().join("foo.assets").exists());
}

#[test]
fn deny_rejects_existing_markdown_or_owned_assets() {
    let temp = TestDir::new();
    let markdown_path = temp.path().join("foo.md");
    publish(
        &document_with_asset("image.png", b"old"),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .unwrap();

    let error = publish(
        &document_with_asset("image.png", b"new"),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .expect_err("deny should reject existing markdown");
    assert!(matches!(error, OutputError::OutputExists(path) if path == markdown_path));

    fs::remove_file(&markdown_path).unwrap();
    let assets_dir = temp.path().join("foo.assets");
    let error = publish(
        &document_with_asset("image.png", b"new"),
        &target(markdown_path, OverwritePolicy::Deny),
        &NeverCancel,
    )
    .expect_err("deny should reject existing assets");
    assert!(matches!(error, OutputError::OutputExists(path) if path == assets_dir));
}

#[test]
fn rejects_unsafe_or_duplicate_asset_file_names() {
    let temp = TestDir::new();
    for unsafe_name in [
        "",
        ".",
        "..",
        "../escape.png",
        "/absolute.png",
        "a/b.png",
        "a\\b.png",
        "C:drive.png",
        ".MDVIEWER-ASSETS.JSON",
        "trailing.",
        "trailing ",
        "name:stream.png",
        "control\u{7}.png",
        "CON",
        "prn.txt",
        "AuX.png",
        "nul.dat",
        "COM1.txt",
        "com9",
        "LPT1.log",
        "lpt9",
        "CONIN$.txt",
        "conout$.png",
    ] {
        let error = publish(
            &document_with_asset(unsafe_name, b"asset"),
            &target(temp.path().join("foo.md"), OverwritePolicy::Deny),
            &NeverCancel,
        )
        .expect_err("unsafe name should fail");
        assert!(matches!(error, OutputError::InvalidAssetFileName(name) if name == unsafe_name));
    }

    let mut document = document_with_asset("same.png", b"one");
    document.assets.push(Asset {
        id: AssetId::new("other").unwrap(),
        file_name: "same.png".into(),
        media_type: "image/png".into(),
        data: b"two".to_vec(),
    });
    let error = publish(
        &document,
        &target(temp.path().join("foo.md"), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .expect_err("duplicate name should fail");
    assert!(matches!(error, OutputError::DuplicateAssetFileName(name) if name == "same.png"));
}

#[test]
fn rejects_case_insensitive_and_unicode_normalized_duplicate_asset_names() {
    let temp = TestDir::new();
    for (index, (first, second)) in [
        ("Image.PNG", "image.png"),
        ("caf\u{e9}.png", "cafe\u{301}.png"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut document = document_with_asset(first, b"one");
        document.assets.push(Asset {
            id: AssetId::new(format!("other-{index}")).unwrap(),
            file_name: second.into(),
            media_type: "image/png".into(),
            data: b"two".to_vec(),
        });

        let error = publish(
            &document,
            &target(
                temp.path().join(format!("foo-{index}.md")),
                OverwritePolicy::Deny,
            ),
            &NeverCancel,
        )
        .expect_err("canonical duplicate should fail");

        assert!(matches!(error, OutputError::DuplicateAssetFileName(name) if name == second));
    }
}

#[test]
fn an_existing_per_target_lock_prevents_publication_and_is_not_removed() {
    let temp = TestDir::new();
    let markdown_path = temp.path().join("foo.md");
    let lock_path = temp.path().join(".foo.md.mdviewer.lock");
    fs::write(&lock_path, b"another transaction").unwrap();

    let error = publish(
        &document_without_assets(),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .expect_err("existing lock should prevent publication");

    assert!(matches!(error, OutputError::OutputExists(path) if path == lock_path));
    assert_eq!(fs::read(&lock_path).unwrap(), b"another transaction");
    assert!(!markdown_path.exists());
}

#[cfg(unix)]
#[test]
fn refuses_a_manifest_asset_that_is_a_symlink_instead_of_a_regular_file() {
    use std::os::unix::fs::symlink;

    let temp = TestDir::new();
    let markdown_path = temp.path().join("foo.md");
    publish(
        &document_with_asset("image.png", b"original"),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .unwrap();
    let assets_dir = temp.path().join("foo.assets");
    let asset_path = assets_dir.join("image.png");
    let external_path = temp.path().join("external.png");
    fs::write(&external_path, b"original").unwrap();
    fs::remove_file(&asset_path).unwrap();
    symlink(&external_path, &asset_path).unwrap();

    let error = publish(
        &document_with_asset("image.png", b"new"),
        &target(markdown_path, OverwritePolicy::Replace),
        &NeverCancel,
    )
    .expect_err("symlinked asset should not establish ownership");

    assert!(matches!(error, OutputError::UnownedAssetsDirectory(path) if path == assets_dir));
    assert!(
        fs::symlink_metadata(asset_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn refuses_invalid_or_tampered_owned_assets_without_modifying_them() {
    let temp = TestDir::new();
    let markdown_path = temp.path().join("foo.md");
    publish(
        &document_with_asset("image.png", b"original"),
        &target(markdown_path.clone(), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .unwrap();
    let assets_dir = temp.path().join("foo.assets");

    fs::write(assets_dir.join("image.png"), b"tampered").unwrap();
    let error = publish(
        &document_with_asset("image.png", b"new"),
        &target(markdown_path, OverwritePolicy::Replace),
        &NeverCancel,
    )
    .expect_err("tampered ownership should fail");
    assert!(matches!(error, OutputError::UnownedAssetsDirectory(path) if path == assets_dir));
    assert_eq!(fs::read(assets_dir.join("image.png")).unwrap(), b"tampered");
}

#[test]
fn reports_malformed_manifest_as_invalid() {
    let temp = TestDir::new();
    let assets_dir = temp.path().join("foo.assets");
    fs::create_dir(&assets_dir).unwrap();
    let manifest_path = assets_dir.join(".mdviewer-assets.json");
    fs::write(&manifest_path, b"not json").unwrap();

    let error = publish(
        &document_with_asset("image.png", b"new"),
        &target(temp.path().join("foo.md"), OverwritePolicy::Replace),
        &NeverCancel,
    )
    .expect_err("malformed manifest should fail");

    assert!(matches!(error, OutputError::InvalidManifest { path, .. } if path == manifest_path));
    assert_eq!(fs::read(&manifest_path).unwrap(), b"not json");
}

#[test]
fn preserves_emitter_failures_as_typed_output_errors() {
    let temp = TestDir::new();
    let mut document = document_with_asset("one.png", b"one");
    document.assets.push(Asset {
        id: AssetId::new("figure").unwrap(),
        file_name: "two.png".into(),
        media_type: "image/png".into(),
        data: b"two".to_vec(),
    });

    let error = publish(
        &document,
        &target(temp.path().join("foo.md"), OverwritePolicy::Deny),
        &NeverCancel,
    )
    .expect_err("emitter error should be preserved");

    assert!(matches!(
        error,
        OutputError::Emit(mdconvert_core::EmitError::DuplicateAssetId { asset_id })
            if asset_id == "figure"
    ));
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}
