use std::path::Path;

use mdviewer_desktop_lib::macos_virtual_printer::{
    PRINTER_NAME, PRINTER_URI, VirtualPrinterPaths, helper_contents, launch_agent_contents,
    queue_uri_from_lpstat, remove_managed_spool_source,
};

#[test]
fn launch_agent_is_loopback_pdf_only_and_uses_the_managed_helper() {
    let paths = VirtualPrinterPaths::new(Path::new("/Users/example")).unwrap();
    let plist = launch_agent_contents(&paths);

    for expected in [
        "/usr/bin/ippeveprinter",
        "<string>localhost</string>",
        "<string>8631</string>",
        "<string>off</string>",
        "<string>application/pdf</string>",
        "<string>MDViewer — Guardar como Markdown</string>",
        paths.helper.to_str().unwrap(),
        paths.spool.to_str().unwrap(),
    ] {
        assert!(plist.contains(expected), "missing {expected}");
    }
    assert!(!plist.contains("0.0.0.0"));
    assert!(plist.contains("<string>-r</string><string>off</string>"));
    assert!(!plist.contains("--web-forms"));
}

#[test]
fn cleanup_removes_only_regular_files_inside_the_managed_spool() {
    let home = tempfile::tempdir().unwrap();
    let paths = VirtualPrinterPaths::new(home.path()).unwrap();
    std::fs::create_dir_all(&paths.spool).unwrap();
    let job = paths.spool.join("job-1.pdf");
    let sidecar = paths.spool.join("job-1.prn");
    std::fs::write(&job, b"%PDF-1.7\n%%EOF\n").unwrap();
    std::fs::write(&sidecar, b"job metadata").unwrap();
    assert!(remove_managed_spool_source(&job, home.path()).unwrap());
    assert!(!job.exists());
    assert!(!sidecar.exists());

    let unrelated = home.path().join("unrelated.pdf");
    std::fs::write(&unrelated, b"%PDF-1.7\n%%EOF\n").unwrap();
    assert!(!remove_managed_spool_source(&unrelated, home.path()).unwrap());
    assert!(unrelated.exists());
}

#[test]
fn helper_delivers_only_a_real_job_to_the_mdviewer_bundle() {
    let helper = helper_contents(Path::new("/Applications/MDViewer.app")).unwrap();
    assert!(helper.starts_with("#!/bin/zsh\n"));
    assert!(helper.contains("[[ $# -eq 1 && -f \"$1\" ]]"));
    assert!(helper.contains("/usr/bin/open -a '/Applications/MDViewer.app' -- \"$1\""));
    assert!(!helper.contains("open -b"));
    assert!(!helper.contains("curl"));
    assert!(!helper.contains("http"));
}

#[test]
fn helper_quotes_the_exact_installing_bundle_path() {
    let helper = helper_contents(Path::new("/Applications/Facundo's MDViewer.app")).unwrap();
    assert!(helper.contains("'/Applications/Facundo'\\''s MDViewer.app'"));
    assert!(helper_contents(Path::new("relative/MDViewer.app")).is_err());
}

#[test]
fn queue_identity_requires_the_exact_cups_name_and_uri() {
    let expected = format!("device for {PRINTER_NAME}: {PRINTER_URI}\n");
    assert_eq!(queue_uri_from_lpstat(&expected), Some(PRINTER_URI));
    assert_eq!(
        queue_uri_from_lpstat("device for MDViewer_Other: ipp://localhost:8631/ipp/print\n"),
        None
    );
    assert_eq!(
        queue_uri_from_lpstat(&format!(
            "device for {PRINTER_NAME}: ipp://example.com/ipp/print\n"
        )),
        Some("ipp://example.com/ipp/print")
    );
}

#[test]
fn paths_are_private_user_components_not_system_paths() {
    let paths = VirtualPrinterPaths::new(Path::new("/Users/example")).unwrap();
    assert!(
        paths
            .launch_agent
            .starts_with("/Users/example/Library/LaunchAgents")
    );
    assert!(
        paths
            .root
            .starts_with("/Users/example/Library/Application Support")
    );
    assert!(VirtualPrinterPaths::new(Path::new("relative")).is_err());
}
