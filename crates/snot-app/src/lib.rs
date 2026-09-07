//! The Snot application shell.
//!
//! Everything here is glue: it owns the library on disk, exposes `snot-core`
//! to the webview as commands, and nothing more. Behaviour that a future CLI
//! or sync daemon would also want belongs in `snot-core`, not here.

mod commands;

use std::sync::Mutex;

use snot_core::Store;
use tauri::Manager;

pub struct AppState {
    pub store: Mutex<Store>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            // One library per install, in the platform's app-data directory.
            // Overridable so a test run or a second profile can point elsewhere.
            let root = match std::env::var_os("SNOT_LIBRARY") {
                Some(path) => std::path::PathBuf::from(path),
                None => app.path().app_data_dir()?.join("library"),
            };
            std::fs::create_dir_all(&root)?;
            // Attachments are served to the webview over the asset protocol,
            // which is scoped to the app-data directory by default. The
            // library can live anywhere, so grant its own directory too.
            app.asset_protocol_scope().allow_directory(&root, true)?;
            let store = Store::open(&root)?;
            // A first launch opens onto a welcome note rather than an empty
            // window that explains nothing. SNOT_DEMO (or the demo-library
            // feature, for mobile test builds) swaps that for a full worked
            // example instead — for testing, never for a real user.
            let demo = cfg!(feature = "demo-library") || std::env::var_os("SNOT_DEMO").is_some();
            store.seed_if_new(demo)?;
            app.manage(AppState {
                store: Mutex::new(store),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library_root,
            commands::stats,
            commands::list_folders,
            commands::create_folder,
            commands::rename_folder,
            commands::set_folder_color,
            commands::move_folder,
            commands::delete_folder,
            commands::list_notes,
            commands::search_notes,
            commands::get_note,
            commands::create_note,
            commands::update_note,
            commands::trash_note,
            commands::restore_note,
            commands::purge_note,
            commands::empty_trash,
            commands::list_tags,
            commands::ensure_tag,
            commands::delete_tag,
            commands::put_attachment,
            commands::attachment_path,
            commands::import_pdf,
            commands::export_markdown,
            commands::write_text_file,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Snot");
}
