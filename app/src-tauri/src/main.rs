// Smart Library desktop app: the Tauri command layer.
//
// Design-spec §3: this layer only converts parameters and results to JSON;
// all the logic lives in `smart-library-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod ai_tools;
mod commands;
mod fonts;
mod mcp_server;

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
    // Only auto-create a portable `data` folder next to the executable in Windows release builds;
    // macOS .app bundles and Linux system/AppImage paths are read-only or signed.
    if root.is_dir() || (cfg!(windows) && !cfg!(debug_assertions)) {
        std::fs::create_dir_all(&root).ok()?;
        return Some((root.join("config"), root.clone()));
    }
    None
}

fn main() {
    let portable = data_dirs();
    #[cfg(windows)]
    if let Some((_, data)) = &portable {
        // WebView2 keeps its own profile (local storage …): keep it here too
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", data.join("webview"));
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // closing the main window ends the program (also closes the pop-out
        // readers and panel windows)
        .on_window_event(|window, event| {
            if window.label() == "main" {
                match event {
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                        if !window.is_minimized().unwrap_or(false) {
                            if let Some(st) = window.app_handle().try_state::<AppState>() {
                                let maximized = window.is_maximized().unwrap_or(false);
                                if maximized {
                                    st.set_window_maximized(true);
                                } else if let (Ok(pos), Ok(size)) = (window.outer_position(), window.inner_size()) {
                                    st.update_window_state(
                                        pos.x,
                                        pos.y,
                                        size.width,
                                        size.height,
                                        false,
                                    );
                                }
                            }
                        }
                    }
                    tauri::WindowEvent::CloseRequested { .. } => {
                        if let Some(st) = window.app_handle().try_state::<AppState>() {
                            st.save_window_state();
                        }
                    }
                    tauri::WindowEvent::Destroyed => {
                        if let Some(st) = window.app_handle().try_state::<AppState>() {
                            st.save_window_state();
                        }
                        if let Some(mcp) = window.app_handle().try_state::<mcp_server::McpRuntime>() {
                            mcp.stop();
                        }
                        window.app_handle().exit(0);
                    }
                    _ => {}
                }
            }
        })
        .setup(|app| {
            let (config_dir, data_dir) = match portable {
                Some(dirs) => dirs,
                None => (app.path().app_config_dir()?, app.path().app_data_dir()?),
            };
            if let Err(e) = smart_library_core::backup::apply_pending_restore(&config_dir, &data_dir) {
                eprintln!("failed to apply pending restore: {e}");
            }
            let state = AppState::new(config_dir.clone(), data_dir);
            if let Some(main_win) = app.get_webview_window("main") {
                if let Some(ws) = state.saved_window_state() {
                    if ws.width >= 400 && ws.height >= 300 {
                        let visible_on_monitor = main_win
                            .available_monitors()
                            .map(|mons| {
                                mons.iter().any(|m| {
                                    let mp = m.position();
                                    let ms = m.size();
                                    let mx2 = mp.x + ms.width as i32;
                                    let my2 = mp.y + ms.height as i32;
                                    let wx2 = ws.x + ws.width as i32;
                                    let wy2 = ws.y + ws.height as i32;
                                    wx2 > mp.x + 80 && ws.x < mx2 - 80 && wy2 > mp.y + 40 && ws.y < my2 - 40
                                })
                            })
                            .unwrap_or(false);
                        if visible_on_monitor {
                            let _ = main_win.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(ws.width, ws.height)));
                            let _ = main_win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(ws.x, ws.y)));
                        }
                    }
                    if ws.maximized {
                        let _ = main_win.maximize();
                    }
                }
                let _ = main_win.show();
                let _ = main_win.set_focus();
            }
            app.manage(state);
            let mcp = mcp_server::init_and_start(app.handle(), &config_dir);
            app.manage(mcp);
            // pictures no note has referred to for 24 hours: look every few hours
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(120));
                let st = handle.state::<AppState>();
                st.sweep_all_pictures();
                st.sweep_expired_snapshots();
                std::thread::sleep(std::time::Duration::from_secs(6 * 3600));
            });
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
            commands::set_folder_filter,
            commands::relink_library,
            commands::add_zim,
            commands::inspect_zim,
            commands::cancel_bind,
            commands::set_folder_writable,
            commands::create_folder_file,
            commands::create_folder_dir,
            commands::move_folder_node,
            commands::folder_dirs,
            commands::sidecar_refresh,
            commands::clean_unused_pictures,
            commands::empty_folder_trash,
            commands::dynasty_table,
            commands::read_text_file,
            commands::t2s_pairs,
            commands::dynasty_text,
            commands::set_dynasty_text,
            commands::delete_folder_dir,
            commands::trash_list,
            commands::trash_restore,
            commands::check_file_path,
            commands::file_name_rules,
            commands::check_file_name,
            commands::folder_children,
            commands::folder_directory,
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
            commands::recent_bump,
            commands::recent_progress,
            commands::recent_patch_extra,
            commands::recent_list,
            commands::recent_remove,
            commands::opened_set,
            commands::recent_clear,
            commands::note_targets,
            commands::create_note_at,
            commands::diary_today,
            commands::note_chain,
            commands::rename_library,
            commands::reorder_libraries,
            commands::rename_note,
            commands::save_note,
            commands::note_append,
            commands::save_bytes,
            commands::note_picture,
            commands::mindmap_add_clips,
            commands::note_attach,
            commands::export_library,
            commands::inspect_package,
            commands::import_package,
            commands::import_dir_as_folder,
            commands::git_check,
            commands::git_default_dir,
            commands::git_inspect_dir,
            commands::add_git_library,
            commands::git_status_all,
            commands::git_pull_library,
            commands::delete_note,
            commands::note_move_targets,
            commands::move_note,
            commands::bind_libraries,
            commands::unbind_library,
            commands::tree,
            commands::open_node,
            commands::folder_file_stamp,
            commands::raw_source,
            commands::resolve_node,
            commands::resolve_zim_link,
            commands::open_external_url,
            commands::open_external_window,
            commands::open_in_file_manager,
            commands::search,
            commands::random_sentence,
            commands::quick_search,
            commands::prepare_titles,
            commands::resolve_book_refs,
            commands::sibling_kepans,
            commands::to_pinyin,
            commands::pinyin_convert,
            commands::pinyin_text,
            commands::set_pinyin_text,
            commands::citation_command,
            commands::build_index,
            commands::index_report,
            commands::check_folder_changes,
            commands::load_workspace,
            commands::save_workspace,
            commands::load_settings,
            commands::save_settings,
            commands::ai_stream,
            commands::ai_abort,
            commands::ai_tool_definitions,
            commands::ai_exec_tool,
            commands::ai_test,
            commands::ai_fetch_models,
            commands::ai_summarize_title,
            commands::ai_summarize_note_title,
            commands::ai_mindmap_parse,
            commands::ai_doc_text,
            commands::ai_session_save,
            commands::ai_session_get,
            commands::ai_session_latest,
            commands::ai_session_list,
            commands::ai_session_delete,
            commands::ai_session_rename,
            commands::ai_session_clear,
            commands::dict_config_get,
            commands::dict_config_set,
            commands::dict_reload,
            commands::dict_search,
            commands::dict_entry,
            commands::dict_lookup,
            commands::dict_import,
            commands::list_snapshots,
            commands::create_snapshot,
            commands::get_snapshot,
            commands::restore_snapshot,
            commands::delete_snapshot,
            commands::list_deleted_notes,
            commands::restore_deleted_note,
            commands::purge_deleted_notes,
            commands::get_snapshot_policy,
            commands::set_snapshot_policy,
            mcp_server::mcp_status_get,
            mcp_server::mcp_config_set,
            commands::backup_create,
            commands::backup_inspect,
            commands::backup_stage_restore,
            commands::relaunch_app,
            commands::relaunch_after_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Smart Library");
}
