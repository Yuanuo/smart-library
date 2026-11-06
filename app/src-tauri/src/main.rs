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
        // `sl`: images of the bookcase (figures/…), read from the zip
        .register_asynchronous_uri_scheme_protocol("sl", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let path = request.uri().path().trim_start_matches('/').to_string();
                let state = app.state::<AppState>();
                let response = match commands::asset(&state, &path) {
                    Some((bytes, mime)) => tauri::http::Response::builder()
                        .header(tauri::http::header::CONTENT_TYPE, mime)
                        .body(bytes),
                    None => tauri::http::Response::builder().status(404).body(Vec::new()),
                };
                responder.respond(response.unwrap());
            });
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
