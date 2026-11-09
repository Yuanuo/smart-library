// Smart Library desktop app: the Tauri command layer.
//
// Design-spec §3: this layer only converts parameters and results to JSON;
// all the logic lives in `smart-library-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod fonts;

use commands::AppState;
use tauri::Manager;

/// Where the data lives. Portable: a `data` folder next to the program (used
/// when it exists, and always in release builds), so the whole folder can be
/// copied or carried on a stick. Development builds use the system's app
/// folders unless a `data` folder was made next to the executable. Returns
/// `(config dir, data dir)`.
fn data_dirs() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let exe = std::env::current_exe().ok()?;
    let root = exe.parent()?.join("data");
    if root.is_dir() || !cfg!(debug_assertions) {
        std::fs::create_dir_all(&root).ok()?;
        return Some((root.join("config"), root.clone()));
    }
    None
}

fn main() {
    let portable = data_dirs();
    if let Some((_, data)) = &portable {
        // WebView2 keeps its own profile (local storage …): keep it here too
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", data.join("webview"));
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // closing the main window ends the program (also closes the pop-out
        // readers and panel windows)
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::Destroyed = event {
                    window.app_handle().exit(0);
                }
            }
        })
        .setup(|app| {
            let (config_dir, data_dir) = match portable {
                Some(dirs) => dirs,
                None => (app.path().app_config_dir()?, app.path().app_data_dir()?),
            };
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
            commands::app_info,
            commands::list_fonts,
            commands::find_in_page,
            commands::inspect_source,
            commands::inspect_folder,
            commands::add_folder,
            commands::set_folder_writable,
            commands::create_folder_file,
            commands::create_folder_dir,
            commands::move_folder_node,
            commands::folder_dirs,
            commands::sidecar_refresh,
            commands::clear_cache,
            commands::delete_folder_dir,
            commands::check_file_path,
            commands::file_name_rules,
            commands::check_file_name,
            commands::folder_children,
            commands::create_notes_library,
            commands::create_note,
            commands::mark_add,
            commands::mark_update,
            commands::mark_delete,
            commands::mark_list,
            commands::mark_tags,
            commands::props_get,
            commands::props_set,
            commands::node_tags_get,
            commands::node_tags_set,
            commands::cite_text,
            commands::recent_touch,
            commands::recent_progress,
            commands::recent_list,
            commands::recent_remove,
            commands::opened_set,
            commands::recent_clear,
            commands::note_targets,
            commands::create_note_at,
            commands::diary_today,
            commands::note_chain,
            commands::rename_library,
            commands::rename_note,
            commands::save_note,
            commands::note_append,
            commands::save_bytes,
            commands::note_picture,
            commands::mindmap_add_clip,
            commands::note_attach,
            commands::export_library,
            commands::import_package,
            commands::delete_note,
            commands::move_note,
            commands::bind_libraries,
            commands::unbind_library,
            commands::tree,
            commands::open_node,
            commands::resolve_node,
            commands::search,
            commands::random_sentence,
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
