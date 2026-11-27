use super::*;
use smart_library_core::backup::{
    create_backup, inspect_backup, stage_restore, BackupInspection, BackupManifest, BackupOptions,
    BackupResult,
};

#[tauri::command]
pub async fn backup_create(
    dest: String,
    include_secrets: bool,
    include_dicts: bool,
    state: State<'_, AppState>,
) -> Result<BackupResult, String> {
    let config_dir = state.config_dir.clone();
    let data_dir = state.data_dir.clone();
    let dest_path = PathBuf::from(dest);
    let version = display_version();
    let opts = BackupOptions {
        include_secrets,
        include_dicts,
    };
    tauri::async_runtime::spawn_blocking(move || {
        create_backup(&config_dir, &data_dir, &dest_path, &version, &opts)
    })
    .await
    .map_err(|e| format!("备份任务执行失败：{}", e))?
}

#[tauri::command]
pub async fn backup_inspect(path: String) -> Result<BackupInspection, String> {
    let archive = PathBuf::from(path);
    tauri::async_runtime::spawn_blocking(move || inspect_backup(&archive))
        .await
        .map_err(|e| format!("读取备份包失败：{}", e))?
}

#[tauri::command]
pub async fn backup_stage_restore(
    path: String,
    state: State<'_, AppState>,
) -> Result<BackupManifest, String> {
    let archive = PathBuf::from(path);
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || stage_restore(&archive, &data_dir))
        .await
        .map_err(|e| format!("准备还原失败：{}", e))?
}

#[tauri::command]
pub fn relaunch_app(app: AppHandle) {
    if let Some(st) = app.try_state::<AppState>() {
        st.save_window_state();
    }
    if let Some(mcp) = app.try_state::<crate::mcp_server::McpRuntime>() {
        mcp.stop();
    }
    app.restart();
}
