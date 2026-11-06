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
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let config_dir = app.path().app_config_dir()?;
            app.manage(AppState::new(config_dir, data_dir));
            Ok(())
        })
        // `sl`: images of a library, `<lib>/<path in the bookcase>`
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
            commands::inspect_source,
            commands::bind_libraries,
            commands::unbind_library,
            commands::tree,
            commands::open_node,
            commands::resolve_node,
            commands::search,
            commands::quick_search,
            commands::prepare_titles,
            commands::citation_command,
            commands::build_index,
            commands::load_workspace,
            commands::save_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Smart Library");
}
