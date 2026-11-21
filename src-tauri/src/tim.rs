use crate::discovery;
use crate::state::{read_lock, Shared};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;

const ALLOWED_REPOS: &[&str] = &[
    "langflow-ai/langflow",
    "FlowiseAI/FlowiseDocs",
    "FlowiseAI/Flowise",
    "n8n-io/n8n-workflows",
    "Zie619/n8n-workflows",
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogHit {
    pub platform: String,
    pub name: String,
    pub path: String,
    pub raw_url: String,
}

struct CatalogRoot {
    platform: &'static str,
    repo: &'static str,
    path: &'static str,
    branch: &'static str,
}

const CATALOGS: &[CatalogRoot] = &[
    CatalogRoot {
        platform: "langflow",
        repo: "langflow-ai/langflow",
        path: "src/backend/base/langflow/initial_setup/starter_projects",
        branch: "main",
    },
    CatalogRoot {
        platform: "flowise",
        repo: "FlowiseAI/FlowiseDocs",
        path: "en/.gitbook/assets",
        branch: "main",
    },
    CatalogRoot {
        platform: "n8n",
        repo: "Zie619/n8n-workflows",
        path: "workflows",
        branch: "main",
    },
];

pub async fn list_catalog(platform: &str) -> Result<Vec<CatalogHit>, String> {
    if platform == "n8n" {
        return list_n8n_tree().await;
    }
    let root = CATALOGS.iter().find(|item| item.platform == platform).ok_or_else(|| format!("Unknown catalog {platform}"))?;
    let url = format!("https://api.github.com/repos/{}/contents/{}?ref={}", root.repo, root.path, root.branch);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "murmuration-catalog")
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("{} catalog returned HTTP {}.", root.repo, response.status()));
    }
    let items: Vec<Value> = response.json().await.map_err(|err| err.to_string())?;
    let mut hits = Vec::new();
    for item in items {
        if item.get("type").and_then(|value| value.as_str()) != Some("file") {
            continue;
        }
        let name = item.get("name").and_then(|value| value.as_str()).unwrap_or("");
        if !name.to_ascii_lowercase().ends_with(".json") {
            continue;
        }
        let path = item.get("path").and_then(|value| value.as_str()).unwrap_or(name);
        let raw = item
            .get("download_url")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("https://raw.githubusercontent.com/{}/{}/{}", root.repo, root.branch, path));
        hits.push(CatalogHit {
            platform: platform.to_string(),
            name: name.trim_end_matches(".json").replace(['-', '_'], " "),
            path: path.to_string(),
            raw_url: raw,
        });
    }
    hits.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    Ok(hits)
}

async fn list_n8n_tree() -> Result<Vec<CatalogHit>, String> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.github.com/repos/Zie619/n8n-workflows/git/trees/main?recursive=1")
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "murmuration-catalog")
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("n8n catalog returned HTTP {}.", response.status()));
    }
    let body: Value = response.json().await.map_err(|err| err.to_string())?;
    let tree = body.get("tree").and_then(|value| value.as_array()).ok_or("n8n catalog tree was empty.")?;
    let mut hits = Vec::new();
    for item in tree {
        if item.get("type").and_then(|value| value.as_str()) != Some("blob") {
            continue;
        }
        let path = item.get("path").and_then(|value| value.as_str()).unwrap_or("");
        if !path.starts_with("workflows/") || !path.to_ascii_lowercase().ends_with(".json") {
            continue;
        }
        let file = path.rsplit('/').next().unwrap_or(path);
        let folder = path.split('/').nth(1).unwrap_or("");
        let title = file.trim_end_matches(".json").replace(['-', '_'], " ");
        hits.push(CatalogHit {
            platform: "n8n".into(),
            name: format!("{folder} {title}"),
            path: path.to_string(),
            raw_url: format!("https://raw.githubusercontent.com/Zie619/n8n-workflows/main/{path}"),
        });
    }
    hits.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    Ok(hits)
}

pub async fn fetch_workflow(raw_url: &str) -> Result<String, String> {
    let parsed = assert_raw_github(raw_url)?;
    let client = reqwest::Client::new();
    let response = client
        .get(parsed)
        .header("User-Agent", "murmuration-catalog")
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Could not download workflow (HTTP {}).", response.status()));
    }
    let text = response.text().await.map_err(|err| err.to_string())?;
    if text.len() > 2_000_000 {
        return Err("Workflow file is too large.".into());
    }
    Ok(text)
}

