//! LLM streaming client (OpenAI-compatible & Anthropic Messages API) over the
//! existing `reqwest` + `rustls` stack. Runs in the Tauri backend to avoid
//! WebView CORS restrictions and supports mid-stream cancellation (`ai_abort`).

use futures_util::future::{AbortHandle, Abortable};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Emitter};

static ABORTS: LazyLock<Mutex<HashMap<String, AbortHandle>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Deserialize)]
pub struct AiModelConfig {
    /// `"openai"` (OpenAI-compatible) or `"anthropic"`
    pub format: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<AiToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiStreamEvent {
    pub req_id: String,
    pub delta: String,
    pub reasoning: String,
    pub tool_calls: Vec<AiToolCall>,
    pub done: bool,
    pub error: Option<String>,
}

fn endpoint_url(format: &str, base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if format == "anthropic" {
        if base.ends_with("/messages") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{}/messages", base)
        } else {
            format!("{}/v1/messages", base)
        }
    } else if base.ends_with("/chat/completions") {
        base.to_string()
    } else if base.ends_with("/v1") || base.ends_with("/v3") || base.ends_with("/v4") {
        format!("{}/chat/completions", base)
    } else {
        format!("{}/v1/chat/completions", base)
    }
}

fn models_url(base_url: &str) -> String {
    let mut base = base_url.trim().trim_end_matches('/');
    if let Some(stripped) = base.strip_suffix("/chat/completions") {
        base = stripped.trim_end_matches('/');
    } else if let Some(stripped) = base.strip_suffix("/messages") {
        base = stripped.trim_end_matches('/');
    }
    if base.ends_with("/v1") || base.ends_with("/v3") || base.ends_with("/v4") {
        format!("{}/models", base)
    } else {
        format!("{}/v1/models", base)
    }
}

pub async fn fetch_models(
    format: String,
    base_url: String,
    api_key: String,
) -> Result<Vec<String>, String> {
    if base_url.trim().is_empty() {
        return Err("请先填写大模型接口地址（Base URL）".into());
    }
    let url = models_url(&base_url);
    let client = build_client(Some(15))?;
    let mut req = client.get(&url);
    let key = api_key.trim();
    if format == "anthropic" {
        if !key.is_empty() {
            req = req.header("x-api-key", key);
        }
        req = req.header("anthropic-version", "2023-06-01");
    } else if !key.is_empty() {
        req = req.bearer_auth(key);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("获取模型列表失败：{}", e))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("读取模型列表响应失败：{}", e))?;
    if !status.is_success() {
        let preview: String = body.chars().take(240).collect();
        return Err(format!("获取模型列表失败 ({}): {}", status.as_u16(), preview));
    }

    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("解析模型列表 JSON 失败：{}", e))?;
    let mut ids = Vec::new();
    if let Some(arr) = json.get("data").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                let id = id.trim();
                if !id.is_empty() {
                    ids.push(id.to_string());
                }
            }
        }
    } else if let Some(arr) = json.get("models").and_then(|v| v.as_array()) {
        for item in arr {
            let id = item
                .as_str()
                .or_else(|| item.get("id").and_then(|v| v.as_str()))
                .or_else(|| item.get("name").and_then(|v| v.as_str()));
            if let Some(id) = id {
                let id = id.trim();
                if !id.is_empty() {
                    ids.push(id.to_string());
                }
            }
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn validate_config(cfg: &AiModelConfig) -> Result<(), String> {
    if cfg.base_url.trim().is_empty() {
        return Err("请先在设置中填写大模型接口地址（Base URL）".into());
    }
    if cfg.model.trim().is_empty() {
        return Err("请先在设置中填写模型名称（Model）".into());
    }
    Ok(())
}

/// Cancel an active streaming request by its `req_id`.
pub fn abort(req_id: &str) {
    if let Some(h) = ABORTS.lock().unwrap().remove(req_id) {
        h.abort();
    }
}

static INIT_CRYPTO: std::sync::Once = std::sync::Once::new();

fn build_client(timeout_secs: Option<u64>) -> Result<reqwest::Client, String> {
    INIT_CRYPTO.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let mut b = reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(20));
    if let Some(secs) = timeout_secs {
        b = b.timeout(std::time::Duration::from_secs(secs));
    }
    b.build().map_err(|e| format!("无法创建网络客户端：{}", e))
}

/// Test a model configuration with a minimal prompt.
pub async fn test_connection(cfg: AiModelConfig) -> Result<String, String> {
    validate_config(&cfg)?;
    let client = build_client(Some(25))?;

    let url = endpoint_url(&cfg.format, &cfg.base_url);
    let key = cfg.api_key.trim();

    let req = if cfg.format == "anthropic" {
        let mut r = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("anthropic-version", "2023-06-01");
        if !key.is_empty() {
            r = r.header("x-api-key", key);
        }
        r.json(&json!({
            "model": cfg.model.trim(),
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "请回复：连接成功"}]
        }))
    } else {
        let mut r = client.post(&url).header("Content-Type", "application/json");
        if !key.is_empty() {
            r = r.header("Authorization", format!("Bearer {}", key));
        }
        r.json(&json!({
            "model": cfg.model.trim(),
            "max_tokens": 32,
            "stream": false,
            "messages": [{"role": "user", "content": "请回复：连接成功"}]
        }))
    };

    let resp = req.send().await.map_err(|e| format!("请求失败：{}", e))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let brief: String = body.chars().take(300).collect();
        return Err(format!("HTTP {}：{}", status.as_u16(), brief));
    }
    let v: Value = serde_json::from_str(&body).map_err(|e| format!("返回内容不是 JSON：{}", e))?;
    let reply = if cfg.format == "anthropic" {
        v["content"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|c| c["text"].as_str())
            .unwrap_or("OK")
    } else {
        v["choices"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|c| c["message"]["content"].as_str())
            .unwrap_or("OK")
    };
    Ok(format!("连接成功（模型回复：{}）", reply.trim()))
}

