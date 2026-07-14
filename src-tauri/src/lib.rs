pub mod agent;
pub mod commands;
pub mod context;
pub mod downloader;
pub mod hardware;
pub mod inference;
pub mod mcp;
pub mod model_registry;
pub mod skills;
pub mod storage;

use agent::tools::ask_user::PendingQuestions;
use commands::agent::ActivePlans;
use commands::conversations::{ActiveGenerations, InferenceState};
use commands::models::InFlightDownloads;
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

    let mut app_builder = tauri::Builder::default();

    // Registered FIRST (the plugin's own documented requirement): a second
    // process exits immediately, focusing the existing window instead.
    // Load-bearing for correctness, not just UX — the whole stack assumes
    // one process per database: `ActiveGenerations`/`PendingQuestions` are
    // in-memory, the inference engine is a per-process singleton, and
    // `storage::heal_interrupted_tool_calls`'s premise ("a trailing
    // unpaired tool_call means a dead process") is only true when no
    // *other* live process can be mid-turn against the same doce.sqlite —
    // without this guard, a second instance's startup healing would forge
    // interrupted results for the first instance's genuinely live turns.
    #[cfg(desktop)]
    {
        app_builder = app_builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.webview_windows().values().next() {
                let _ = window.set_focus();
            }
        }));
    }

    app_builder = app_builder.plugin(tauri_plugin_dialog::init());
    app_builder = app_builder.plugin(tauri_plugin_shell::init());

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
        .manage(ActivePlans::default())
        .manage(PendingQuestions::default())
        .manage(DbCell::new())
        .setup(move |app| {
            builder.mount_events(app);
            // Crash-safety backstop (Task 3.2): reap any `llama-server`
            // orphaned by a previous run before this run ever spawns its
            // own. `panic = "abort"` (Cargo.toml) skips `Drop` on a panic,
            // and llama-server doesn't exit on its own when doce's end of
            // the pipe disappears, so without this a crash can leave a
            // second full model resident once we spawn again — fatal on
            // memory-constrained hardware. Must run before any code path
            // that could call `inference::server::spawn`.
            inference::server::reap_orphan(app.handle());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running doce");
}