fn assert_raw_github(url: &str) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "Invalid URL.".to_string())?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("raw.githubusercontent.com") {
        return Err("Remote import is limited to raw.githubusercontent.com.".into());
    }
    let parts: Vec<&str> = parsed.path().split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() < 4 {
        return Err("Malformed GitHub raw URL.".into());
    }
    let repo = format!("{}/{}", parts[0], parts[1]);
    if !ALLOWED_REPOS.contains(&repo.as_str()) {
        return Err(format!("Repository not allowlisted for remote import: {repo}"));
    }
    Ok(parsed)
}

pub async fn run_tool(state: &Shared, server: &str, args: &Value, workspace: &str, approved: bool) -> String {
    let root = workspace_root(state, workspace);
    if let Err(err) = std::fs::create_dir_all(&root) {
        return format!("workspace is not available: {err}");
    }
    match server {
        "filesystem" => run_files(args, &root),
        "bash" => {
            if !approved {
                return "Windows shell needs approval. Confirm the command, then run it again.".into();
            }
            run_cmd(args.get("command").and_then(|value| value.as_str()).unwrap_or(""), &root)
        }
        "git" => run_git(args.get("op").and_then(|value| value.as_str()).unwrap_or("status"), &root),
        "sqlite" => run_sqlite(args.get("sql").and_then(|value| value.as_str()).unwrap_or(""), &root),
        "memory" => {
            if let Some(note) = args.get("note").and_then(|value| value.as_str()) {
                remember(note);
            }
            run_memory(args.get("limit").and_then(|value| value.as_u64()).unwrap_or(12))
        }
        "web_search" => run_search(args.get("q").or_else(|| args.get("query")).and_then(|value| value.as_str()).unwrap_or("")).await,
        "fetch_url" => {
            if !approved {
                return "HTTP fetch needs approval.".into();
            }
            run_fetch(args.get("url").and_then(|value| value.as_str()).unwrap_or("")).await
        }
        "workspace_info" => run_workspace_info(&root),
        "mcp_bridge" => {
            if !approved {
                return "MCP bridge needs approval.".into();
            }
            let tool = args.get("tool").and_then(|value| value.as_str()).unwrap_or("");
            let bridge = args.get("bridge").and_then(|value| value.as_str()).unwrap_or("");
            run_bridge(tool, bridge, args).await
        }
        other => format!("Unknown tool {other}"),
    }
}

fn workspace_root(state: &Shared, workspace: &str) -> PathBuf {
    let chosen = if workspace.trim().is_empty() {
        read_lock(state).tool_root.clone()
    } else {
        workspace.to_string()
    };
    if chosen.trim().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(chosen)
    }
}

fn resolve_inside(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.contains('\0') {
        return Err("path contains a null.".into());
    }
    let root = std::fs::canonicalize(root).map_err(|err| format!("workspace is not available: {err}"))?;
    let joined = if Path::new(relative).is_absolute() {
        PathBuf::from(relative)
    } else {
        root.join(relative)
    };
    let check = if joined.exists() {
        std::fs::canonicalize(&joined).map_err(|err| err.to_string())?
    } else if let Some(parent) = joined.parent() {
        let parent = if parent.as_os_str().is_empty() { root.clone() } else { std::fs::canonicalize(parent).map_err(|err| err.to_string())? };
        parent.join(joined.file_name().unwrap_or_default())
    } else {
        return Err("path is not inside the workspace.".into());
    };
    if !check.starts_with(&root) {
        return Err("path escapes the workspace.".into());
    }
    Ok(check)
}

fn run_files(args: &Value, root: &Path) -> String {
    let op = args.get("op").and_then(|value| value.as_str()).unwrap_or("list");
    let relative = args.get("path").and_then(|value| value.as_str()).unwrap_or(".");
    let target = match resolve_inside(root, relative) {
        Ok(path) => path,
        Err(err) => return err,
    };
    match op {
        "list" => match std::fs::read_dir(&target) {
            Ok(entries) => {
                let mut lines = Vec::new();
                for entry in entries.flatten().take(200) {
                    let kind = if entry.path().is_dir() { "d" } else { "f" };
                    lines.push(format!("{kind} {}", entry.file_name().to_string_lossy()));
                }
                if lines.is_empty() { "(empty)".into() } else { lines.join("\n") }
            }
            Err(err) => format!("list failed: {err}"),
        },
        "read" => match std::fs::read(&target) {
            Ok(bytes) => {
                let bytes = if bytes.len() > 64_000 { &bytes[..64_000] } else { &bytes };
                String::from_utf8_lossy(bytes).to_string()
            }
            Err(err) => format!("read failed: {err}"),
        },
        "write" => {
            let content = args.get("content").and_then(|value| value.as_str()).unwrap_or("");
            if content.len() > 200_000 {
                return "Refusing a write larger than 200KB.".into();
            }
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&target, content) {
                Ok(()) => format!("Wrote {}", target.display()),
                Err(err) => format!("write failed: {err}"),
            }
        }
        other => format!("Unsupported filesystem op {other}"),
    }
}

