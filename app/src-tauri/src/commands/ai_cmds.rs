use super::*;

#[derive(Serialize)]
pub struct AiDocDto {
    pub lib: String,
    pub lib_title: String,
    pub book_id: Option<String>,
    pub path: String,
    pub title: String,
    pub content_type: String,
    pub text: String,
}

#[derive(Serialize)]
pub struct AiSessionDto {
    pub id: i64,
    pub scope_kind: String,
    pub lib: String,
    pub path: String,
    pub book_id: Option<String>,
    pub title: String,
    pub doc_title: String,
    pub messages_json: String,
    pub refs_json: String,
    pub updated: i64,
}

#[derive(Serialize)]
pub struct AiSessionSummaryDto {
    pub id: i64,
    pub scope_kind: String,
    pub lib: String,
    pub path: String,
    pub book_id: Option<String>,
    pub title: String,
    pub doc_title: String,
    pub updated: i64,
}

#[tauri::command]
pub async fn ai_stream(
    req_id: String,
    config: crate::ai::AiModelConfig,
    system: Option<String>,
    messages: Vec<crate::ai::AiMessage>,
    reasoning_effort: Option<String>,
    tools: Option<Vec<serde_json::Value>>,
    app: AppHandle,
) -> Result<(), String> {
    crate::ai::stream_chat(app, req_id, config, system, messages, reasoning_effort, tools).await
}

#[tauri::command]
pub fn ai_tool_definitions() -> Vec<serde_json::Value> {
    crate::ai_tools::tool_definitions()
}

#[tauri::command]
pub async fn ai_exec_tool(
    name: String,
    arguments: String,
    state: State<'_, AppState>,
) -> Result<crate::ai_tools::AiToolResult, String> {
    Ok(crate::ai_tools::exec_tool(&name, &arguments, state).await)
}

#[tauri::command]
pub fn ai_abort(req_id: String) {
    crate::ai::abort(&req_id);
}

#[tauri::command]
pub async fn ai_test(config: crate::ai::AiModelConfig) -> Result<String, String> {
    crate::ai::test_connection(config).await
}

#[tauri::command]
pub async fn ai_fetch_models(
    format: String,
    base_url: String,
    api_key: String,
) -> Result<Vec<String>, String> {
    crate::ai::fetch_models(format, base_url, api_key).await
}

#[tauri::command]
pub async fn ai_summarize_title(
    config: crate::ai::AiModelConfig,
    user_text: String,
    assistant_text: String,
) -> Result<String, String> {
    crate::ai::summarize_title(config, user_text, assistant_text).await
}

#[tauri::command]
pub fn ai_mindmap_parse(input: String, fallback_title: Option<String>) -> Result<String, String> {
    smart_library_core::mindmap::from_outline_or_json(&input, fallback_title.as_deref().unwrap_or("脑图"))
}

