use std::{fs, path::PathBuf};

use mdviewer_desktop_lib::{
    commands::{
        authorize_open_selection, authorize_save_selection, cancel_conversion, claim_print_job,
        convert_document, finish_print_job, integration_status, invoke_handler, open_document,
        sanitized_markdown_name, save_document, validate_external_url, warning_codes,
    },
    deep_link::parse_print_deep_link,
    forward_print_deep_link,
    jobs::PrintJobStore,
    state::{AppState, SelectionAccess},
};
use tauri::Listener;

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
fn native_save_name_is_bounded_on_unicode_grapheme_boundaries() {
    use unicode_segmentation::UnicodeSegmentation;

    let family = "👨‍👩‍👧‍👦";
    let suggested = format!("{}{}.MD", "Información ".repeat(30), family.repeat(30));
    let name = sanitized_markdown_name(&suggested);

    assert!(name.to_lowercase().ends_with(".md"));
    assert!(!name.to_lowercase().ends_with(".md.md"));
    assert!(name.graphemes(true).count() <= 120);
    assert!(name.is_char_boundary(name.len()));
}

#[test]
fn mock_runtime_dispatches_ipc_deep_links_events_and_window_lifecycle() {
    let temp = temp_dir("mock-runtime-lifecycle");
    let app_state = state(&temp);
    let source = temp.join("scope").join("input.pdf");
    fs::write(&source, b"%PDF-1.7\n%%EOF\n").unwrap();
    let staged = app_state
        .jobs()
        .stage_pdf(&source, Some("Runtime job"))
        .unwrap();
    let job_id = staged.id.to_string();

    let app = tauri::test::mock_builder()
        .manage(app_state)
        .invoke_handler(invoke_handler())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let invoke = |cmd: &str, body: serde_json::Value, callback: u32| {
        tauri::test::get_ipc_response(
            &webview,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(callback),
                error: tauri::ipc::CallbackFn(callback + 1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_owned(),
            },
        )
        .map(|body| body.deserialize::<serde_json::Value>().unwrap())
    };

    let (event_tx, event_rx) = std::sync::mpsc::channel();
    app.listen("print-job-requested", move |event| {
        event_tx.send(event.payload().to_owned()).unwrap();
    });
    forward_print_deep_link(app.handle(), &format!("mdviewer://print/{job_id}"));
    let payload = event_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("deep link did not emit print-job-requested");
    assert_eq!(serde_json::from_str::<String>(&payload).unwrap(), job_id);

    forward_print_deep_link(app.handle(), "mdviewer://print/not-a-uuid");
    assert!(
        event_rx
            .recv_timeout(std::time::Duration::from_millis(20))
            .is_err()
    );

    let status = invoke("integration_status", serde_json::json!({}), 10).unwrap();
    assert_eq!(status["deep_link_scheme"], "mdviewer");
    assert_eq!(status["pending_print_job_ids"], serde_json::json!([job_id]));

    let claimed = invoke("claim_print_job", serde_json::json!({ "id": job_id }), 12).unwrap();
    assert_eq!(claimed["title"], "Runtime job");
    assert!(claimed["source_token"].as_str().is_some());
    assert!(claimed.get("input_pdf").is_none());
    let claimed_id = claimed["id"].as_str().unwrap();
    invoke(
        "finish_print_job",
        serde_json::json!({ "id": claimed_id }),
        14,
    )
    .unwrap();

    webview.show().unwrap();
    webview.set_focus().unwrap();
    let (lifecycle_tx, lifecycle_rx) = std::sync::mpsc::channel();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let close_window = webview.clone();
    let closer = std::thread::spawn(move || {
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("mock runtime did not become ready");
        close_window.close().unwrap();
    });
    app.run(move |_handle, event| {
        let marker = match event {
            tauri::RunEvent::Ready => {
                ready_tx.send(()).unwrap();
                Some("ready")
            }
            tauri::RunEvent::WindowEvent {
                event: tauri::WindowEvent::CloseRequested { .. },
                ..
            } => Some("close-requested"),
            tauri::RunEvent::ExitRequested { .. } => Some("exit-requested"),
            tauri::RunEvent::Exit => Some("exit"),
            _ => None,
        };
        if let Some(marker) = marker {
            lifecycle_tx.send(marker).unwrap();
        }
    });
    closer.join().unwrap();
    let lifecycle = lifecycle_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(
        lifecycle,
        ["ready", "close-requested", "exit-requested", "exit"]
    );
    fs::remove_dir_all(temp).unwrap();
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
    let saved = save_document(&state, &write, "changed").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "changed");
    save_document(&state, &saved.write_token, "changed again").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "changed again");
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
    assert!(source.contains("NtCreateFile"));
    assert!(source.contains("RootDirectory"));
    assert!(source.contains("SetFileInformationByHandle"));
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
    assert_eq!(
        open_document(&state, &claimed.source_token)
            .unwrap()
            .content,
        "%PDF-1.7\n%%EOF\n"
    );
    finish_print_job(&state, &claimed.id).unwrap();
    assert_eq!(
        finish_print_job(&state, &claimed.id).unwrap_err().code,
        "job_not_found"
    );

    let status = integration_status(&state).unwrap();
    assert_eq!(status.deep_link_scheme, "mdviewer");
    assert!(status.print_jobs_available);
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn native_selection_helpers_return_names_and_opaque_capabilities_only() {
    let temp = temp_dir("native-selections");
    let state = state(&temp);
    let markdown = temp.join("scope").join("notes.md");
    let output = temp.join("scope").join("copy.md");
    fs::write(&markdown, "# Notes").unwrap();

    let selected = authorize_open_selection(&state, &markdown, true).unwrap();
    assert_eq!(selected.name, "notes.md");
    assert!(!selected.read_token.contains(std::path::MAIN_SEPARATOR));
    assert!(selected.write_token.is_some());
    assert_eq!(
        open_document(&state, &selected.read_token).unwrap().content,
        "# Notes"
    );

    let destination = authorize_save_selection(&state, &output).unwrap();
    assert_eq!(destination.name, "copy.md");
    assert!(!destination.write_token.contains(std::path::MAIN_SEPARATOR));
    save_document(&state, &destination.write_token, "saved").unwrap();
    assert_eq!(fs::read_to_string(output).unwrap(), "saved");
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn converted_result_can_be_reopened_only_through_its_new_read_capability() {
    let temp = temp_dir("converted-token");
    let state = state(&temp);
    let source = temp.join("scope").join("source.html");
    let output = temp.join("scope").join("result.md");
    fs::write(&source, "<h1>Token result</h1>").unwrap();
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
    assert!(
        open_document(&state, &result.markdown_token)
            .unwrap()
            .content
            .contains("# Token result")
    );
    let wire = serde_json::to_value(&result).unwrap();
    assert!(wire.get("markdown_token").is_some());
    assert!(wire.get("write_token").is_some());
    assert!(wire.get("markdown_path").is_none());
    assert!(wire.get("assets_path").is_none());
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn external_url_allowlist_rejects_active_and_ambiguous_schemes() {
    assert_eq!(
        validate_external_url("https://example.com/path?q=local#section")
            .unwrap()
            .as_str(),
        "https://example.com/path?q=local#section"
    );
    for hostile in [
        "javascript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "file:///etc/passwd",
        "https://user:secret@example.com/",
        "https:///missing-host",
        "http://127.0.0.1@evil.example/",
    ] {
        assert!(
            validate_external_url(hostile).is_err(),
            "accepted {hostile:?}"
        );
    }
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
        serde_json::json!([
            "core:event:allow-listen",
            "core:event:allow-unlisten",
            "core:window:allow-close",
            "core:window:allow-set-focus",
            "core:window:allow-show"
        ])
    );
    assert!(!capability.contains("shell:"));
    assert!(!capability.contains("fs:"));
    assert!(!capability.contains("http:"));
}
