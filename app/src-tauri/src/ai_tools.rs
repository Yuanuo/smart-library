//! Built-in AI Agent tools that expose Smart Library's knowledge-base
//! capabilities (search, read, history, tags/bookmarks, notes/mindmaps/folders
//! creation & editing) to LLM function calling.

use crate::commands::{self, AppState};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use tauri::State;

/// Access control policy applied when executing tools.
#[derive(Debug, Clone)]
pub struct ToolPolicy {
    pub allow_write: bool,
    pub denied_libs: HashSet<String>,
}

impl ToolPolicy {
    pub fn allow_all() -> Self {
        Self {
            allow_write: true,
            denied_libs: HashSet::new(),
        }
    }

    pub fn is_lib_denied(&self, lib: &str) -> bool {
        self.denied_libs.contains(lib)
    }
}

pub fn is_write_tool(name: &str) -> bool {
    matches!(
        name,
        "create_note"
            | "edit_note"
            | "create_folder"
            | "rename_node"
            | "move_node"
            | "delete_node"
            | "set_node_tags"
            | "add_bookmark"
    )
}

fn check_lib_allowed(policy: &ToolPolicy, lib: &str) -> Result<(), String> {
    if policy.is_lib_denied(lib) {
        Err(format!("知识库「{}」已被设置为禁止外部 Agent 访问", lib))
    } else {
        Ok(())
    }
}

fn check_lib_writable(state: &AppState, lib_id: &str) -> Result<commands::Library, String> {
    let lib = state.library(lib_id)?;
    let is_writable = lib.kind == "notes" || (lib.kind == "folder" && lib.writable);
    if !is_writable {
        return Err(format!(
            "知识库「{}」（id={}，类型={}）为只读库，不允许写入操作。请先调用 list_libraries 检查 writable=true 的可写知识库。",
            lib.title, lib.id, lib.kind
        ));
    }
    Ok(lib)
}

fn filter_json_array_by_lib(arr: &mut Vec<Value>, key: &str, denied: &HashSet<String>) {
    if denied.is_empty() {
        return;
    }
    arr.retain(|item| {
        item.get(key)
            .and_then(|v| v.as_str())
            .map(|id| !denied.contains(id))
            .unwrap_or(true)
    });
}

#[derive(Debug, Clone, Serialize)]
pub struct AiToolResult {
    pub ok: bool,
    pub output: String,
    pub mutated: bool,
    pub affected_lib: Option<String>,
    pub affected_path: Option<String>,
}

fn ok_json(v: Value) -> AiToolResult {
    AiToolResult {
        ok: true,
        output: serde_json::to_string(&v).unwrap_or_else(|_| v.to_string()),
        mutated: false,
        affected_lib: None,
        affected_path: None,
    }
}

fn ok_mutated(v: Value, lib: impl Into<String>, path: Option<String>) -> AiToolResult {
    AiToolResult {
        ok: true,
        output: serde_json::to_string(&v).unwrap_or_else(|_| v.to_string()),
        mutated: true,
        affected_lib: Some(lib.into()),
        affected_path: path,
    }
}

fn err_res(msg: impl Into<String>) -> AiToolResult {
    AiToolResult {
        ok: false,
        output: msg.into(),
        mutated: false,
        affected_lib: None,
        affected_path: None,
    }
}

/// Canonical tool definitions (`{ name, description, parameters }`).
/// Converted to OpenAI `tools` or Anthropic `tools` inside `ai.rs`,
/// and to MCP `tools/list` inside `mcp_server.rs`.
pub fn tool_definitions() -> Vec<Value> {
    tool_definitions_for(true)
}

