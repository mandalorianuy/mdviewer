use std::{fs, path::PathBuf};

use mdviewer_desktop_lib::{
    commands::{
        cancel_conversion, claim_print_job, convert_document, integration_status, open_document,
        save_document, warning_codes,
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
