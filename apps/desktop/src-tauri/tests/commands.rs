use std::{fs, path::PathBuf};

use mdviewer_desktop_lib::{
    commands::{
        cancel_conversion, claim_print_job, convert_document, integration_status, invoke_handler,
        open_document, save_document, warning_codes,
    },
    deep_link::parse_print_deep_link,
    jobs::PrintJobStore,
    state::{AppState, SelectionAccess},
};

fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mdviewer-task12-command-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn state(temp: &std::path::Path) -> AppState {
    let scope = temp.join("scope");
    fs::create_dir(&scope).unwrap();
    let store = PrintJobStore::new(temp.join("jobs"), [&scope]).unwrap();
    AppState::new(store, temp.join("runtime")).unwrap()
}

#[test]
fn deep_link_parser_accepts_only_exact_print_uuid_url() {
    let id = "6ba7b810-9dad-4f11-80b4-00c04fd430c8";
    assert_eq!(
        parse_print_deep_link(&format!("mdviewer://print/{id}"))
            .unwrap()
            .to_string(),
        id
    );

    for hostile in [
        "MDVIEWER://print/6ba7b810-9dad-4f11-80b4-00c04fd430c8",
        "mdviewer://PRINT/6ba7b810-9dad-4f11-80b4-00c04fd430c8",
        "mdviewer://user@print/6ba7b810-9dad-4f11-80b4-00c04fd430c8",
        "mdviewer://print:80/6ba7b810-9dad-4f11-80b4-00c04fd430c8",
        "mdviewer://print/6BA7B810-9DAD-4F11-80B4-00C04FD430C8",
        "mdviewer://print/6ba7b810-9dad-4f11-80b4-00c04fd430c8?x=1",
        "mdviewer://print/6ba7b810-9dad-4f11-80b4-00c04fd430c8#x",
        "mdviewer://print/6ba7b810-9dad-4f11-80b4-00c04fd430c8/extra",
        "mdviewer://print/%2e%2e%2f6ba7b810-9dad-4f11-80b4-00c04fd430c8",
        "mdviewer://print/6ba7b810-9dad-4f11-80b4-00c04fd430c8%2fextra",
        "mdviewer:print/6ba7b810-9dad-4f11-80b4-00c04fd430c8",
        "https://print/6ba7b810-9dad-4f11-80b4-00c04fd430c8",
    ] {
        assert!(
            parse_print_deep_link(hostile).is_err(),
            "accepted {hostile:?}"
        );
    }
}

#[test]
fn file_commands_require_unforgeable_access_typed_selection_tokens() {
    let temp = temp_dir("tokens");
    let state = state(&temp);
    let file = temp.join("scope").join("note.md");
    fs::write(&file, "hello").unwrap();
    let read = state
        .authorize_user_selection(&file, SelectionAccess::Read)
        .unwrap();
    let write = state
        .authorize_user_selection(&file, SelectionAccess::Write)
        .unwrap();

    assert_eq!(open_document(&state, &read).unwrap().content, "hello");
    assert_eq!(
        open_document(&state, "forged/path").unwrap_err().code,
        "invalid_token"
    );
    assert_eq!(
        save_document(&state, &read, "changed").unwrap_err().code,
        "access_denied"
    );
    save_document(&state, &write, "changed").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "changed");
    fs::remove_dir_all(temp).unwrap();
}

