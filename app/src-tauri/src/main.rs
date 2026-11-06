// Smart Library desktop app: the Tauri command layer.
//
// Design-spec §3: this layer only converts parameters and results to JSON;
// all the logic lives in `smart-library-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use commands::AppState;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let config_dir = app.path().app_config_dir()?;
            app.manage(AppState::new(
                config_dir.join("config.json"),
                data_dir.join("repos").join("cbeta").join("index"),
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::open_bookcase,
            commands::tree,
            commands::open_volume,
            commands::search,
            commands::build_index,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Smart Library");
}
