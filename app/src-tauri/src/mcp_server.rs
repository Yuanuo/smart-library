//! Local MCP (Streamable HTTP) + lightweight REST server embedded in the
//! Smart Library desktop app.
//!
//! Strictly binds to `127.0.0.1:<port>`, shares the single in-process
//! `AppState`, reuses `ai_tools` (with read-only / read-write and per-library
//! deny-list enforcement), and emits `library-mutated` to refresh the UI
//! whenever an external Agent writes to a library.

use crate::ai_tools::{self, ToolPolicy};
use crate::commands::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

pub const DEFAULT_PORT: u16 = 27123;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_true")]
    pub allow_write: bool,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub denied_libs: Vec<String>,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_true() -> bool {
    true
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            allow_write: true,
            token: String::new(),
            denied_libs: Vec::new(),
        }
    }
}

impl McpConfig {
    pub fn normalized(mut self) -> Self {
        if self.port == 0 {
            self.port = DEFAULT_PORT;
        }
        self.token = self.token.trim().to_string();
        let mut seen = HashSet::new();
        self.denied_libs.retain(|s| {
            let trimmed = s.trim();
            !trimmed.is_empty() && seen.insert(trimmed.to_string())
        });
        self
    }

    pub fn to_policy(&self) -> ToolPolicy {
        ToolPolicy {
            allow_write: self.allow_write,
            denied_libs: self.denied_libs.iter().cloned().collect(),
        }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.port)
    }

    fn path(config_dir: &Path) -> PathBuf {
        config_dir.join("mcp.json")
    }

    pub fn load(config_dir: &Path) -> Self {
        std::fs::read_to_string(Self::path(config_dir))
            .ok()
            .and_then(|s| serde_json::from_str::<McpConfig>(&s).ok())
            .map(Self::normalized)
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(config_dir)
            .map_err(|e| format!("创建配置目录失败：{e}"))?;
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化本地 Agent 配置失败：{e}"))?;
        std::fs::write(Self::path(config_dir), text)
            .map_err(|e| format!("保存本地 Agent 配置失败：{e}"))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct McpStatusDto {
    pub running: bool,
    pub url: String,
    pub error: Option<String>,
    pub config: McpConfig,
}

#[derive(Clone)]
struct SharedAuth {
    token: Arc<RwLock<String>>,
    policy: Arc<RwLock<ToolPolicy>>,
}

struct RuntimeInner {
    config: McpConfig,
    running: bool,
    active_port: Option<u16>,
    error: Option<String>,
    shutdown: Option<oneshot::Sender<()>>,
    shared: SharedAuth,
}

pub struct McpRuntime {
    inner: Mutex<RuntimeInner>,
}

impl McpRuntime {
    pub fn new(config: McpConfig) -> Self {
        let cfg = config.normalized();
        let shared = SharedAuth {
            token: Arc::new(RwLock::new(cfg.token.clone())),
            policy: Arc::new(RwLock::new(cfg.to_policy())),
        };
        Self {
            inner: Mutex::new(RuntimeInner {
                config: cfg,
                running: false,
                active_port: None,
                error: None,
                shutdown: None,
                shared,
            }),
        }
    }

    pub fn status(&self) -> McpStatusDto {
        let g = self.inner.lock().unwrap();
        McpStatusDto {
            running: g.running,
            url: g.config.url(),
            error: g.error.clone(),
            config: g.config.clone(),
        }
    }

    /// Start or stop the server according to `cfg`. Does not persist to disk.
    pub fn reconcile(&self, app: &AppHandle, cfg: McpConfig) -> McpStatusDto {
        let cfg = cfg.normalized();
        let mut g = self.inner.lock().unwrap();

        // Always hot-update auth token & tool policy without restarting if port unchanged.
        *g.shared.token.write().unwrap() = cfg.token.clone();
        *g.shared.policy.write().unwrap() = cfg.to_policy();

        let need_restart = if !cfg.enabled {
            g.running
        } else {
            !g.running || g.active_port != Some(cfg.port)
        };

        if need_restart {
            if let Some(tx) = g.shutdown.take() {
                let _ = tx.send(());
            }
            g.running = false;
            g.active_port = None;
            g.error = None;
        }

        if cfg.enabled && !g.running {
            match StdTcpListener::bind(("127.0.0.1", cfg.port)) {
                Ok(std_listener) => {
                    if let Err(e) = std_listener.set_nonblocking(true) {
                        g.error = Some(format!("初始化监听套接字失败：{e}"));
                    } else {
                        let (tx, rx) = oneshot::channel();
                        let app_handle = app.clone();
                        let shared = g.shared.clone();
                        tauri::async_runtime::spawn(async move {
                            run_server(std_listener, rx, app_handle, shared).await;
                        });
                        g.shutdown = Some(tx);
                        g.running = true;
                        g.active_port = Some(cfg.port);
                        g.error = None;
                    }
                }
                Err(e) => {
                    g.error = Some(format!(
                        "无法监听 127.0.0.1:{}（端口可能已被占用）：{}",
                        cfg.port, e
                    ));
                }
            }
        }

        g.config = cfg;
        McpStatusDto {
            running: g.running,
            url: g.config.url(),
            error: g.error.clone(),
            config: g.config.clone(),
        }
    }

    pub fn stop(&self) {
        let mut g = self.inner.lock().unwrap();
        if let Some(tx) = g.shutdown.take() {
            let _ = tx.send(());
        }
        g.running = false;
        g.active_port = None;
    }
}

pub fn init_and_start(app: &AppHandle, config_dir: &Path) -> McpRuntime {
    let cfg = McpConfig::load(config_dir);
    let rt = McpRuntime::new(cfg.clone());
    if cfg.enabled {
        let st = rt.reconcile(app, cfg);
        if let Some(err) = st.error {
            eprintln!("local MCP server failed to start: {err}");
        }
    }
    rt
}

// ------------------------------------------------------------ Tauri commands

#[tauri::command]
pub async fn mcp_status_get(rt: State<'_, McpRuntime>) -> Result<McpStatusDto, String> {
    Ok(rt.status())
}

#[tauri::command]
pub async fn mcp_config_set(
    config: McpConfig,
    app: AppHandle,
    state: State<'_, AppState>,
    rt: State<'_, McpRuntime>,
) -> Result<McpStatusDto, String> {
    let normalized = config.normalized();
    normalized.save(state.config_dir())?;
    Ok(rt.reconcile(&app, normalized))
}

// ------------------------------------------------------------ HTTP/1.1 loop

async fn run_server(
    std_listener: StdTcpListener,
    mut shutdown: oneshot::Receiver<()>,
    app: AppHandle,
    shared: SharedAuth,
) {
    let listener = match TcpListener::from_std(std_listener) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("local MCP listener error: {e}");
            return;
        }
    };

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                break;
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let app_c = app.clone();
                        let shared_c = shared.clone();
                        tauri::async_runtime::spawn(async move {
                            handle_connection(stream, app_c, shared_c).await;
                        });
                    }
                    Err(e) => {
                        eprintln!("local MCP accept error: {e}");
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }
}

