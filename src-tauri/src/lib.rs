pub mod agent;
pub mod commands;
pub mod downloader;
pub mod hardware;
pub mod inference;
pub mod mcp;
pub mod model_registry;
pub mod scheduler;
pub mod skills;
pub mod storage;

use commands::conversations::{ActiveGenerations, InferenceState};
use commands::models::InFlightDownloads;
use scheduler::Scheduler;
use storage::DbCell;

pub fn run() {
    let builder = commands::specta_builder();

    #[cfg(debug_assertions)]
    builder
        .export(
            specta_typescript::Typescript::default(),
            "../src/lib/bindings.ts",
        )
        .expect("failed to export typescript bindings");

    #[cfg_attr(not(feature = "wdio"), allow(unused_mut))]
    let mut app_builder = tauri::Builder::default();

    #[cfg(feature = "wdio")]
    {
        app_builder = app_builder
            .plugin(tauri_plugin_wdio::init())
            .plugin(tauri_plugin_wdio_webdriver::init());
    }

    app_builder
        .invoke_handler(builder.invoke_handler())
        .manage(InferenceState::default())
        .manage(InFlightDownloads::default())
        .manage(ActiveGenerations::default())
        .manage(DbCell::new())
        .manage(Scheduler::new())
        .setup(move |app| {
            builder.mount_events(app);
            scheduler::worker::spawn(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running doce");
}