/// Generate a concise title (<= 16 chars) from the first exchange.
pub async fn summarize_title(
    cfg: AiModelConfig,
    user_text: String,
    assistant_text: String,
) -> Result<String, String> {
    validate_config(&cfg)?;
    let client = build_client(Some(15))?;
    let url = endpoint_url(&cfg.format, &cfg.base_url);
    let key = cfg.api_key.trim();
    let u: String = user_text.chars().take(400).collect();
    let a: String = assistant_text.chars().take(400).collect();
    let prompt = format!(
        "请用不超过14个字的简短标题概括以下对话主题（只输出标题本身，不要标点符号、引号或解释）：\n用户：{}\n助手：{}",
        u, a
    );

    let req = if cfg.format == "anthropic" {
        let mut r = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("anthropic-version", "2023-06-01");
        if !key.is_empty() {
            r = r.header("x-api-key", key);
        }
        r.json(&json!({
            "model": cfg.model.trim(),
            "max_tokens": 32,
            "messages": [{"role": "user", "content": prompt}]
        }))
    } else {
        let mut r = client.post(&url).header("Content-Type", "application/json");
        if !key.is_empty() {
            r = r.header("Authorization", format!("Bearer {}", key));
        }
        r.json(&json!({
            "model": cfg.model.trim(),
            "max_tokens": 32,
            "messages": [{"role": "user", "content": prompt}]
        }))
    };

    let resp = req.send().await.map_err(|e| format!("请求失败：{}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let v: Value = resp.json().await.map_err(|e| format!("解析失败：{}", e))?;
    let raw = if cfg.format == "anthropic" {
        v["content"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|c| c["text"].as_str())
            .unwrap_or("")
    } else {
        v["choices"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|c| c["message"]["content"].as_str())
            .unwrap_or("")
    };
    let cleaned = raw
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\'' | '“' | '”' | '「' | '」' | '《' | '》' | '。' | '：' | ':'))
        .lines()
        .next()
        .unwrap_or("")
        .trim();
    let short: String = cleaned.chars().take(18).collect();
    if short.is_empty() {
        Err("空标题".into())
    } else {
        Ok(short)
    }
}

/// Stream a chat completion and emit `ai-stream` events to the frontend.
pub async fn stream_chat(
    app: AppHandle,
    req_id: String,
    cfg: AiModelConfig,
    system: Option<String>,
    messages: Vec<AiMessage>,
    reasoning_effort: Option<String>,
    tools: Option<Vec<Value>>,
) -> Result<(), String> {
    validate_config(&cfg)?;
    let (handle, reg) = AbortHandle::new_pair();
    ABORTS.lock().unwrap().insert(req_id.clone(), handle);

    let res = match Abortable::new(
        stream_inner(&app, &req_id, &cfg, system, messages, reasoning_effort, tools),
        reg,
    )
    .await
    {
        Ok(r) => r,
        Err(_aborted) => Ok(Vec::new()),
    };
    ABORTS.lock().unwrap().remove(&req_id);

    match res {
        Ok(tool_calls) => {
            let _ = app.emit(
                "ai-stream",
                AiStreamEvent {
                    req_id,
                    delta: String::new(),
                    reasoning: String::new(),
                    tool_calls,
                    done: true,
                    error: None,
                },
            );
            Ok(())
        }
        Err(e) => {
            let _ = app.emit(
                "ai-stream",
                AiStreamEvent {
                    req_id,
                    delta: String::new(),
                    reasoning: String::new(),
                    tool_calls: Vec::new(),
                    done: true,
                    error: Some(e.clone()),
                },
            );
            Err(e)
        }
    }
}