struct ParsedHttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    keep_alive: bool,
}

impl ParsedHttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

async fn read_http_request(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
) -> Result<Option<ParsedHttpRequest>, (u16, String)> {
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = find_header_end(buf) {
            break pos;
        }
        if buf.len() > MAX_HEADER_BYTES {
            return Err((431, "Request headers too large".into()));
        }
        let n = stream
            .read(&mut tmp)
            .await
            .map_err(|e| (400, format!("read error: {e}")))?;
        if n == 0 {
            if buf.is_empty() {
                return Ok(None);
            }
            return Err((400, "Unexpected EOF while reading headers".into()));
        }
        buf.extend_from_slice(&tmp[..n]);
    };

    let mut headers_arr = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers_arr);
    match req.parse(&buf[..header_end]) {
        Ok(httparse::Status::Complete(_)) => {}
        Ok(httparse::Status::Partial) => {
            return Err((400, "Incomplete HTTP headers".into()));
        }
        Err(e) => {
            return Err((400, format!("Malformed HTTP request: {e}")));
        }
    }

    let method = req.method.unwrap_or("").to_string();
    let raw_path = req.path.unwrap_or("/");
    let path = raw_path.split('?').next().unwrap_or("/").to_string();
    let version = req.version.unwrap_or(1);
    let mut headers = Vec::new();
    for h in req.headers.iter() {
        let val = String::from_utf8_lossy(h.value).trim().to_string();
        headers.push((h.name.to_string(), val));
    }

    let conn_hdr = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("connection"))
        .map(|(_, v)| v.to_ascii_lowercase());
    let keep_alive = match conn_hdr.as_deref() {
        Some(v) if v.contains("close") => false,
        Some(v) if v.contains("keep-alive") => true,
        _ => version >= 1,
    };

    let is_chunked = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked"));
    let content_length: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);

    // Consume headers from persistent buffer; leftover bytes belong to body or next pipelined request
    buf.drain(..header_end);

    let body = if is_chunked {
        read_chunked_body(stream, buf).await?
    } else if content_length > 0 {
        if content_length > MAX_BODY_BYTES {
            return Err((413, "Request body too large".into()));
        }
        while buf.len() < content_length {
            let n = stream
                .read(&mut tmp)
                .await
                .map_err(|e| (400, format!("read body error: {e}")))?;
            if n == 0 {
                return Err((400, "Unexpected EOF while reading body".into()));
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        buf.drain(..content_length).collect()
    } else {
        Vec::new()
    };

    Ok(Some(ParsedHttpRequest {
        method,
        path,
        headers,
        body,
        keep_alive,
    }))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|pos| pos + 4)
}