#[cfg(unix)]
#[test]
fn save_uses_the_authorized_parent_handle_after_the_path_is_replaced() {
    let temp = temp_dir("save-parent-authority");
    let state = state(&temp);
    let destination = temp.join("destination");
    let held_destination = temp.join("destination-held");
    let outside = temp.join("outside");
    fs::create_dir(&destination).unwrap();
    fs::create_dir(&outside).unwrap();
    let selected = destination.join("note.md");
    let token = state
        .authorize_user_selection(&selected, SelectionAccess::Write)
        .unwrap();

    fs::rename(&destination, &held_destination).unwrap();
    std::os::unix::fs::symlink(&outside, &destination).unwrap();
    save_document(&state, &token, "bound to selected directory").unwrap();

    assert_eq!(
        fs::read_to_string(held_destination.join("note.md")).unwrap(),
        "bound to selected directory"
    );
    assert!(!outside.join("note.md").exists());
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn save_rejects_an_existing_target_replaced_after_authorization() {
    let temp = temp_dir("save-target-replaced");
    let state = state(&temp);
    let target = temp.join("scope").join("note.md");
    fs::write(&target, "authorized original").unwrap();
    let token = state
        .authorize_user_selection(&target, SelectionAccess::Write)
        .unwrap();
    fs::remove_file(&target).unwrap();
    fs::write(&target, "concurrent replacement").unwrap();

    let error = save_document(&state, &token, "must not overwrite replacement").unwrap_err();
    assert_eq!(error.code, "scope_changed");
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "concurrent replacement"
    );
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn save_publication_uses_atomic_compare_and_swap_primitives() {
    let source =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/state.rs")).unwrap();
    assert!(source.contains("RENAME_EXCHANGE"));
    assert!(source.contains("renameatx_np"));
    assert!(source.contains("RENAME_SWAP"));
    assert!(source.contains("ReplaceFileW"));
}

#[cfg(unix)]
#[test]
fn read_selection_rejects_a_symlink_instead_of_authorizing_its_target() {
    let temp = temp_dir("read-symlink");
    let state = state(&temp);
    let target = temp.join("scope").join("target.md");
    let link = temp.join("scope").join("link.md");
    fs::write(&target, "private target").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let error = state
        .authorize_user_selection(&link, SelectionAccess::Read)
        .unwrap_err();
    assert_eq!(error.code(), "invalid_selection");
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn convert_uses_the_cli_transaction_and_exposes_only_stable_warning_codes() {
    let temp = temp_dir("convert");
    let state = state(&temp);
    let source = temp.join("scope").join("source.html");
    let output = temp.join("scope").join("result.md");
    fs::write(&source, "<h1>Hello</h1><p>Local only.</p>").unwrap();
    let source_token = state
        .authorize_user_selection(&source, SelectionAccess::Read)
        .unwrap();
    let output_token = state
        .authorize_user_selection(&output, SelectionAccess::Write)
        .unwrap();
    let operation = uuid::Uuid::new_v4().to_string();

    let result = convert_document(&state, &operation, &source_token, &output_token).unwrap();
    assert_eq!(result.operation_id, operation);
    assert_eq!(
        result.markdown_path,
        fs::canonicalize(&output).unwrap().to_string_lossy()
    );
    assert!(fs::read_to_string(&output).unwrap().contains("# Hello"));
    assert_eq!(
        warning_codes(&state, &operation).unwrap(),
        result.warning_codes
    );
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn conversion_republishes_cli_assets_inside_the_authorized_destination_parent() {
    let temp = temp_dir("convert-assets");
    let state = state(&temp);
    let source = temp.join("scope").join("source.png");
    let output = temp.join("scope").join("image.md");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("tests/fixtures/formats/metadata.png");
    fs::copy(fixture, &source).unwrap();
    let source_token = state
        .authorize_user_selection(&source, SelectionAccess::Read)
        .unwrap();
    let output_token = state
        .authorize_user_selection(&output, SelectionAccess::Write)
        .unwrap();

    let result = convert_document(
        &state,
        &uuid::Uuid::new_v4().to_string(),
        &source_token,
        &output_token,
    )
    .unwrap();

    let assets = output.with_extension("assets");
    assert_eq!(
        result.assets_path.as_deref(),
        Some(
            fs::canonicalize(&assets)
                .unwrap()
                .to_string_lossy()
                .as_ref()
        )
    );
    assert!(assets.join("image-001.png").is_file());
    assert!(assets.join(".mdviewer-assets.json").is_file());
    assert!(
        fs::read_to_string(output)
            .unwrap()
            .contains("image.assets/image-001.png")
    );
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn conversion_rejects_a_regular_source_replaced_after_user_selection() {
    let temp = temp_dir("source-replaced");
    let state = state(&temp);
    let source = temp.join("scope").join("source.html");
    let output = temp.join("scope").join("result.md");
    fs::write(&source, "<p>Selected A</p>").unwrap();
    let source_token = state
        .authorize_user_selection(&source, SelectionAccess::Read)
        .unwrap();
    let output_token = state
        .authorize_user_selection(&output, SelectionAccess::Write)
        .unwrap();
    fs::remove_file(&source).unwrap();
    fs::write(&source, "<p>Replaced B</p>").unwrap();

    let error = convert_document(
        &state,
        &uuid::Uuid::new_v4().to_string(),
        &source_token,
        &output_token,
    )
    .unwrap_err();
    assert_eq!(error.code, "source_changed");
    assert!(!output.exists());
    fs::remove_dir_all(temp).unwrap();
}

#[cfg(unix)]
#[test]
fn conversion_uses_the_authorized_output_parent_after_path_replacement() {
    let temp = temp_dir("convert-parent-authority");
    let state = state(&temp);
    let source = temp.join("scope").join("source.html");
    let destination = temp.join("destination");
    let held_destination = temp.join("destination-held");
    let outside = temp.join("outside");
    fs::write(&source, "<h1>Stable authority</h1>").unwrap();
    fs::create_dir(&destination).unwrap();
    fs::create_dir(&outside).unwrap();
    let output = destination.join("result.md");
    let source_token = state
        .authorize_user_selection(&source, SelectionAccess::Read)
        .unwrap();
    let output_token = state
        .authorize_user_selection(&output, SelectionAccess::Write)
        .unwrap();
    fs::rename(&destination, &held_destination).unwrap();
    std::os::unix::fs::symlink(&outside, &destination).unwrap();

    convert_document(
        &state,
        &uuid::Uuid::new_v4().to_string(),
        &source_token,
        &output_token,
    )
    .unwrap();

    assert!(
        fs::read_to_string(held_destination.join("result.md"))
            .unwrap()
            .contains("# Stable authority")
    );
    assert!(!outside.join("result.md").exists());
    fs::remove_dir_all(temp).unwrap();
}

#[cfg(unix)]
#[test]
fn conversion_rejects_same_inode_mutation_and_new_hardlinks_after_selection() {
    let temp = temp_dir("source-mutated");
    let state = state(&temp);
    let source = temp.join("scope").join("source.html");
    let output = temp.join("scope").join("result.md");
    fs::write(&source, "<p>AAAA</p>").unwrap();
    let source_token = state
        .authorize_user_selection(&source, SelectionAccess::Read)
        .unwrap();
    let output_token = state
        .authorize_user_selection(&output, SelectionAccess::Write)
        .unwrap();
    fs::write(&source, "<p>BBBB</p>").unwrap();
    fs::hard_link(&source, temp.join("scope").join("alias.html")).unwrap();

    let error = convert_document(
        &state,
        &uuid::Uuid::new_v4().to_string(),
        &source_token,
        &output_token,
    )
    .unwrap_err();
    assert_eq!(error.code, "source_changed");
    assert!(!output.exists());
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn conversion_selection_tokens_are_single_use() {
    let temp = temp_dir("token-replay");
    let state = state(&temp);
    let source = temp.join("scope").join("source.html");
    let output = temp.join("scope").join("result.md");
    fs::write(&source, "<p>Once</p>").unwrap();
    let source_token = state
        .authorize_user_selection(&source, SelectionAccess::Read)
        .unwrap();
    let output_token = state
        .authorize_user_selection(&output, SelectionAccess::Write)
        .unwrap();
    convert_document(
        &state,
        &uuid::Uuid::new_v4().to_string(),
        &source_token,
        &output_token,
    )
    .unwrap();

    let error = convert_document(
        &state,
        &uuid::Uuid::new_v4().to_string(),
        &source_token,
        &output_token,
    )
    .unwrap_err();
    assert_eq!(error.code, "invalid_token");
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn cancellation_and_errors_are_typed_stable_and_redacted() {
    let temp = temp_dir("errors");
    let state = state(&temp);
    let operation = uuid::Uuid::new_v4().to_string();
    let error = cancel_conversion(&state, &operation).unwrap_err();
    assert_eq!(error.code, "conversion_not_running");
    assert!(!error.message.contains(&temp.to_string_lossy().to_string()));

    let forged = open_document(&state, "../../private-secret").unwrap_err();
    assert_eq!(
        serde_json::to_value(&forged).unwrap()["code"],
        "invalid_token"
    );
    assert!(!forged.message.contains("private-secret"));
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn async_conversion_worker_keeps_cancellation_independently_dispatchable() {
    let temp = temp_dir("async-cancel");
    let state = state(&temp);
    let source = temp.join("scope").join("large.html");
    let output = temp.join("scope").join("cancelled.md");
    let mut document = String::from("<html><body>");
    for _ in 0..1_000_000 {
        document.push_str("<p>cancellable local conversion</p>");
    }
    document.push_str("</body></html>");
    fs::write(&source, document).unwrap();
    let source_token = state
        .authorize_user_selection(&source, SelectionAccess::Read)
        .unwrap();
    let output_token = state
        .authorize_user_selection(&output, SelectionAccess::Write)
        .unwrap();
    let operation = uuid::Uuid::new_v4().to_string();

    let app = tauri::test::mock_builder()
        .manage(state)
        .invoke_handler(invoke_handler())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let convert_webview = webview.clone();
    let convert_operation = operation.clone();
    let conversion = std::thread::spawn(move || {
        tauri::test::get_ipc_response(
            &convert_webview,
            tauri::webview::InvokeRequest {
                cmd: "convert".into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(serde_json::json!({
                    "operationId": convert_operation,
                    "sourceToken": source_token,
                    "outputToken": output_token
                })),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_owned(),
            },
        )
    });
    let mut cancelled = false;
    for _ in 0..5_000 {
        let response = tauri::test::get_ipc_response(
            &webview,
            tauri::webview::InvokeRequest {
                cmd: "cancel".into(),
                callback: tauri::ipc::CallbackFn(2),
                error: tauri::ipc::CallbackFn(3),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(serde_json::json!({
                    "operationId": operation
                })),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_owned(),
            },
        );
        match response {
            Ok(_) => {
                cancelled = true;
                break;
            }
            Err(error) if error["code"] == "conversion_not_running" => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) => panic!("unexpected IPC cancellation error: {error}"),
        }
    }
    assert!(
        cancelled,
        "conversion never became independently cancellable"
    );
    let result = conversion.join().unwrap().unwrap_err();

    assert_eq!(result["code"], "cancelled");
    assert!(!output.exists());
    let source_text =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/commands.rs"))
            .unwrap();
    assert!(source_text.contains("pub(super) async fn convert("));
    assert!(source_text.contains("convert_document_async("));
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn job_claim_and_integration_status_are_local_and_minimal() {
    let temp = temp_dir("claim");
    let state = state(&temp);
    let source = temp.join("scope").join("input.pdf");
    fs::write(&source, b"%PDF-1.7\n%%EOF\n").unwrap();
    let staged = state.jobs().stage_pdf(&source, Some("Print")).unwrap();
    let claimed = claim_print_job(&state, &staged.id.to_string()).unwrap();
    assert_eq!(claimed.id, staged.id.to_string());
    assert_eq!(claimed.title, "Print");

    let status = integration_status(&state).unwrap();
    assert_eq!(status.deep_link_scheme, "mdviewer");
    assert!(status.print_jobs_available);
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn tauri_configuration_has_one_scheme_strict_csp_and_no_shell_or_fs_permissions() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest.join("tauri.conf.json")).unwrap()).unwrap();
    assert_eq!(
        config["plugins"]["deep-link"]["desktop"]["schemes"],
        serde_json::json!(["mdviewer"])
    );
    let csp = config["app"]["security"]["csp"].as_str().unwrap();
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("connect-src ipc: http://ipc.localhost"));
    assert!(csp.contains("object-src 'none'"));

    let capability = fs::read_to_string(manifest.join("capabilities/default.json")).unwrap();
    let capability_json: serde_json::Value = serde_json::from_str(&capability).unwrap();
    assert_eq!(
        capability_json["permissions"],
        serde_json::json!(["core:event:allow-listen", "core:event:allow-unlisten"])
    );
    assert!(!capability.contains("shell:"));
    assert!(!capability.contains("fs:"));
    assert!(!capability.contains("http:"));
}