fn run_cmd(command: &str, root: &Path) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return "Missing command.".into();
    }
    if trimmed.len() > 2000 || trimmed.contains('\0') {
        return "Command is too long or contains a null.".into();
    }
    let mut child = match discovery::command("cmd")
        .args(["/d", "/s", "/c", trimmed])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => return format!("Windows shell failed to start: {err}"),
    };
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let mut err = String::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = std::io::Read::take(pipe, 20_000).read_to_string(&mut out);
        }
        if let Some(pipe) = stderr.as_mut() {
            let _ = std::io::Read::take(pipe, 4_000).read_to_string(&mut err);
        }
        (out, err)
    });
    let started = std::time::Instant::now();
    let finished = loop {
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) if started.elapsed() > std::time::Duration::from_secs(20) => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(err) => return format!("Windows shell failed: {err}"),
        }
    };
    let (stdout, stderr) = reader.join().unwrap_or_default();
    if !finished {
        return format!("Windows shell timed out after 20 seconds.\n{}", clip(format!("{stdout}{stderr}").trim(), 2_000));
    }
    let text = format!("{stdout}{stderr}");
    let text = text.trim();
    if text.is_empty() { "(no output)".into() } else { clip(text, 20_000) }
}

fn run_git(op: &str, root: &Path) -> String {
    let args: &[&str] = match op {
        "log" => &["log", "-5", "--oneline"],
        "diff" => &["diff", "--stat"],
        _ => &["status", "--short"],
    };
    match discovery::command("git").args(args).current_dir(root).output() {
        Ok(output) => {
            let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            let text = text.trim();
            if text.is_empty() { "(clean)".into() } else { clip(text, 20_000) }
        }
        Err(err) => format!("git failed: {err}"),
    }
}

fn is_read_only_sql(sql: &str) -> bool {
    let body = sql.trim().trim_end_matches(';').trim();
    if body.is_empty() || body.contains(';') {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    if !(lower.starts_with("select") || lower.starts_with("with")) {
        return false;
    }
    for word in ["insert", "update", "delete", "drop", "alter", "attach", "pragma", "create", "replace", "reindex", "vacuum"] {
        if lower.split(|ch: char| !ch.is_ascii_alphanumeric()).any(|part| part == word) {
            return false;
        }
    }
    true
}

fn run_sqlite(sql: &str, root: &Path) -> String {
    if !is_read_only_sql(sql) {
        return "SQLite tool accepts one read-only SELECT.".into();
    }
    let db = match resolve_inside(root, "memory.sqlite") {
        Ok(path) => path,
        Err(err) => return err,
    };
    if !db.exists() {
        return "memory.sqlite is not in the workspace yet. Agent memory uses a separate notes file.".into();
    }
    let script = "import os,sqlite3,json\ncon=sqlite3.connect('file:'+os.environ['TIM_DB']+'?mode=ro', uri=True)\ncur=con.execute(os.environ['TIM_SQL'])\ncols=[d[0] for d in cur.description] if cur.description else []\nrows=[dict(zip(cols,row)) for row in cur.fetchmany(50)]\nprint(json.dumps(rows))";
    match discovery::command("python")
        .arg("-c")
        .arg(script)
        .env("TIM_DB", &db)
        .env("TIM_SQL", sql.trim())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) if output.status.success() => clip(&String::from_utf8_lossy(&output.stdout), 8000),
        Ok(output) => clip(&String::from_utf8_lossy(&output.stderr), 2000),
        Err(err) => format!("python is required for the SQLite tool: {err}"),
    }
}

fn memory_file() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").or_else(|_| std::env::var("HOME")).unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("Murmuration").join("tim-memory.json")
}

fn run_memory(limit: u64) -> String {
    let path = memory_file();
    let limit = limit.clamp(1, 50) as usize;
    let existing = std::fs::read_to_string(&path).unwrap_or_else(|_| "[]".into());
    let mut rows: Vec<Value> = serde_json::from_str(&existing).unwrap_or_default();
    if rows.len() > limit {
        rows = rows.split_off(rows.len() - limit);
    }
    clip(&serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into()), 8000)
}

pub fn remember(note: &str) {
    if note.trim().is_empty() {
        return;
    }
    let path = memory_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_else(|_| "[]".into());
    let mut rows: Vec<Value> = serde_json::from_str(&existing).unwrap_or_default();
    rows.push(json!({ "at": crate::state::now_iso(), "note": clip(note, 2000) }));
    if rows.len() > 200 {
        rows = rows.split_off(rows.len() - 200);
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into()));
}

