pub fn builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
}

pub fn run() {
    builder()
        .run(tauri::generate_context!())
        .expect("MDViewer failed to start");
}