pub fn tool_definitions_for(allow_write: bool) -> Vec<Value> {
    let mut defs = vec![
        json!({
            "name": "list_libraries",
            "description": "列出当前程序已绑定的所有可访问知识库（包括库 id、标题、类型 notes/folder/cbeta/html/zim、是否可写 writable、索引是否就绪 index_ready）。在检索或写入特定库前，应先调用此工具获取库 id 并检查 writable 字段。",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "get_library_tree",
            "description": "浏览指定知识库的目录树结构。对内部笔记库返回树形节点（含 id/path、title）；对本地文件夹库可传入 parent_rel 浏览子目录，并可限制返回条目数。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "知识库 id" },
                    "parent_rel": { "type": "string", "description": "可选，仅用于 folder 类型库的子目录相对路径（留空为根目录）" },
                    "max_items": { "type": "integer", "description": "可选，最多返回节点数，默认 100" }
                },
                "required": ["lib"]
            }
        }),
        json!({
            "name": "search_library",
            "description": "在已建索引（index_ready=true）的知识库中执行全文检索，返回命中文档及摘录片段。若目标库未建索引或只查书名/标题，请直接用 quick_search。",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "检索关键词" },
                    "lib": { "type": "string", "description": "可选，限定检索的知识库 id；不填则检索全部可访问库" },
                    "phrase": { "type": "boolean", "description": "可选，是否精确短语匹配，默认 false" },
                    "limit": { "type": "integer", "description": "可选，最多返回结果数（1-50），默认 15" }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "quick_search",
            "description": "按标题快速查找文档、经名、文章或目录（无需全文索引，支持多关键词、别名、拼音首字母及 CBETA # 命令）。找具体书名/篇名时优先使用。",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "标题关键词（空格分隔多个词）或 #T0001_001 等定位命令" },
                    "lib": { "type": "string", "description": "可选，限定检索的知识库 id；不填则跨库搜索" },
                    "limit": { "type": "integer", "description": "可选，最多返回结果数（1-100），默认 20" },
                    "pinyin": { "type": "boolean", "description": "可选，是否启用拼音首字母匹配，默认 false" }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "read_document",
            "description": "读取知识库中任意文档的正文或 Markdown 文本。长文档可通过 offset 和 max_chars 分段读取；仅需预览或验证开头时建议传较小的 max_chars（如 1500）。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "知识库 id" },
                    "path": { "type": "string", "description": "文档路径或笔记 id（notes 库为数字 id 字符串，folder 库为相对路径，zim/html 为文章路径，cbeta 为卷路径）" },
                    "book_id": { "type": "string", "description": "可选，CBETA 书号（如 T0220）" },
                    "offset": { "type": "integer", "description": "可选，起始字符偏移（从 0 开始），用于分段读取长文档，默认 0" },
                    "max_chars": { "type": "integer", "description": "可选，本次最大返回字符数（200-60000），默认 6000" }
                },
                "required": ["lib", "path"]
            }
        }),
        json!({
            "name": "list_recent_history",
            "description": "获取用户的最近阅读与编辑历史记录，可按关键词过滤。",
            "parameters": {
                "type": "object",
                "properties": {
                    "filter": { "type": "string", "description": "可选，按标题或路径关键词过滤" },
                    "limit": { "type": "integer", "description": "可选，返回条数，默认 20" }
                },
                "required": []
            }
        }),
        json!({
            "name": "list_marks_and_tags",
            "description": "查询用户的书签、摘录笔记以及全库已有标签列表。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "可选，限定知识库 id" },
                    "tag": { "type": "string", "description": "可选，按标签过滤" },
                    "text": { "type": "string", "description": "可选，按摘录或批注内容过滤" },
                    "limit": { "type": "integer", "description": "可选，返回条数，默认 30" }
                },
                "required": []
            }
        }),
        json!({
            "name": "get_node_tags",
            "description": "读取指定文档当前挂载的标签列表。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "知识库 id" },
                    "path": { "type": "string", "description": "文档路径或笔记 id" },
                    "book_id": { "type": "string", "description": "可选，CBETA 书号" }
                },
                "required": ["lib", "path"]
            }
        }),
        json!({
            "name": "dict_lookup",
            "description": "在用户已安装的本地词典中精确查询指定词条的释义。",
            "parameters": {
                "type": "object",
                "properties": {
                    "word": { "type": "string", "description": "要查询的词条" },
                    "limit": { "type": "integer", "description": "可选，最多返回释义条数（1-50），默认 20" }
                },
                "required": ["word"]
            }
        }),
        json!({
            "name": "dict_search",
            "description": "在用户已安装的本地词典中按关键词搜索相关词条列表。",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "搜索关键词" },
                    "offset": { "type": "integer", "description": "可选，分页偏移，默认 0" },
                    "limit": { "type": "integer", "description": "可选，返回条数（1-100），默认 30" }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "set_node_tags",
            "description": "为指定文档设置标签列表（多个标签用逗号或空格分隔）。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "知识库 id" },
                    "path": { "type": "string", "description": "文档路径或笔记 id" },
                    "book_id": { "type": "string", "description": "可选，CBETA 书号" },
                    "doc_title": { "type": "string", "description": "文档标题" },
                    "tags": { "type": "string", "description": "标签列表字符串，如 \"禅宗, 笔记\"" }
                },
                "required": ["lib", "path", "doc_title", "tags"]
            }
        }),
        json!({
            "name": "add_bookmark",
            "description": "为指定文档添加一条书签/摘录批注。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "知识库 id" },
                    "path": { "type": "string", "description": "文档路径或笔记 id" },
                    "book_id": { "type": "string", "description": "可选，CBETA 书号" },
                    "doc_title": { "type": "string", "description": "文档标题" },
                    "quote": { "type": "string", "description": "摘录原文（书签可留空）" },
                    "note": { "type": "string", "description": "批注或心得" },
                    "tags": { "type": "string", "description": "可选，标签字符串" }
                },
                "required": ["lib", "path", "doc_title", "note"]
            }
        }),
        json!({
            "name": "create_note",
            "description": "在可写的知识库（list_libraries 返回 writable=true 的 notes 库或读写 folder 库）中新建文档（Markdown 笔记、脑图 mindmap 或白板 whiteboard）并写入初始内容。若创建 mindmap，content 可直接传 Markdown 标题/缩进列表大纲，程序会自动转换为脑图结构。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "目标知识库 id（必须是 writable=true 的 notes 库或 folder 库）" },
                    "parent": { "type": "string", "description": "可选，目标父路径。notes 库可传 \"目录A/子目录B\"（空串表示根目录）；folder 库传相对文件夹路径（空串表示根目录）" },
                    "title": { "type": "string", "description": "新建文档标题" },
                    "kind": { "type": "string", "enum": ["md", "mindmap", "whiteboard"], "description": "文档类型：md（默认）、mindmap（脑图）或 whiteboard（白板）" },
                    "content": { "type": "string", "description": "可选，初始正文内容（md 为 Markdown，mindmap 为 Markdown 大纲或 JSON）" }
                },
                "required": ["lib", "title"]
            }
        }),
        json!({
            "name": "edit_note",
            "description": "编辑已有可写文档（向末尾追加内容或整篇替换，修改前自动创建快照）。要求目标知识库 writable=true。若目标为 mindmap 且传入 Markdown 大纲，可自动转换为脑图 JSON。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "知识库 id（必须 writable=true）" },
                    "id": { "type": "string", "description": "文档 id（notes 库为数字 id 字符串，folder 库为相对路径）" },
                    "mode": { "type": "string", "enum": ["append", "replace"], "description": "append（追加到末尾）或 replace（替换正文）" },
                    "content": { "type": "string", "description": "要写入或追加的内容" }
                },
                "required": ["lib", "id", "mode", "content"]
            }
        }),
        json!({
            "name": "create_folder",
            "description": "在读写模式的本地文件夹库（folder 且 writable=true）中新建子文件夹。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "folder 类型且 writable=true 的知识库 id" },
                    "parent": { "type": "string", "description": "父目录相对路径（空串表示库根目录）" },
                    "name": { "type": "string", "description": "新建文件夹名称" }
                },
                "required": ["lib", "name"]
            }
        }),
        json!({
            "name": "rename_node",
            "description": "重命名可写知识库（writable=true）中的文档。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "知识库 id（必须 writable=true）" },
                    "id": { "type": "string", "description": "文档 id（notes 为数字 id，folder 为相对路径）" },
                    "title": { "type": "string", "description": "新标题" }
                },
                "required": ["lib", "id", "title"]
            }
        }),
        json!({
            "name": "move_node",
            "description": "移动可写知识库（writable=true）中的文档或文件夹到新的父节点下。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "知识库 id（必须 writable=true）" },
                    "id": { "type": "string", "description": "要移动的文档 id 或相对路径" },
                    "target_parent": { "type": "string", "description": "目标父节点 id（notes 库，空串或 null 为根）或目标父文件夹相对路径（folder 库，空串为根）" }
                },
                "required": ["lib", "id"]
            }
        }),
        json!({
            "name": "delete_node",
            "description": "删除可写知识库（writable=true）中的文档（自动移入回收站，可恢复）。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lib": { "type": "string", "description": "知识库 id（必须 writable=true）" },
                    "id": { "type": "string", "description": "文档 id（notes 为数字 id，folder 为相对路径）" }
                },
                "required": ["lib", "id"]
            }
        }),
    ];
    if !allow_write {
        defs.retain(|d| {
            d.get("name")
                .and_then(|v| v.as_str())
                .map(|n| !is_write_tool(n))
                .unwrap_or(true)
        });
    }
    defs
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

