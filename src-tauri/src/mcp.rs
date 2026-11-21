use crate::state::{node_vram, AppState, Shared};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

pub async fn handle(shared: Shared, headers: HeaderMap, body: Value) -> Response {
    let method = body.get("method").and_then(|value| value.as_str()).unwrap_or("").to_string();
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    if method != "initialize" && !authorized(&headers, &shared) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32001, "message": "Murmuration MCP requires the gateway bearer token." }
            })),
        )
            .into_response();
    }

    match method.as_str() {
        "initialize" => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": { "name": "murmuration", "version": "0.1.0" },
                "capabilities": { "tools": {} }
            }
        }))
        .into_response(),
        "tools/list" => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": tool_specs() }
        }))
        .into_response(),
        "tools/call" => {
            let name = body
                .pointer("/params/name")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let args = body.pointer("/params/arguments").cloned().unwrap_or_else(|| json!({}));
            let payload = call_tool(shared, &name, &args).await;
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "content": [{ "type": "text", "text": payload }] }
            }))
            .into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("Unknown MCP method {method}") }
            })),
        )
            .into_response(),
    }
}

fn authorized(headers: &HeaderMap, shared: &Shared) -> bool {
    let token = {
        let state = crate::state::read_lock(shared);
        state.gateway_token.clone()
    };
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(|value| value == format!("Bearer {token}"))
        .unwrap_or(false)
}

fn tool_specs() -> Value {
    json!([
        { "name": "swarm_status", "description": "Read Murmuration node, VRAM, and route status.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "files_read", "description": "Read a text file inside the armed project root.", "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] } },
        { "name": "files_write", "description": "Write a text file inside the armed project root.", "inputSchema": { "type": "object", "properties": { "path": { "type": "string" }, "contents": { "type": "string" } }, "required": ["path", "contents"] } },
        { "name": "shell_exec", "description": "Run a shell command in the armed project root. The inspector must arm shell first.", "inputSchema": { "type": "object", "properties": { "command": { "type": "string" } }, "required": ["command"] } },
        { "name": "vector_query", "description": "Lexical overlap search across mesh memory. This is not a model embedding.", "inputSchema": { "type": "object", "properties": { "query": { "type": "string" }, "k": { "type": "integer" } }, "required": ["query"] } },
        { "name": "web_search", "description": "Search via TENSORMESH_SEARCH_URL when configured, and always include local mesh hits.", "inputSchema": { "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] } }
    ])
}

async fn call_tool(shared: Shared, name: &str, args: &Value) -> String {
    match name {
        "swarm_status" => {
            let state = crate::state::read_lock(&shared);
            status_text(&state)
        }
        "files_read" => {
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or("");
            let root = crate::state::read_lock(&shared).tool_root.clone();
            match resolve_under(&root, path) {
                Ok(full) => match std::fs::read(&full) {
                    Ok(bytes) => {
                        let bytes = if bytes.len() > 262_144 { &bytes[..262_144] } else { &bytes };
                        String::from_utf8_lossy(bytes).to_string()
                    }
                    Err(err) => format!("read failed: {err}"),
                },
                Err(err) => err,
            }
        }
        "files_write" => {
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or("");
            let contents = args.get("contents").and_then(|value| value.as_str()).unwrap_or("");
            if contents.len() > 524_288 {
                return "write refused: contents exceed 512 KB".into();
            }
            let root = crate::state::read_lock(&shared).tool_root.clone();
            match resolve_under(&root, path) {
                Ok(full) => {
                    if let Some(parent) = full.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match std::fs::write(&full, contents) {
                        Ok(()) => format!("wrote {}", full.display()),
                        Err(err) => format!("write failed: {err}"),
                    }
                }
                Err(err) => err,
            }
        }
        "shell_exec" => {
            let command = args.get("command").and_then(|value| value.as_str()).unwrap_or("").to_string();
            let (root, armed) = {
                let state = crate::state::read_lock(&shared);
                (state.tool_root.clone(), state.shell_armed)
            };
            if !armed {
                return "shell is disarmed. Arm it in the inspector before calling shell_exec.".into();
            }
            if root.is_empty() {
                return "set a project root before running shell commands.".into();
            }
            if command.trim().is_empty() || command.len() > 2000 || command.contains('\0') {
                return "command was empty, too long, or contained a null.".into();
            }
            let root_owned = root.clone();
            let joined = tokio::task::spawn_blocking(move || run_shell(&root_owned, &command)).await;
            match joined {
                Ok(text) => text,
                Err(err) => format!("shell task failed: {err}"),
            }
        }
        "vector_query" => {
            let query = args.get("query").and_then(|value| value.as_str()).unwrap_or("");
            let k = args.get("k").and_then(|value| value.as_u64()).unwrap_or(5).clamp(1, 12) as usize;
            let state = crate::state::read_lock(&shared);
            lexical_query(&state, query, k)
        }
        "web_search" => {
            let query = args.get("query").and_then(|value| value.as_str()).unwrap_or("").to_string();
            let local = {
                let state = crate::state::read_lock(&shared);
                lexical_query(&state, &query, 4)
            };
            let remote = match std::env::var("TENSORMESH_SEARCH_URL") {
                Ok(base) if base.starts_with("http://") || base.starts_with("https://") => {
                    let url = format!("{}{}", base.trim_end_matches('/'), format!("?q={}", urlencoding_min(&query)));
                    match reqwest::Client::new().get(url).timeout(Duration::from_secs(8)).send().await {
                        Ok(response) => {
                            let status = response.status();
                            let text = response.text().await.unwrap_or_default();
                            format!("provider status {status}\n{}", clip(&text, 1500))
                        }
                        Err(err) => format!("provider request failed: {err}"),
                    }
                }
                _ => "No search provider is configured. Set TENSORMESH_SEARCH_URL to an HTTP endpoint that accepts ?q=.".into(),
            };
            format!("Local mesh memory\n{local}\n\nWeb provider\n{remote}")
        }
        other => format!("Unknown tool {other}"),
    }
}

