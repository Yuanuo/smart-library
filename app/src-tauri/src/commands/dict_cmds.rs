use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct DictStateDto {
    pub paths: Vec<String>,
    pub disabled: Vec<String>,
    pub dicts: Vec<DictInfo>,
}

impl AppState {
    fn dict_config_path(&self) -> PathBuf {
        self.config_dir.join("dicts.json")
    }

    fn ensure_dicts(&self) {
        if self.dicts.read().unwrap().is_some() {
            return;
        }
        let mut guard = self.dicts.write().unwrap();
        if guard.is_none() {
            let cfg = std::fs::read_to_string(self.dict_config_path())
                .ok()
                .and_then(|s| serde_json::from_str::<DictConfig>(&s).ok())
                .unwrap_or_default();
            *guard = Some(DictManager::from_config(cfg));
        }
    }

    fn dict_state_dto(&self) -> DictStateDto {
        self.ensure_dicts();
        let guard = self.dicts.read().unwrap();
        let mgr = guard.as_ref().unwrap();
        let cfg = mgr.config();
        DictStateDto {
            paths: cfg.paths.clone(),
            disabled: cfg.disabled.clone(),
            dicts: mgr.list(),
        }
    }

    fn save_dict_config(&self, cfg: &DictConfig) -> Result<(), String> {
        let _ = std::fs::create_dir_all(&self.config_dir);
        let json = serde_json::to_string_pretty(cfg)
            .map_err(|e| format!("序列化词典配置失败: {e}"))?;
        std::fs::write(self.dict_config_path(), json)
            .map_err(|e| format!("保存词典配置失败: {e}"))
    }
}

#[tauri::command]
pub async fn dict_config_get(state: State<'_, AppState>) -> Result<DictStateDto, String> {
    Ok(state.dict_state_dto())
}

#[tauri::command]
pub async fn dict_config_set(
    paths: Vec<String>,
    disabled: Vec<String>,
    state: State<'_, AppState>,
) -> Result<DictStateDto, String> {
    let cfg = DictConfig { paths, disabled };
    state.save_dict_config(&cfg)?;
    state.ensure_dicts();
    {
        let mut guard = state.dicts.write().unwrap();
        if let Some(mgr) = guard.as_mut() {
            mgr.set_config(cfg);
        }
    }
    Ok(state.dict_state_dto())
}

#[tauri::command]
pub async fn dict_reload(state: State<'_, AppState>) -> Result<DictStateDto, String> {
    state.ensure_dicts();
    {
        let mut guard = state.dicts.write().unwrap();
        if let Some(mgr) = guard.as_mut() {
            mgr.reload();
        }
    }
    Ok(state.dict_state_dto())
}

#[tauri::command]
pub async fn dict_search(
    query: String,
    scope_ids: Option<Vec<String>>,
    offset: Option<usize>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<DictSearchResult, String> {
    state.ensure_dicts();
    let guard = state.dicts.read().unwrap();
    let mgr = guard.as_ref().unwrap();
    Ok(mgr.search(
        &query,
        scope_ids.as_deref(),
        offset.unwrap_or(0),
        limit.unwrap_or(50),
    ))
}

#[tauri::command]
pub async fn dict_entry(
    dict_id: String,
    entry_id: u32,
    state: State<'_, AppState>,
) -> Result<DictEntryDetail, String> {
    state.ensure_dicts();
    let guard = state.dicts.read().unwrap();
    let mgr = guard.as_ref().unwrap();
    mgr.get_entry(&dict_id, entry_id)
}

#[tauri::command]
pub async fn dict_lookup(
    word: String,
    scope_ids: Option<Vec<String>>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<DictEntryDetail>, String> {
    state.ensure_dicts();
    let guard = state.dicts.read().unwrap();
    let mgr = guard.as_ref().unwrap();
    Ok(mgr.lookup_exact(&word, scope_ids.as_deref(), limit.unwrap_or(50)))
}