fn run_workspace_info(root: &Path) -> String {
    let mut files = 0u64;
    let mut bytes = 0u64;
    fn walk(dir: &Path, depth: u8, files: &mut u64, bytes: &mut u64) {
        if depth > 4 || *files > 500 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, depth + 1, files, bytes);
            } else if path.is_file() {
                *files += 1;
                if let Ok(meta) = path.metadata() {
                    *bytes += meta.len();
                }
            }
        }
    }
    walk(root, 0, &mut files, &mut bytes);
    json!({ "workspace": root.display().to_string(), "files": files, "bytes": bytes }).to_string()
}

async fn run_fetch(url: &str) -> String {
    let Ok(parsed) = reqwest::Url::parse(url.trim()) else { return "Invalid url.".into() };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return "Only http and https URLs are allowed.".into();
    }
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(12)).build().map_err(|err| err.to_string());
    let client = match client {
        Ok(client) => client,
        Err(err) => return err,
    };
    match client.get(parsed).send().await {
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            format!("status {status}\n{}", clip(&text, 32_000))
        }
        Err(err) => format!("fetch failed: {err}"),
    }
}

async fn run_search(query: &str) -> String {
    let query = query.trim();
    if query.is_empty() {
        return "Missing query.".into();
    }
    let url = format!("https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1", urlencoding(query));
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(8)).build();
    let Ok(client) = client else { return "Search client failed.".into() };
    let Ok(response) = client.get(url).send().await else { return "Search failed.".into() };
    if !response.status().is_success() {
        return format!("Search failed ({})", response.status());
    }
    let Ok(body) = response.json::<Value>().await else { return "Search returned unreadable JSON.".into() };
    let mut lines = Vec::new();
    if let Some(text) = body.get("Heading").and_then(|value| value.as_str()) {
        if !text.is_empty() { lines.push(text.to_string()); }
    }
    if let Some(text) = body.get("AbstractText").and_then(|value| value.as_str()) {
        if !text.is_empty() { lines.push(text.to_string()); }
    }
    if let Some(topics) = body.get("RelatedTopics").and_then(|value| value.as_array()) {
        for topic in topics.iter().take(5) {
            if let Some(text) = topic.get("Text").and_then(|value| value.as_str()) {
                lines.push(text.to_string());
            }
        }
    }
    if lines.is_empty() { "No instant answer.".into() } else { lines.join("\n") }
}

async fn run_bridge(tool: &str, bridge: &str, args: &Value) -> String {
    if tool.trim().is_empty() {
        return "Missing tool name for MCP bridge.".into();
    }
    let Ok(endpoint) = reqwest::Url::parse(bridge.trim()) else { return "MCP bridge URL is invalid.".into() };
    if endpoint.scheme() != "http" && endpoint.scheme() != "https" {
        return "MCP bridge must be http or https.".into();
    }
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build();
    let Ok(client) = client else { return "MCP bridge client failed.".into() };
    let body = json!({ "tool": tool, "arguments": args });
    match client.post(endpoint).json(&body).send().await {
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            if !status.is_success() {
                format!("MCP bridge error ({status}): {}", clip(&text, 2000))
            } else {
                clip(&text, 8000)
            }
        }
        Err(err) => format!("MCP bridge failed: {err}"),
    }
}

fn urlencoding(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn clip(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

#[tauri::command]
pub async fn tim_list_catalog(platform: String) -> Result<Vec<CatalogHit>, String> {
    list_catalog(&platform).await
}

#[tauri::command]
pub async fn tim_fetch_workflow(raw_url: String) -> Result<String, String> {
    fetch_workflow(&raw_url).await
}

#[tauri::command]
pub async fn tim_run_tool(state: tauri::State<'_, Shared>, server: String, args: Value, workspace: String, approved: bool) -> Result<String, String> {
    Ok(run_tool(state.inner(), &server, &args, &workspace, approved).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_tool_rejects_writes() {
        assert!(is_read_only_sql("select * from memories"));
        assert!(!is_read_only_sql("select 1; drop table memories"));
        assert!(!is_read_only_sql("delete from memories"));
        assert!(!is_read_only_sql(""));
    }

    #[test]
    fn raw_github_allowlist_rejects_other_hosts() {
        assert!(assert_raw_github("https://example.com/langflow-ai/langflow/main/a.json").is_err());
        assert!(assert_raw_github("https://raw.githubusercontent.com/someone/else/main/a.json").is_err());
        assert!(assert_raw_github("https://raw.githubusercontent.com/langflow-ai/langflow/main/starter.json").is_ok());
    }
}