fn status_text(state: &AppState) -> String {
    let mut lines = vec![format!(
        "Murmuration {} mode on :{}. mesh={}",
        state.mode, state.gateway_port, state.mesh_source
    )];
    for node in &state.nodes {
        let (total, alloc) = node_vram(node);
        lines.push(format!(
            "{} status={} relay={} ping={}ms vram={:.1}/{:.1}GB tok={:.0}/s role={}",
            node.hostname,
            node.status,
            node.relay,
            node.ping_ms,
            alloc,
            total,
            node.tokens_per_sec,
            node.role.clone().unwrap_or_else(|| "none".into())
        ));
    }
    lines.join("\n")
}

fn lexical_query(state: &AppState, query: &str, k: usize) -> String {
    let needles: Vec<String> = query
        .split_whitespace()
        .map(|word| word.to_ascii_lowercase())
        .filter(|word| word.len() > 2)
        .collect();
    let mut docs = Vec::new();
    for model in &state.models {
        docs.push((format!("model {}", model.name), model.name.clone()));
    }
    for line in state.logs.iter().rev().take(40) {
        docs.push((format!("log {}", line.source), line.message.clone()));
    }
    for entry in state.traffic.iter().rev().take(12) {
        docs.push(("traffic".into(), format!("{} {}", entry.decision, entry.request_preview)));
    }
    let mut scored: Vec<(i32, String, String)> = docs
        .into_iter()
        .map(|(kind, text)| {
            let hay = text.to_ascii_lowercase();
            let score = needles.iter().filter(|needle| hay.contains(needle.as_str())).count() as i32;
            (score, kind, text)
        })
        .filter(|(score, _, _)| *score > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    if scored.is_empty() {
        return "No lexical overlap in mesh memory. This index is not a vector model.".into();
    }
    scored
        .into_iter()
        .take(k)
        .map(|(score, kind, text)| format!("{kind} score={score} {}", clip(&text, 180)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolve_under(root: &str, path: &str) -> Result<PathBuf, String> {
    if root.trim().is_empty() {
        return Err("set a project root in the inspector before using file tools.".into());
    }
    if path.contains('\0') {
        return Err("path contains a null.".into());
    }
    let root = PathBuf::from(root);
    let joined = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root.join(path)
    };
    let root_canon = std::fs::canonicalize(&root).map_err(|err| format!("project root is not available: {err}"))?;
    let check = if joined.exists() {
        std::fs::canonicalize(&joined).map_err(|err| err.to_string())?
    } else if let Some(parent) = joined.parent() {
        let parent = std::fs::canonicalize(parent).map_err(|err| format!("parent is not available: {err}"))?;
        parent.join(joined.file_name().unwrap_or_default())
    } else {
        return Err("path is not inside the project root.".into());
    };
    if !check.starts_with(&root_canon) {
        return Err("path escapes the project root.".into());
    }
    Ok(check)
}

fn run_shell(root: &str, command: &str) -> String {
    let child = if cfg!(windows) {
        crate::discovery::command("cmd")
            .args(["/C", command])
            .current_dir(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    } else {
        crate::discovery::command("sh")
            .args(["-c", command])
            .current_dir(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let mut child = match child {
        Ok(child) => child,
        Err(err) => return format!("shell failed to start: {err}"),
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                let mut err = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut out);
                }
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr.read_to_string(&mut err);
                }
                return format!("exit {}\n{}{}", status.code().unwrap_or(-1), clip(&out, 8000), clip(&err, 4000));
            }
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(8) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return "shell timed out after 8s".into();
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            Err(err) => return format!("shell wait failed: {err}"),
        }
    }
}

fn clip(value: &str, max: usize) -> String {
    let mut out: String = value.chars().take(max).collect();
    if value.chars().count() > max {
        out.push('…');
    }
    out
}

fn urlencoding_min(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn note_policy(state: &mut AppState) {
    state.log(
        "gateway",
        "mcp",
        "info",
        format!(
            "Tool root {} · shell {}",
            if state.tool_root.is_empty() { "unset" } else { state.tool_root.as_str() },
            if state.shell_armed { "armed" } else { "disarmed" }
        ),
    );
}