async fn read_chunked_body(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
) -> Result<Vec<u8>, (u16, String)> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    let mut tmp = [0u8; 4096];

    loop {
        // Find chunk size line ending with \r\n
        let line_end = loop {
            if let Some(rel) = buf[cursor..].windows(2).position(|w| w == b"\r\n") {
                break cursor + rel;
            }
            if buf.len() > MAX_BODY_BYTES {
                return Err((413, "Chunked body too large".into()));
            }
            let n = stream
                .read(&mut tmp)
                .await
                .map_err(|e| (400, format!("read chunk error: {e}")))?;
            if n == 0 {
                return Err((400, "Unexpected EOF in chunked body".into()));
            }
            buf.extend_from_slice(&tmp[..n]);
        };

        let size_line = String::from_utf8_lossy(&buf[cursor..line_end]);
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let chunk_size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| (400, format!("Invalid chunk size: {size_hex}")))?;
        cursor = line_end + 2;

        if chunk_size == 0 {
            // Consume trailing CRLF after the 0-length terminator chunk
            while buf.len() < cursor + 2 {
                let n = stream
                    .read(&mut tmp)
                    .await
                    .map_err(|e| (400, format!("read chunk trailer error: {e}")))?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            if buf.len() >= cursor + 2 && &buf[cursor..cursor + 2] == b"\r\n" {
                cursor += 2;
            }
            break;
        }
        if out.len().saturating_add(chunk_size) > MAX_BODY_BYTES {
            return Err((413, "Chunked body too large".into()));
        }

        let need = cursor + chunk_size + 2;
        while buf.len() < need {
            let n = stream
                .read(&mut tmp)
                .await
                .map_err(|e| (400, format!("read chunk payload error: {e}")))?;
            if n == 0 {
                return Err((400, "Unexpected EOF in chunk payload".into()));
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        out.extend_from_slice(&buf[cursor..cursor + chunk_size]);
        cursor += chunk_size + 2;
    }

    buf.drain(..cursor.min(buf.len()));
    Ok(out)
}

async fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    keep_alive: bool,
) -> bool {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Internal Server Error",
    };
    let conn_line = if keep_alive {
        "Connection: keep-alive\r\nKeep-Alive: timeout=30"
    } else {
        "Connection: close"
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, X-API-Key, Mcp-Session-Id, MCP-Protocol-Version\r\n\
         {conn_line}\r\n\r\n",
        body.len()
    );
    if stream.write_all(head.as_bytes()).await.is_err() {
        return false;
    }
    if !body.is_empty() && stream.write_all(body).await.is_err() {
        return false;
    }
    stream.flush().await.is_ok()
}