fn arg_nonempty(args: &Value, key: &str) -> Option<String> {
    arg_str(args, key)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn trim_tree_value(v: Value, budget: &mut usize) -> Value {
    if *budget == 0 {
        return json!({ "truncated": true });
    }
    match v {
        Value::Array(arr) => {
            let mut out = Vec::new();
            for item in arr {
                if *budget == 0 {
                    out.push(json!({ "truncated": true }));
                    break;
                }
                *budget -= 1;
                out.push(trim_tree_value(item, budget));
            }
            Value::Array(out)
        }
        Value::Object(mut map) => {
            if let Some(children) = map.remove("children") {
                map.insert("children".into(), trim_tree_value(children, budget));
            }
            Value::Object(map)
        }
        other => other,
    }
}

pub async fn exec_tool(
    name: &str,
    args_raw: &str,
    state: State<'_, AppState>,
) -> AiToolResult {
    exec_tool_with_policy(name, args_raw, state, &ToolPolicy::allow_all()).await
}

pub async fn exec_tool_with_policy(
    name: &str,
    args_raw: &str,
    state: State<'_, AppState>,
    policy: &ToolPolicy,
) -> AiToolResult {
    if is_write_tool(name) && !policy.allow_write {
        return err_res(format!(
            "当前本地 Agent 服务为只读模式，禁止调用写入工具「{}」。如需允许写入，请在 Smart Library 设置 → 本地 Agent 中开启「允许写入」。",
            name
        ));
    }

    let args: Value = if args_raw.trim().is_empty() {
        json!({})
    } else {
        match serde_json::from_str(args_raw) {
            Ok(v) => v,
            Err(e) => return err_res(format!("工具参数 JSON 解析失败：{}", e)),
        }
    };

    match name {
        "list_libraries" => match commands::status(state).await {
            Ok(st) => {
                let mut v = serde_json::to_value(st).unwrap_or(json!({}));
                let libs: Vec<Value> = v
                    .get_mut("libraries")
                    .and_then(|x| x.as_array_mut())
                    .map(|arr| {
                        filter_json_array_by_lib(arr, "id", &policy.denied_libs);
                        arr.iter()
                            .map(|item| {
                                let mut obj = serde_json::Map::new();
                                for k in ["id", "title", "kind", "writable", "index_ready", "data_version"] {
                                    if let Some(val) = item.get(k) {
                                        if !val.is_null() {
                                            obj.insert(k.into(), val.clone());
                                        }
                                    }
                                }
                                if let Some(err) = item.get("error").filter(|e| !e.is_null()) {
                                    obj.insert("error".into(), err.clone());
                                }
                                Value::Object(obj)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                ok_json(json!({ "libraries": libs }))
            }
            Err(e) => err_res(e),
        },

        "get_library_tree" => {
            let Some(lib) = arg_nonempty(&args, "lib") else {
                return err_res("缺少必填参数 lib");
            };
            if let Err(e) = check_lib_allowed(policy, &lib) {
                return err_res(e);
            }
            let parent_rel = arg_nonempty(&args, "parent_rel");
            let max_items = args
                .get("max_items")
                .and_then(|v| v.as_u64())
                .unwrap_or(100)
                .clamp(1, 500) as usize;
            let res = if let Some(rel) = parent_rel {
                commands::folder_children(lib, rel, state).await
            } else {
                commands::tree(lib, state).await
            };
            match res {
                Ok(nodes) => {
                    let val = serde_json::to_value(nodes).unwrap_or(json!([]));
                    let mut budget = max_items;
                    ok_json(trim_tree_value(val, &mut budget))
                }
                Err(e) => err_res(e),
            }
        }

        "search_library" => {
            let Some(query) = arg_nonempty(&args, "query") else {
                return err_res("缺少必填参数 query");
            };
            let lib = arg_nonempty(&args, "lib");
            if let Some(ref l) = lib {
                if let Err(e) = check_lib_allowed(policy, l) {
                    return err_res(e);
                }
            }
            let phrase = args.get("phrase").and_then(|v| v.as_bool()).unwrap_or(false);
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(15)
                .clamp(1, 50) as usize;
            match commands::search(query.clone(), phrase, lib.clone(), None, None, Some(1), Some(limit), state.clone()).await
            {
                Ok(dto) => {
                    let mut v = serde_json::to_value(dto).unwrap_or(json!({}));
                    if !policy.denied_libs.is_empty() {
                        let mut first_allowed: Option<String> = None;
                        if let Some(arr) = v.get_mut("libraries").and_then(|x| x.as_array_mut()) {
                            filter_json_array_by_lib(arr, "id", &policy.denied_libs);
                            for item in arr.iter() {
                                let count = item.get("count").and_then(|c| c.as_u64()).unwrap_or(0);
                                if count > 0 {
                                    if let Some(id) = item.get("id").and_then(|s| s.as_str()) {
                                        first_allowed = Some(id.to_string());
                                        break;
                                    }
                                }
                            }
                        }
                        let selected_denied = v
                            .get("lib")
                            .and_then(|s| s.as_str())
                            .map(|id| policy.is_lib_denied(id))
                            .unwrap_or(false);
                        if selected_denied {
                            if let Some(next_lib) = first_allowed {
                                if let Ok(dto2) = commands::search(query, phrase, Some(next_lib), None, None, Some(1), Some(limit), state).await {
                                    v = serde_json::to_value(dto2).unwrap_or(json!({}));
                                    if let Some(arr) = v.get_mut("libraries").and_then(|x| x.as_array_mut()) {
                                        filter_json_array_by_lib(arr, "id", &policy.denied_libs);
                                    }
                                }
                            } else if let Some(obj) = v.as_object_mut() {
                                obj.insert("lib".into(), Value::Null);
                                obj.insert("hits".into(), json!([]));
                                obj.insert("facets".into(), json!([]));
                                obj.insert("total_volumes".into(), json!(0));
                                obj.insert("total_books".into(), json!(0));
                                obj.insert("pages".into(), json!(0));
                            }
                        }
                    }
                    // Strip heavy UI-only fields (facets, millis, zero-hit libraries) to keep responses fast for LLMs
                    if let Some(obj) = v.as_object_mut() {
                        obj.remove("facets");
                        obj.remove("millis");
                        if let Some(arr) = obj.get_mut("libraries").and_then(|x| x.as_array_mut()) {
                            arr.retain(|item| item.get("count").and_then(|c| c.as_u64()).unwrap_or(0) > 0);
                        }
                    }
                    ok_json(v)
                }
                Err(e) => err_res(e),
            }
        }

        "quick_search" => {
            let Some(query) = arg_nonempty(&args, "query") else {
                return err_res("缺少必填参数 query");
            };
            let lib = arg_nonempty(&args, "lib");
            if let Some(ref l) = lib {
                if let Err(e) = check_lib_allowed(policy, l) {
                    return err_res(e);
                }
            }
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .clamp(1, 100) as usize;
            let pinyin = args.get("pinyin").and_then(|v| v.as_bool()).unwrap_or(false);
            match commands::quick_search(query, lib, Some(limit), None, None, Some(pinyin), state).await {
                Ok(dto) => {
                    let mut v = serde_json::to_value(dto).unwrap_or(json!({}));
                    if !policy.denied_libs.is_empty() {
                        if let Some(arr) = v.get_mut("libraries").and_then(|x| x.as_array_mut()) {
                            filter_json_array_by_lib(arr, "id", &policy.denied_libs);
                        }
                        if let Some(arr) = v.get_mut("items").and_then(|x| x.as_array_mut()) {
                            filter_json_array_by_lib(arr, "lib", &policy.denied_libs);
                            let n = arr.len();
                            if let Some(obj) = v.as_object_mut() {
                                obj.insert("total".into(), json!(n));
                            }
                        }
                    }
                    if let Some(obj) = v.as_object_mut() {
                        obj.remove("millis");
                        if let Some(arr) = obj.get_mut("libraries").and_then(|x| x.as_array_mut()) {
                            arr.retain(|item| item.get("count").and_then(|c| c.as_u64()).unwrap_or(0) > 0);
                        }
                    }
                    ok_json(v)
                }
                Err(e) => err_res(e),
            }
        }

        "read_document" => {
            let Some(lib) = arg_nonempty(&args, "lib") else {
                return err_res("缺少必填参数 lib");
            };
            if let Err(e) = check_lib_allowed(policy, &lib) {
                return err_res(e);
            }
            let path = arg_nonempty(&args, "path");
            let book_id = arg_nonempty(&args, "book_id");
            let offset = args
                .get("offset")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let max_chars = args
                .get("max_chars")
                .and_then(|v| v.as_u64())
                .unwrap_or(6000)
                .clamp(200, 60000) as usize;
            match commands::ai_doc_text(lib, book_id, path, state).await {
                Ok(mut dto) => {
                    let total_chars = dto.text.chars().count();
                    let slice: String = dto.text.chars().skip(offset).take(max_chars).collect();
                    let returned_chars = slice.chars().count();
                    let next_end = offset.saturating_add(returned_chars);
                    let truncated = next_end < total_chars;
                    dto.text = slice;
                    let mut v = serde_json::to_value(&dto).unwrap_or(json!({}));
                    if let Some(obj) = v.as_object_mut() {
                        obj.insert("offset".into(), json!(offset));
                        obj.insert("returned_chars".into(), json!(returned_chars));
                        obj.insert("total_chars".into(), json!(total_chars));
                        obj.insert("truncated".into(), json!(truncated));
                        if truncated {
                            obj.insert("next_offset".into(), json!(next_end));
                        }
                    }
                    ok_json(v)
                }
                Err(e) => err_res(e),
            }
        }

        "list_recent_history" => {
            let filter = arg_nonempty(&args, "filter");
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .clamp(1, 200) as usize;
            match commands::recent_list(filter, Some(limit), state).await {
                Ok(list) => {
                    let mut v = serde_json::to_value(list).unwrap_or(json!([]));
                    if let Some(arr) = v.as_array_mut() {
                        filter_json_array_by_lib(arr, "lib", &policy.denied_libs);
                    }
                    ok_json(v)
                }
                Err(e) => err_res(e),
            }
        }

        "list_marks_and_tags" => {
            let lib = arg_nonempty(&args, "lib");
            if let Some(ref l) = lib {
                if let Err(e) = check_lib_allowed(policy, l) {
                    return err_res(e);
                }
            }
            let tag = arg_nonempty(&args, "tag");
            let text = arg_nonempty(&args, "text");
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(30)
                .clamp(1, 200) as usize;
            let tags = commands::mark_tags(state.clone()).await.unwrap_or_default();
            match commands::mark_list(tag, text, lib, None, None, None, Some(limit), state).await {
                Ok(marks) => {
                    let mut marks_v = serde_json::to_value(marks).unwrap_or(json!([]));
                    if let Some(arr) = marks_v.as_array_mut() {
                        filter_json_array_by_lib(arr, "lib", &policy.denied_libs);
                    }
                    ok_json(json!({ "all_tags": tags, "marks": marks_v }))
                }
                Err(e) => err_res(e),
            }
        }

        "get_node_tags" => {
            let Some(lib) = arg_nonempty(&args, "lib") else {
                return err_res("缺少必填参数 lib");
            };
            if let Err(e) = check_lib_allowed(policy, &lib) {
                return err_res(e);
            }
            let Some(path) = arg_nonempty(&args, "path") else {
                return err_res("缺少必填参数 path");
            };
            let book_id = arg_nonempty(&args, "book_id");
            match commands::node_tags_get(lib, book_id, path, state).await {
                Ok(tags) => ok_json(json!({ "tags": tags })),
                Err(e) => err_res(e),
            }
        }

        "dict_lookup" => {
            let Some(word) = arg_nonempty(&args, "word") else {
                return err_res("缺少必填参数 word");
            };
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .clamp(1, 50) as usize;
            match commands::dict_lookup(word, None, Some(limit), state).await {
                Ok(entries) => ok_json(serde_json::to_value(entries).unwrap_or(json!([]))),
                Err(e) => err_res(e),
            }
        }

        "dict_search" => {
            let Some(query) = arg_nonempty(&args, "query") else {
                return err_res("缺少必填参数 query");
            };
            let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(30)
                .clamp(1, 100) as usize;
            match commands::dict_search(query, None, Some(offset), Some(limit), state).await {
                Ok(res) => ok_json(serde_json::to_value(res).unwrap_or(json!({}))),
                Err(e) => err_res(e),
            }
        }

        "set_node_tags" => {
            let Some(lib) = arg_nonempty(&args, "lib") else {
                return err_res("缺少必填参数 lib");
            };
            if let Err(e) = check_lib_allowed(policy, &lib) {
                return err_res(e);
            }
            let Some(path) = arg_nonempty(&args, "path") else {
                return err_res("缺少必填参数 path");
            };
            let doc_title = arg_nonempty(&args, "doc_title").unwrap_or_else(|| path.clone());
            let tags = arg_str(&args, "tags").unwrap_or("").to_string();
            let book_id = arg_nonempty(&args, "book_id");
            match commands::node_tags_set(lib.clone(), book_id, path.clone(), doc_title, tags.clone(), state).await {
                Ok(()) => ok_mutated(json!({ "ok": true, "lib": lib, "path": path, "tags": tags }), lib, Some(path)),
                Err(e) => err_res(e),
            }
        }

        "add_bookmark" => {
            let Some(lib) = arg_nonempty(&args, "lib") else {
                return err_res("缺少必填参数 lib");
            };
            if let Err(e) = check_lib_allowed(policy, &lib) {
                return err_res(e);
            }
            let Some(path) = arg_nonempty(&args, "path") else {
                return err_res("缺少必填参数 path");
            };
            let book_id = arg_nonempty(&args, "book_id");
            let doc_title = arg_nonempty(&args, "doc_title").unwrap_or_else(|| path.clone());
            let quote = arg_str(&args, "quote").unwrap_or("").to_string();
            let note = arg_str(&args, "note").unwrap_or("").to_string();
            let tags = arg_str(&args, "tags").unwrap_or("").to_string();
            let kind = if quote.trim().is_empty() { "bookmark" } else { "note" }.to_string();
            match commands::mark_add(
                kind,
                lib.clone(),
                path.clone(),
                book_id,
                None,
                quote,
                note,
                doc_title,
                String::new(),
                tags,
                state,
            )
            .await
            {
                Ok(id) => ok_mutated(json!({ "id": id, "lib": lib, "path": path }), lib, Some(path)),
                Err(e) => err_res(e),
            }
        }

        "create_note" => {
            let Some(lib_id) = arg_nonempty(&args, "lib") else {
                return err_res("缺少必填参数 lib");
            };
            if let Err(e) = check_lib_allowed(policy, &lib_id) {
                return err_res(e);
            }
            let lib_info = match check_lib_writable(&state, &lib_id) {
                Ok(l) => l,
                Err(e) => return err_res(e),
            };
            let Some(title) = arg_nonempty(&args, "title") else {
                return err_res("缺少必填参数 title");
            };
            let parent = arg_str(&args, "parent").unwrap_or("").trim().to_string();
            let kind = arg_nonempty(&args, "kind").unwrap_or_else(|| "md".to_string());
            let content = arg_str(&args, "content").unwrap_or("").to_string();

            let created = if lib_info.kind == "notes" {
                commands::create_note_at(
                    Some(lib_id.clone()),
                    parent,
                    Some(title.clone()),
                    Some(kind.clone()),
                    state.clone(),
                )
                .await
            } else if lib_info.kind == "folder" {
                commands::create_folder_file(
                    lib_id.clone(),
                    parent,
                    Some(kind.clone()),
                    Some(title.clone()),
                    state.clone(),
                )
                .await
            } else {
                return err_res(format!("知识库 {}（类型 {}）不支持新建文档", lib_info.title, lib_info.kind));
            };

            let dto = match created {
                Ok(d) => d,
                Err(e) => return err_res(e),
            };
            let created_val = serde_json::to_value(&dto).unwrap_or(json!({}));
            let doc_id = created_val["id"].as_str().unwrap_or("").to_string();

            if !content.trim().is_empty() && !doc_id.is_empty() {
                let body = if kind == "mindmap" {
                    match smart_library_core::mindmap::from_outline_or_json(&content, &title) {
                        Ok(j) => j,
                        Err(e) => return err_res(format!("已创建脑图但转换内容失败：{}", e)),
                    }
                } else {
                    content
                };
                if let Err(e) = commands::save_note(lib_id.clone(), doc_id.clone(), body, state).await {
                    return err_res(format!("文档已创建（id={}），但写入内容失败：{}", doc_id, e));
                }
            }
            ok_mutated(created_val, lib_id, Some(doc_id))
        }

        "edit_note" => {
            let Some(lib) = arg_nonempty(&args, "lib") else {
                return err_res("缺少必填参数 lib");
            };
            if let Err(e) = check_lib_allowed(policy, &lib) {
                return err_res(e);
            }
            let lib_info = match check_lib_writable(&state, &lib) {
                Ok(l) => l,
                Err(e) => return err_res(e),
            };
            let Some(id) = arg_nonempty(&args, "id") else {
                return err_res("缺少必填参数 id");
            };
            let mode = arg_nonempty(&args, "mode").unwrap_or_else(|| "append".to_string());
            let content = arg_str(&args, "content").unwrap_or("").to_string();

            let (title, content_type, existing_raw) = match commands::read_writable_raw(&state, &lib, &id) {
                Ok(v) => v,
                Err(e) => return err_res(e),
            };
            let _ = state
                .snapshots
                .create(&lib, &id, "ai", "AI 修改前", &content_type, &existing_raw);

            let res: Result<(), String> = if content_type == smart_library_core::mindmap::TYPE {
                let next_json = if mode == "replace" {
                    smart_library_core::mindmap::from_outline_or_json(&content, &title)
                } else {
                    smart_library_core::mindmap::append_outline_or_json(&existing_raw, &content, &title)
                };
                match next_json {
                    Ok(body) => commands::save_note(lib.clone(), id.clone(), body, state).await,
                    Err(e) => Err(e),
                }
            } else if content_type == smart_library_core::whiteboard::TYPE {
                if mode != "replace" {
                    Err("白板文档不支持文本追加，请使用 replace 模式传入合法的 Excalidraw JSON，或在编辑器中应用 AI 结果".into())
                } else {
                    match serde_json::from_str::<Value>(&content) {
                        Ok(v) if v.is_object() && v.get("elements").is_some_and(Value::is_array) => {
                            commands::save_note(lib.clone(), id.clone(), content, state).await
                        }
                        Ok(_) => Err("白板 JSON 缺少 elements 数组，已阻止覆写".into()),
                        Err(e) => Err(format!("白板 JSON 解析失败，已阻止覆写：{}", e)),
                    }
                }
            } else if mode == "replace" {
                commands::save_note(lib.clone(), id.clone(), content, state).await
            } else if lib_info.kind == "folder" {
                let base = existing_raw.trim_end();
                let next = if base.is_empty() {
                    content
                } else {
                    format!("{}\n\n{}", base, content.trim_start())
                };
                commands::save_note(lib.clone(), id.clone(), next, state).await
            } else {
                commands::note_append(lib.clone(), id.clone(), content, state).await
            };
            match res {
                Ok(()) => ok_mutated(json!({ "ok": true, "lib": lib, "id": id, "mode": mode }), lib, Some(id)),
                Err(e) => err_res(e),
            }
        }

        "create_folder" => {
            let Some(lib) = arg_nonempty(&args, "lib") else {
                return err_res("缺少必填参数 lib");
            };
            if let Err(e) = check_lib_allowed(policy, &lib) {
                return err_res(e);
            }
            if let Err(e) = check_lib_writable(&state, &lib) {
                return err_res(e);
            }
            let Some(name) = arg_nonempty(&args, "name") else {
                return err_res("缺少必填参数 name");
            };
            let parent = arg_str(&args, "parent").unwrap_or("").trim().to_string();
            match commands::create_folder_dir(lib.clone(), parent, name, state).await {
                Ok(dto) => {
                    let v = serde_json::to_value(&dto).unwrap_or(json!({}));
                    let path = v["id"].as_str().map(|s| s.to_string());
                    ok_mutated(v, lib, path)
                }
                Err(e) => err_res(e),
            }
        }

        "rename_node" => {
            let Some(lib) = arg_nonempty(&args, "lib") else {
                return err_res("缺少必填参数 lib");
            };
            if let Err(e) = check_lib_allowed(policy, &lib) {
                return err_res(e);
            }
            if let Err(e) = check_lib_writable(&state, &lib) {
                return err_res(e);
            }
            let Some(id) = arg_nonempty(&args, "id") else {
                return err_res("缺少必填参数 id");
            };
            let Some(title) = arg_nonempty(&args, "title") else {
                return err_res("缺少必填参数 title");
            };
            match commands::rename_note(lib.clone(), id.clone(), title, state).await {
                Ok(new_id) => ok_mutated(json!({ "ok": true, "lib": lib, "id": new_id }), lib, Some(new_id)),
                Err(e) => err_res(e),
            }
        }

        "move_node" => {
            let Some(lib) = arg_nonempty(&args, "lib") else {
                return err_res("缺少必填参数 lib");
            };
            if let Err(e) = check_lib_allowed(policy, &lib) {
                return err_res(e);
            }
            let lib_info = match check_lib_writable(&state, &lib) {
                Ok(l) => l,
                Err(e) => return err_res(e),
            };
            let Some(id) = arg_nonempty(&args, "id") else {
                return err_res("缺少必填参数 id");
            };
            let target_parent = arg_str(&args, "target_parent").unwrap_or("").trim().to_string();
            if lib_info.kind == "folder" {
                match commands::move_folder_node(lib.clone(), id, target_parent, state).await {
                    Ok(new_rel) => ok_mutated(json!({ "ok": true, "lib": lib, "id": new_rel }), lib, Some(new_rel)),
                    Err(e) => err_res(e),
                }
            } else if lib_info.kind == "notes" {
                let parent_id = if target_parent.is_empty() {
                    None
                } else {
                    Some(target_parent)
                };
                match commands::move_note(lib.clone(), id.clone(), parent_id, 999999, state).await {
                    Ok(()) => ok_mutated(json!({ "ok": true, "lib": lib, "id": id }), lib, Some(id)),
                    Err(e) => err_res(e),
                }
            } else {
                err_res(format!("知识库 {} 不支持移动操作", lib_info.title))
            }
        }

        "delete_node" => {
            let Some(lib) = arg_nonempty(&args, "lib") else {
                return err_res("缺少必填参数 lib");
            };
            if let Err(e) = check_lib_allowed(policy, &lib) {
                return err_res(e);
            }
            if let Err(e) = check_lib_writable(&state, &lib) {
                return err_res(e);
            }
            let Some(id) = arg_nonempty(&args, "id") else {
                return err_res("缺少必填参数 id");
            };
            match commands::delete_note(lib.clone(), id.clone(), state).await {
                Ok(n) => ok_mutated(json!({ "deleted": n, "lib": lib, "id": id }), lib, None),
                Err(e) => err_res(e),
            }
        }

        _ => err_res(format!("未知工具：{}", name)),
    }
}
