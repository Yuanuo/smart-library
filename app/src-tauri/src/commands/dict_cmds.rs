use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct DictStateDto {
    pub paths: Vec<String>,
    pub disabled: Vec<String>,
    pub dicts: Vec<DictInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictImportOutcomeDto {
    pub source: String,
    pub name: String,
    pub path: String,
    pub format: String,
    pub entries: usize,
    pub variants: usize,
    pub skipped: usize,
    pub has_extra: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictImportFailureDto {
    pub source: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictImportResultDto {
    pub imported: Vec<DictImportOutcomeDto>,
    pub failed: Vec<DictImportFailureDto>,
    pub state: DictStateDto,
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

/// Import TXT / StarDict / MDX dictionaries into `<data>/dicts` and add that
/// directory to the dictionary paths. The manager is dropped first so the
/// Windows file mappings of old `.words` files do not block replacing them.
#[tauri::command]
pub async fn dict_import(
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<DictImportResultDto, String> {
    if paths.is_empty() {
        state.ensure_dicts();
        return Ok(DictImportResultDto {
            imported: Vec::new(),
            failed: Vec::new(),
            state: state.dict_state_dto(),
        });
    }
    let out_dir = state.data_dir.join("dicts");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("创建词典目录失败 {}: {e}", out_dir.display()))?;
    {
        let mut guard = state.dicts.write().unwrap();
        *guard = None;
    }
    let sources = paths.clone();
    let out = out_dir.clone();
    let (imported, failed) = tauri::async_runtime::spawn_blocking(move || {
        let mut imported = Vec::new();
        let mut failed = Vec::new();
        for source in sources {
            match smart_library_core::dict_import::import_path(std::path::Path::new(&source), &out) {
                Ok(outcomes) => imported.extend(outcomes),
                Err(error) => failed.push(DictImportFailureDto { source, error }),
            }
        }
        (imported, failed)
    })
    .await
    .map_err(|e| format!("导入任务失败: {e}"))?;

    if !imported.is_empty() {
        let mut config = std::fs::read_to_string(state.dict_config_path())
            .ok()
            .and_then(|s| serde_json::from_str::<DictConfig>(&s).ok())
            .unwrap_or_default();
        let dir = out_dir.to_string_lossy().into_owned();
        if !config.paths.iter().any(|p| p == &dir) {
            config.paths.push(dir);
        }
        state.save_dict_config(&config)?;
    }
    state.ensure_dicts();
    Ok(DictImportResultDto {
        imported: imported
            .into_iter()
            .map(|o| DictImportOutcomeDto {
                source: o.source,
                name: o.name,
                path: o.output,
                format: o.format.to_string(),
                entries: o.entries,
                variants: o.variants,
                skipped: o.skipped,
                has_extra: o.has_extra,
                warnings: o.warnings,
            })
            .collect(),
        failed,
        state: state.dict_state_dto(),
    })
}