async fn write_json(stream: &mut TcpStream, status: u16, val: &Value, keep_alive: bool) -> bool {
    let bytes = serde_json::to_vec(val).unwrap_or_else(|_| b"{}".to_vec());
    write_response(stream, status, "application/json; charset=utf-8", &bytes, keep_alive).await
}

fn check_auth(req: &ParsedHttpRequest, expected_token: &str) -> bool {
    let expected = expected_token.trim();
    if expected.is_empty() {
        return true;
    }
    if let Some(auth) = req.header("authorization") {
        if let Some(bearer) = auth.strip_prefix("Bearer ").or_else(|| auth.strip_prefix("bearer ")) {
            if bearer.trim() == expected {
                return true;
            }
        }
    }
    if let Some(key) = req.header("x-api-key") {
        if key.trim() == expected {
            return true;
        }
    }
    false
}

async fn handle_connection(mut stream: TcpStream, app: AppHandle, shared: SharedAuth) {
    let _ = stream.set_nodelay(true);
    let mut read_buf = Vec::with_capacity(4096);

    loop {
        let req = match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            read_http_request(&mut stream, &mut read_buf),
        )
        .await
        {
            Ok(Ok(Some(r))) => r,
            Ok(Ok(None)) => return,
            Ok(Err((code, msg))) => {
                let _ = write_json(&mut stream, code, &json!({ "error": msg }), false).await;
                return;
            }
            Err(_) => {
                // Idle keep-alive timeout
                return;
            }
        };

        let ka = req.keep_alive;
        let ok = dispatch_request(&mut stream, req, &app, &shared, ka).await;
        if !ka || !ok {
            return;
        }
    }
}

