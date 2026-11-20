//! Stdio-to-HTTP bridge for MCP clients that only speak stdio transport.
//!
//! Reads newline-delimited JSON-RPC 2.0 messages from stdin, forwards each one
//! via HTTP/1.1 POST to the running Smart Library desktop app's `/mcp`
//! endpoint, and writes JSON-RPC responses back to stdout.

use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const DEFAULT_URL: &str = "http://127.0.0.1:27123/mcp";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_http_url(raw: &str) -> Result<ParsedUrl, String> {
    let rest = raw
        .trim()
        .strip_prefix("http://")
        .ok_or_else(|| format!("仅支持 http:// 本地地址，收到：{raw}"))?;
    let (host_port, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], rest[idx..].to_string()),
        None => (rest, "/mcp".to_string()),
    };
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => {
            let port = p
                .parse::<u16>()
                .map_err(|_| format!("无效端口号：{p}"))?;
            (h.to_string(), port)
        }
        None => (host_port.to_string(), 80),
    };
    if host.is_empty() {
        return Err(format!("无效主机名：{raw}"));
    }
    Ok(ParsedUrl { host, port, path })
}

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn post_json(url: &ParsedUrl, token: &str, payload: &[u8]) -> Result<HttpResponse, String> {
    let addr = format!("{}:{}", url.host, url.port);
    let mut stream = TcpStream::connect(&addr).map_err(|e| {
        format!(
            "无法连接到 Smart Library 本地 MCP 服务（http://{}{}）：{}。请确认 Smart Library 桌面程序正在运行，且已在「设置 → 本地 Agent」中开启服务。",
            addr, url.path, e
        )
    })?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));

    let mut req = format!(
        "POST {} HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Type: application/json\r\n\
         Accept: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n",
        url.path,
        addr,
        payload.len()
    );
    if !token.is_empty() {
        req.push_str(&format!("Authorization: Bearer {}\r\n", token));
    }
    req.push_str("\r\n");

    stream
        .write_all(req.as_bytes())
        .and_then(|_| stream.write_all(payload))
        .and_then(|_| stream.flush())
        .map_err(|e| format!("发送请求到 Smart Library 失败：{e}"))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("读取 Smart Library 响应失败：{e}"))?;

    parse_http_response(&raw)
}

fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, String> {
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "无效的 HTTP 响应（缺少头部结束标记）".to_string())?;
    let head = String::from_utf8_lossy(&raw[..header_end]);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("无法解析 HTTP 状态行：{status_line}"))?;

    let mut is_chunked = false;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("transfer-encoding")
                && v.to_ascii_lowercase().contains("chunked")
            {
                is_chunked = true;
            }
        }
    }

    let body_raw = &raw[header_end + 4..];
    let body = if is_chunked {
        decode_chunked(body_raw)?
    } else {
        body_raw.to_vec()
    };

    Ok(HttpResponse { status, body })
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let Some(pos) = data.windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let size_str = String::from_utf8_lossy(&data[..pos]);
        let hex = size_str.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(hex, 16)
            .map_err(|_| format!("无效的 chunk 长度：{hex}"))?;
        data = &data[pos + 2..];
        if size == 0 {
            break;
        }
        if data.len() < size {
            return Err("chunked 响应体不完整".into());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size..];
        if data.starts_with(b"\r\n") {
            data = &data[2..];
        }
    }
    Ok(out)
}

fn jsonrpc_err_line(id: Option<Value>, code: i64, msg: String) -> Option<String> {
    let id = id?;
    let v = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": msg,
        }
    });
    serde_json::to_string(&v).ok()
}

fn forward_line(url: &ParsedUrl, token: &str, line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let req_id = serde_json::from_str::<Value>(trimmed)
        .ok()
        .and_then(|v| v.get("id").cloned());

    match post_json(url, token, trimmed.as_bytes()) {
        Ok(resp) => {
            if resp.status == 202 || resp.status == 204 || resp.body.is_empty() {
                return None;
            }
            if (200..300).contains(&resp.status) {
                if let Ok(v) = serde_json::from_slice::<Value>(&resp.body) {
                    return serde_json::to_string(&v).ok();
                }
                let text = String::from_utf8_lossy(&resp.body).trim().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            } else {
                let detail = String::from_utf8_lossy(&resp.body).trim().to_string();
                jsonrpc_err_line(
                    req_id,
                    -32000,
                    format!("Smart Library MCP 服务返回 HTTP {}：{}", resp.status, detail),
                )
            }
        }
        Err(e) => jsonrpc_err_line(req_id, -32000, e),
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut url_str = std::env::var("SMART_LIBRARY_MCP_URL").unwrap_or_else(|_| DEFAULT_URL.into());
    let mut token = std::env::var("SMART_LIBRARY_MCP_TOKEN").unwrap_or_default();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--url" => {
                i += 1;
                url_str = args.get(i).ok_or("--url 需要指定地址")?.clone();
            }
            "--token" => {
                i += 1;
                token = args.get(i).ok_or("--token 需要指定令牌")?.clone();
            }
            "--help" | "-h" => {
                eprintln!(
                    "usage: smart-library mcp-bridge [--url http://127.0.0.1:27123/mcp] [--token <token>]\n\
                     env:   SMART_LIBRARY_MCP_URL, SMART_LIBRARY_MCP_TOKEN"
                );
                return Ok(());
            }
            other => return Err(format!("未知参数：{other}")),
        }
        i += 1;
    }

    let url = parse_http_url(&url_str)?;
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("读取 stdin 失败：{e}"))?;
        if let Some(out) = forward_line(&url, &token, &line) {
            writeln!(stdout, "{out}").map_err(|e| format!("写入 stdout 失败：{e}"))?;
            stdout.flush().map_err(|e| format!("刷新 stdout 失败：{e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn parses_local_http_urls() {
        let u = parse_http_url("http://127.0.0.1:27123/mcp").unwrap();
        assert_eq!(u.host, "127.0.0.1");
        assert_eq!(u.port, 27123);
        assert_eq!(u.path, "/mcp");

        let u2 = parse_http_url("http://localhost:9000").unwrap();
        assert_eq!(u2.path, "/mcp");
        assert!(parse_http_url("https://127.0.0.1:27123/mcp").is_err());
    }

    #[test]
    fn decodes_chunked_response() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"ok\": \r\n5\r\ntrue}\r\n0\r\n\r\n";
        let resp = parse_http_response(raw).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(String::from_utf8(resp.body).unwrap(), "{\"ok\": true}");
    }

    #[test]
    fn forwards_jsonrpc_and_handles_notification() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            for idx in 0..2 {
                let (mut sock, _) = listener.accept().unwrap();
                let mut buf = [0u8; 2048];
                let n = sock.read(&mut buf).unwrap();
                let req = String::from_utf8_lossy(&buf[..n]);
                assert!(req.contains("Authorization: Bearer tok123"));
                if idx == 0 {
                    let body = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    sock.write_all(resp.as_bytes()).unwrap();
                } else {
                    sock.write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
                        .unwrap();
                }
            }
        });

        let url = parse_http_url(&format!("http://127.0.0.1:{port}/mcp")).unwrap();
        let out = forward_line(&url, "tok123", r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#)
            .expect("should return response");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["ok"], true);

        let notif = forward_line(
            &url,
            "tok123",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        );
        assert!(notif.is_none());

        handle.join().unwrap();
    }
}
