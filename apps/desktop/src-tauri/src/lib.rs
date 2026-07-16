pub mod commands;
pub mod deep_link;
pub mod jobs;
pub mod state;

use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

pub fn builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            let app_data = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&app_data)?;
            let jobs = jobs::PrintJobStore::new(
                app_data.join("print-jobs"),
                std::iter::empty::<&std::path::Path>(),
            )?;
            let state = state::AppState::new(jobs, app_data.join("runtime"))?;
            app.manage(state);
            if let Some(urls) = app.deep_link().get_current()? {
                for url in urls {
                    forward_print_deep_link(app.handle(), url.as_str());
                }
            }
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    forward_print_deep_link(&handle, url.as_str());
                }
            });
            Ok(())
        })
        .invoke_handler(commands::invoke_handler())
}

pub fn forward_print_deep_link<R: tauri::Runtime>(app: &tauri::AppHandle<R>, value: &str) {
    let Ok(id) = deep_link::parse_print_deep_link(value) else {
        return;
    };
    let state = app.state::<state::AppState>();
    if state.queue_print_job(id).is_ok() {
        let _ = app.emit("print-job-requested", id.to_string());
    }
}

pub fn run() {
    builder()
        .run(tauri::generate_context!())
        .expect("MDViewer failed to start");
}