async fn dispatch_request(
    stream: &mut TcpStream,
    req: ParsedHttpRequest,
    app: &AppHandle,
    shared: &SharedAuth,
    ka: bool,
) -> bool {
    if req.method.eq_ignore_ascii_case("OPTIONS") {
        return write_response(stream, 204, "text/plain", b"", ka).await;
    }

    if req.method.eq_ignore_ascii_case("GET") && req.path == "/health" {
        return write_json(
            stream,
            200,
            &json!({
                "ok": true,
                "app": "smart-library",
                "version": env!("CARGO_PKG_VERSION"),
            }),
            ka,
        )
        .await;
    }

    let expected_token = shared.token.read().unwrap().clone();
    if !check_auth(&req, &expected_token) {
        return write_json(
            stream,
            401,
            &json!({ "error": "未授权：请提供正确的 Authorization: Bearer <token>" }),
            ka,
        )
        .await;
    }

    let policy = shared.policy.read().unwrap().clone();

    // MCP Streamable HTTP endpoint
    if req.path == "/mcp" {
        if req.method.eq_ignore_ascii_case("GET") {
            return write_json(
                stream,
                405,
                &json!({ "error": "Use POST /mcp for MCP JSON-RPC requests" }),
                ka,
            )
            .await;
        }
        if !req.method.eq_ignore_ascii_case("POST") {
            return write_json(stream, 405, &json!({ "error": "Method not allowed" }), ka).await;
        }
        let body_val: Value = match serde_json::from_slice(&req.body) {
            Ok(v) => v,
            Err(e) => {
                return write_json(
                    stream,
                    200,
                    &jsonrpc_error(Value::Null, -32700, format!("Parse error: {e}")),
                    ka,
                )
                .await;
            }
        };

        if let Some(batch) = body_val.as_array() {
            let mut responses = Vec::new();
            for item in batch {
                if let Some(resp) = handle_mcp_message(item, app, &policy).await {
                    responses.push(resp);
                }
            }
            return if responses.is_empty() {
                write_response(stream, 202, "application/json", b"", ka).await
            } else {
                write_json(stream, 200, &Value::Array(responses), ka).await
            };
        } else {
            return match handle_mcp_message(&body_val, app, &policy).await {
                Some(resp) => write_json(stream, 200, &resp, ka).await,
                None => write_response(stream, 202, "application/json", b"", ka).await,
            };
        }
    }

    // Lightweight REST endpoints for scripts & non-MCP local agents
    if req.method.eq_ignore_ascii_case("GET") && req.path == "/api/tools" {
        return write_json(
            stream,
            200,
            &json!({
                "allow_write": policy.allow_write,
                "tools": mcp_tool_list(policy.allow_write),
            }),
            ka,
        )
        .await;
    }

    if req.method.eq_ignore_ascii_case("POST") {
        if let Some(tool_name) = req.path.strip_prefix("/api/tools/") {
            let tool_name = tool_name.trim();
            if !tool_name.is_empty() && !tool_name.contains('/') {
                let args_str = String::from_utf8_lossy(&req.body).to_string();
                let res = ai_tools::exec_tool_with_policy(
                    tool_name,
                    &args_str,
                    app,
                    app.state::<AppState>(),
                    &policy,
                )
                .await;
                if res.mutated {
                    emit_mutated(app, &res.affected_lib, &res.affected_path);
                }
                let status = if res.ok { 200 } else { 400 };
                let val = serde_json::to_value(&res).unwrap_or(json!({ "ok": false }));
                return write_json(stream, status, &val, ka).await;
            }
        }
    }

    write_json(stream, 404, &json!({ "error": "Not found" }), ka).await
}

// ------------------------------------------------------------ MCP JSON-RPC 2.0

fn jsonrpc_ok(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        }
    })
}

fn mcp_tool_list(allow_write: bool) -> Vec<Value> {
    ai_tools::tool_definitions_for(allow_write)
        .into_iter()
        .map(|d| {
            json!({
                "name": d["name"],
                "description": d["description"],
                "inputSchema": d["parameters"],
            })
        })
        .collect()
}

fn emit_mutated(app: &AppHandle, lib: &Option<String>, path: &Option<String>) {
    let _ = app.emit(
        "library-mutated",
        json!({
            "lib": lib,
            "path": path,
        }),
    );
}

fn build_initialize_result(params: Option<&Value>, allow_write: bool) -> Value {
    let client_version = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("2025-03-26");

    let mode_hint = if allow_write {
        "当前已开启读写模式。写操作前请先调用 list_libraries 检查目标知识库的 writable 字段（仅 notes 库或读写模式的 folder 库可写入文档；修改前系统会自动创建快照，删除会进入回收站）。"
    } else {
        "当前为只读模式，仅开放检索与阅读工具。"
    };

    json!({
        "protocolVersion": client_version,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "smart-library",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": format!(
            "Smart Library 本地知识库 MCP 服务。使用建议：1) 查找书名/经名/篇名或目录请优先用 quick_search（无需全文索引，可直接跨库）；2) 全文内容检索用 search_library（仅对 index_ready=true 的库有效，未建索引的库请改用 quick_search 或 get_library_tree）；3) 仅需确认或预览正文开头时，read_document 建议传较小的 max_chars（如 1500~3000）；4) 可用 get_document_structure 提取文档科判大纲（含跨兄弟文档父子关联）与偈颂列表，用 get_document_book_refs 提取并定位文中《…》引用书名；5) 支持查本地词典 dict_lookup。{}",
            mode_hint
        )
    })
}