async fn stream_inner(
    app: &AppHandle,
    req_id: &str,
    cfg: &AiModelConfig,
    system: Option<String>,
    messages: Vec<AiMessage>,
    reasoning_effort: Option<String>,
    tools: Option<Vec<Value>>,
) -> Result<Vec<AiToolCall>, String> {
    let client = build_client(None)?;

    let url = endpoint_url(&cfg.format, &cfg.base_url);
    let key = cfg.api_key.trim();
    let sys = system.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let effort = reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let active_tools = tools.filter(|t| !t.is_empty());

    let req = if cfg.format == "anthropic" {
        let mut r = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("anthropic-version", "2023-06-01");
        if !key.is_empty() {
            r = r.header("x-api-key", key);
        }
        let mut msgs: Vec<Value> = Vec::new();
        for m in messages.into_iter().filter(|m| m.role != "system") {
            if m.role == "assistant" && m.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
                let mut blocks: Vec<Value> = Vec::new();
                if !m.content.is_empty() {
                    blocks.push(json!({ "type": "text", "text": m.content }));
                }
                for tc in m.tool_calls.unwrap_or_default() {
                    let input: Value = serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": input,
                    }));
                }
                msgs.push(json!({ "role": "assistant", "content": blocks }));
            } else if m.role == "tool" {
                let block = json!({
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id.unwrap_or_default(),
                    "content": m.content,
                });
                if let Some(last) = msgs.last_mut() {
                    if last["role"] == "user" && last["content"].is_array() {
                        last["content"].as_array_mut().unwrap().push(block);
                        continue;
                    }
                }
                msgs.push(json!({ "role": "user", "content": [block] }));
            } else {
                msgs.push(json!({ "role": m.role, "content": m.content }));
            }
        }
        let mut max_tokens: u64 = 8192;
        let mut body = json!({
            "model": cfg.model.trim(),
            "max_tokens": max_tokens,
            "stream": true,
            "messages": msgs,
        });
        if let Some(tools_list) = active_tools {
            let mapped: Vec<Value> = tools_list
                .into_iter()
                .map(|t| {
                    json!({
                        "name": t["name"],
                        "description": t["description"],
                        "input_schema": t["parameters"],
                    })
                })
                .collect();
            body["tools"] = json!(mapped);
        }
        if let Some(eff) = effort.as_deref() {
            let budget: u64 = match eff {
                "low" => 1024,
                "medium" => 4096,
                "high" => 10240,
                "xhigh" => 16384,
                "ultra" | "max" => 32768,
                _ => 4096,
            };
            if max_tokens <= budget {
                max_tokens = budget + 4096;
                body["max_tokens"] = json!(max_tokens);
            }
            body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
        }
        if let Some(s) = sys {
            body["system"] = json!(s);
        }
        r.json(&body)
    } else {
        let mut r = client.post(&url).header("Content-Type", "application/json");
        if !key.is_empty() {
            r = r.header("Authorization", format!("Bearer {}", key));
        }
        let mut msgs: Vec<Value> = Vec::with_capacity(messages.len() + 1);
        if let Some(s) = sys {
            msgs.push(json!({ "role": "system", "content": s }));
        }
        for m in messages {
            if m.role == "assistant" && m.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
                let calls: Vec<Value> = m
                    .tool_calls
                    .unwrap_or_default()
                    .into_iter()
                    .map(|tc| {
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": tc.arguments,
                            }
                        })
                    })
                    .collect();
                msgs.push(json!({
                    "role": "assistant",
                    "content": if m.content.is_empty() { Value::Null } else { json!(m.content) },
                    "tool_calls": calls,
                }));
            } else if m.role == "tool" {
                msgs.push(json!({
                    "role": "tool",
                    "tool_call_id": m.tool_call_id.unwrap_or_default(),
                    "content": m.content,
                }));
            } else {
                msgs.push(json!({ "role": m.role, "content": m.content }));
            }
        }
        let mut body = json!({
            "model": cfg.model.trim(),
            "stream": true,
            "messages": msgs,
        });
        if let Some(tools_list) = active_tools {
            let mapped: Vec<Value> = tools_list
                .into_iter()
                .map(|t| json!({ "type": "function", "function": t }))
                .collect();
            body["tools"] = json!(mapped);
        }
        if let Some(eff) = effort {
            body["reasoning_effort"] = json!(eff);
        }
        r.json(&body)
    };

    let resp = req.send().await.map_err(|e| format!("网络请求失败：{}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let brief: String = body.chars().take(400).collect();
        return Err(format!("请求失败（HTTP {}）：{}", status.as_u16(), brief));
    }

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut tool_calls: BTreeMap<usize, AiToolCall> = BTreeMap::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("读取响应流出错：{}", e))?;
        buf.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(nl) = buf.find('\n') {
            let line = buf[..nl].trim_end_matches('\r').to_string();
            buf.drain(..=nl);
            let data = if let Some(d) = line.strip_prefix("data: ") {
                d.trim()
            } else if let Some(d) = line.strip_prefix("data:") {
                d.trim()
            } else {
                continue;
            };
            if data.is_empty() || data == "[DONE]" {
                if data == "[DONE]" {
                    return Ok(tool_calls
                        .into_values()
                        .filter(|c| !c.name.is_empty())
                        .collect());
                }
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if cfg.format == "anthropic" {
                if v["type"] == "message_stop" {
                    return Ok(tool_calls
                        .into_values()
                        .filter(|c| !c.name.is_empty())
                        .collect());
                }
                if v["type"] == "error" {
                    let msg = v["error"]["message"].as_str().unwrap_or("Anthropic 流式错误");
                    return Err(msg.to_string());
                }
                if v["type"] == "content_block_start" && v["content_block"]["type"] == "tool_use" {
                    let idx = v["index"].as_u64().unwrap_or(0) as usize;
                    let id = v["content_block"]["id"].as_str().unwrap_or("").to_string();
                    let name = v["content_block"]["name"].as_str().unwrap_or("").to_string();
                    tool_calls.insert(
                        idx,
                        AiToolCall {
                            id,
                            name,
                            arguments: String::new(),
                        },
                    );
                }
                if v["type"] == "content_block_delta" {
                    if v["delta"]["type"] == "input_json_delta" {
                        let idx = v["index"].as_u64().unwrap_or(0) as usize;
                        if let Some(partial) = v["delta"]["partial_json"].as_str() {
                            if let Some(entry) = tool_calls.get_mut(&idx) {
                                entry.arguments.push_str(partial);
                            }
                        }
                    } else {
                        let delta = v["delta"]["text"].as_str().unwrap_or("");
                        let reasoning = v["delta"]["thinking"].as_str().unwrap_or("");
                        if !delta.is_empty() || !reasoning.is_empty() {
                            let _ = app.emit(
                                "ai-stream",
                                AiStreamEvent {
                                    req_id: req_id.to_string(),
                                    delta: delta.to_string(),
                                    reasoning: reasoning.to_string(),
                                    tool_calls: Vec::new(),
                                    done: false,
                                    error: None,
                                },
                            );
                        }
                    }
                }
            } else {
                if let Some(err_msg) = v["error"]["message"].as_str() {
                    return Err(err_msg.to_string());
                }
                let choice = &v["choices"][0]["delta"];
                let delta = choice["content"].as_str().unwrap_or("");
                let reasoning = choice["reasoning_content"].as_str().unwrap_or("");
                if !delta.is_empty() || !reasoning.is_empty() {
                    let _ = app.emit(
                        "ai-stream",
                        AiStreamEvent {
                            req_id: req_id.to_string(),
                            delta: delta.to_string(),
                            reasoning: reasoning.to_string(),
                            tool_calls: Vec::new(),
                            done: false,
                            error: None,
                        },
                    );
                }
                if let Some(arr) = choice["tool_calls"].as_array() {
                    for tc in arr {
                        let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                        let entry = tool_calls.entry(idx).or_insert_with(|| AiToolCall {
                            id: format!("call_{}", idx),
                            name: String::new(),
                            arguments: String::new(),
                        });
                        if let Some(id) = tc["id"].as_str() {
                            if !id.is_empty() {
                                entry.id = id.to_string();
                            }
                        }
                        if let Some(name) = tc["function"]["name"].as_str() {
                            entry.name.push_str(name);
                        }
                        if let Some(args) = tc["function"]["arguments"].as_str() {
                            entry.arguments.push_str(args);
                        }
                    }
                }
            }
        }
    }
    Ok(tool_calls
        .into_values()
        .filter(|c| !c.name.is_empty())
        .collect())
}