/// Read the plain text (or Markdown representation) of a document so it can be
/// referenced in an AI conversation without modifying the document.
#[tauri::command]
pub async fn ai_doc_text(
    lib: String,
    book_id: Option<String>,
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<AiDocDto, String> {
    let l = state.library(&lib)?;
    if l.kind == "zim" {
        let path = path.ok_or("没有指定文章")?;
        let z = state.zim(&l)?;
        let mut z = z.lock().unwrap();
        let i = z.entry_index(&path).ok_or("ZIM 文章不存在")?;
        let title = z.entries[i].title.clone();
        let plain = z.article(i, None)?;
        return Ok(AiDocDto {
            lib: l.id,
            lib_title: l.title,
            book_id: None,
            path,
            title,
            content_type: "html".into(),
            text: plain.text.text,
        });
    }
    if l.kind == "notes" {
        let store = state.notes(&l)?;
        let path = path.ok_or("没有指定笔记")?;
        let id: i64 = path.parse().map_err(|_| format!("不是笔记：{}", path))?;
        let note = store.get(id)?.ok_or("笔记不存在")?;
        let text = if note.content_type == smart_library_core::mindmap::TYPE {
            smart_library_core::mindmap::to_markdown(&note.content)
        } else if note.content_type == smart_library_core::whiteboard::TYPE {
            smart_library_core::whiteboard::to_markdown(&note.content)
        } else {
            note.content.clone()
        };
        return Ok(AiDocDto {
            lib: l.id,
            lib_title: l.title,
            book_id: None,
            path,
            title: note.title,
            content_type: if note.content_type.is_empty() { "markdown".into() } else { note.content_type },
            text,
        });
    }
    if l.kind == "folder" {
        let f = state.folder(&l)?;
        let path = path.ok_or("没有指定文件")?;
        let name = path.rsplit('/').next().unwrap_or(&path);
        let title = folder::title_of(name).to_string();
        let edit_kind = folder::EditKind::of(name);
        if matches!(edit_kind, Some(folder::EditKind::MindMap) | Some(folder::EditKind::Whiteboard)) {
            let (raw, _) = f.read_edit(&path)?;
            let text = if edit_kind == Some(folder::EditKind::MindMap) {
                smart_library_core::mindmap::to_markdown(&raw)
            } else {
                smart_library_core::whiteboard::to_markdown(&raw)
            };
            return Ok(AiDocDto {
                lib: l.id,
                lib_title: l.title,
                book_id: None,
                path,
                title,
                content_type: edit_kind.unwrap().content_type().into(),
                text,
            });
        }
        let plain = f.article(&path, None)?;
        return Ok(AiDocDto {
            lib: l.id,
            lib_title: l.title,
            book_id: None,
            path,
            title,
            content_type: "text".into(),
            text: plain.text.text,
        });
    }
    let bc = state.bookcase_of(&l)?;
    if l.kind == "html" {
        let t = state.html_tree(&l)?;
        let path = path.ok_or("没有指定文章")?;
        let leaf = t.leaf(&path).ok_or_else(|| format!("目录里没有这篇文章：{}", path))?;
        let plain = bc.article(&path, None)?;
        return Ok(AiDocDto {
            lib: l.id,
            lib_title: l.title,
            book_id: None,
            path,
            title: leaf.title.clone(),
            content_type: "html".into(),
            text: plain.text.text,
        });
    }
    let book_id = book_id.ok_or("没有指定经号")?;
    let book = bc.book(&book_id).ok_or_else(|| format!("没有这部经：{}", book_id))?;
    let vols = bc.volumes(&book_id);
    let vol = match &path {
        Some(p) if !p.is_empty() => vols.iter().find(|v| &v.path == p),
        _ => vols.iter().find(|v| v.exists),
    }
    .ok_or_else(|| format!("{} 没有可读的卷", book_id))?;
    let vt = bc.volume_text(&vol.path)?;
    let full_title = if vol.label.is_empty() {
        book.title.clone()
    } else {
        format!("{} {}", book.title, vol.label)
    };
    Ok(AiDocDto {
        lib: l.id,
        lib_title: l.title,
        book_id: Some(book_id),
        path: vol.path.clone(),
        title: full_title,
        content_type: "cbeta".into(),
        text: vt.text,
    })
}

fn to_session_dto(s: smart_library_core::ai_chat::AiSession) -> AiSessionDto {
    AiSessionDto {
        id: s.id,
        scope_kind: s.scope_kind,
        lib: s.lib,
        path: s.path,
        book_id: s.book_id,
        title: s.title,
        doc_title: s.doc_title,
        messages_json: s.messages_json,
        refs_json: s.refs_json,
        updated: s.updated,
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn ai_session_save(
    id: Option<i64>,
    scope_kind: String,
    lib: Option<String>,
    path: Option<String>,
    book_id: Option<String>,
    title: String,
    doc_title: Option<String>,
    messages_json: String,
    refs_json: Option<String>,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    state.ai_chat.save_session(
        id,
        &scope_kind,
        lib.as_deref().unwrap_or(""),
        path.as_deref().unwrap_or(""),
        book_id.as_deref(),
        &title,
        doc_title.as_deref().unwrap_or(""),
        &messages_json,
        refs_json.as_deref().unwrap_or("[]"),
    )
}

#[tauri::command]
pub async fn ai_session_get(id: i64, state: State<'_, AppState>) -> Result<Option<AiSessionDto>, String> {
    Ok(state.ai_chat.get_session(id)?.map(to_session_dto))
}

#[tauri::command]
pub async fn ai_session_latest(lib: String, path: String, state: State<'_, AppState>) -> Result<Option<AiSessionDto>, String> {
    Ok(state.ai_chat.latest_for_doc(&lib, &path)?.map(to_session_dto))
}

#[tauri::command]
pub async fn ai_session_list(
    lib: Option<String>,
    path: Option<String>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<AiSessionSummaryDto>, String> {
    let rows = state
        .ai_chat
        .list_sessions(lib.as_deref(), path.as_deref(), limit.unwrap_or(100).clamp(1, 500))?;
    Ok(rows
        .into_iter()
        .map(|s| AiSessionSummaryDto {
            id: s.id,
            scope_kind: s.scope_kind,
            lib: s.lib,
            path: s.path,
            book_id: s.book_id,
            title: s.title,
            doc_title: s.doc_title,
            updated: s.updated,
        })
        .collect())
}

#[tauri::command]
pub async fn ai_session_delete(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    state.ai_chat.delete_session(id)
}

#[tauri::command]
pub async fn ai_session_rename(id: i64, title: String, state: State<'_, AppState>) -> Result<(), String> {
    state.ai_chat.rename_session(id, &title)
}

#[tauri::command]
pub async fn ai_session_clear(state: State<'_, AppState>) -> Result<usize, String> {
    state.ai_chat.clear()
}