async fn handle_mcp_message(
    msg: &Value,
    app: &AppHandle,
    policy: &ToolPolicy,
) -> Option<Value> {
    let id = msg.get("id").cloned();
    let Some(method) = msg.get("method").and_then(|v| v.as_str()) else {
        return id.map(|id| jsonrpc_error(id, -32600, "Invalid Request: missing method"));
    };

    // Notifications (no id) require no response body
    if id.is_none() || method.starts_with("notifications/") {
        return None;
    }
    let id = id.unwrap();
    let params = msg.get("params");

    match method {
        "initialize" => Some(jsonrpc_ok(
            id,
            build_initialize_result(params, policy.allow_write),
        )),
        "ping" => Some(jsonrpc_ok(id, json!({}))),
        "tools/list" => Some(jsonrpc_ok(
            id,
            json!({
                "tools": mcp_tool_list(policy.allow_write),
            }),
        )),
        "tools/call" => {
            let Some(name) = params.and_then(|p| p.get("name")).and_then(|v| v.as_str()) else {
                return Some(jsonrpc_error(id, -32602, "Invalid params: missing tool name"));
            };
            let args_val = params
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            let args_str = serde_json::to_string(&args_val).unwrap_or_else(|_| "{}".into());

            let res = ai_tools::exec_tool_with_policy(
                name,
                &args_str,
                app,
                app.state::<AppState>(),
                policy,
            )
            .await;

            if res.mutated {
                emit_mutated(app, &res.affected_lib, &res.affected_path);
            }

            Some(jsonrpc_ok(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": res.output,
                    }],
                    "isError": !res.ok,
                }),
            ))
        }
        _ => Some(jsonrpc_error(
            id,
            -32601,
            format!("Method not found: {method}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_normalization() {
        let cfg = McpConfig {
            enabled: true,
            port: 0,
            allow_write: true,
            token: "  abc123  ".into(),
            denied_libs: vec!["a".into(), " ".into(), "a".into(), "b".into()],
        }
        .normalized();
        assert_eq!(cfg.port, DEFAULT_PORT);
        assert_eq!(cfg.token, "abc123");
        assert_eq!(cfg.denied_libs, vec!["a", "b"]);
        assert_eq!(cfg.url(), format!("http://127.0.0.1:{DEFAULT_PORT}/mcp"));
    }

    #[test]
    fn mcp_tool_list_filters_write_tools() {
        let ro = mcp_tool_list(false);
        let rw = mcp_tool_list(true);
        assert!(rw.len() > ro.len());
        assert!(ro.iter().any(|t| t["name"] == "search_library"));
        assert!(ro.iter().any(|t| t["name"] == "quick_search"));
        assert!(!ro.iter().any(|t| t["name"] == "create_note"));
        assert!(rw.iter().any(|t| t["name"] == "create_note"));
        assert!(rw.iter().all(|t| t.get("inputSchema").is_some()));
    }

    #[test]
    fn auth_header_checks_bearer_and_api_key() {
        let req = ParsedHttpRequest {
            method: "POST".into(),
            path: "/mcp".into(),
            headers: vec![("Authorization".into(), "Bearer secret-key".into())],
            body: vec![],
            keep_alive: true,
        };
        assert!(check_auth(&req, ""));
        assert!(check_auth(&req, "secret-key"));
        assert!(!check_auth(&req, "wrong-key"));

        let req2 = ParsedHttpRequest {
            method: "POST".into(),
            path: "/mcp".into(),
            headers: vec![("X-API-Key".into(), "secret-key".into())],
            body: vec![],
            keep_alive: true,
        };
        assert!(check_auth(&req2, "secret-key"));
    }

    #[test]
    fn initialize_result_contains_capabilities_and_version() {
        let res = build_initialize_result(Some(&json!({ "protocolVersion": "2024-11-05" })), true);
        assert_eq!(res["protocolVersion"], "2024-11-05");
        assert_eq!(res["serverInfo"]["name"], "smart-library");
        assert!(res["capabilities"].get("tools").is_some());
    }
}
