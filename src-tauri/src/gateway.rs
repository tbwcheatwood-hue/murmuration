use crate::mcp;
use crate::orchestrator::{self, RouteDecision};
use crate::state::{new_traffic, node_vram, read_lock, write_lock, AppState, Shared};
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::pin::Pin;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tower_http::cors::CorsLayer;

#[derive(Clone)]
struct Ctx {
    shared: Shared,
    client: reqwest::Client,
}

struct InflightGuard(Shared);

impl InflightGuard {
    fn new(shared: Shared) -> Self {
        {
            let mut state = write_lock(&shared);
            state.inflight = state.inflight.saturating_add(1);
            state.sync_inference();
        }
        Self(shared)
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        let mut state = write_lock(&self.0);
        state.inflight = state.inflight.saturating_sub(1);
        state.sync_inference();
    }
}

pub fn spawn(handle: AppHandle, shared: Shared) {
    std::thread::Builder::new()
        .name("murmuration-gateway".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("gateway runtime");
            runtime.block_on(serve(handle, shared));
        })
        .ok();
}

async fn serve(handle: AppHandle, shared: Shared) {
    let client = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(600))
        .build()
        .expect("http client");
    let mut bound = None;
    for port in 11435..11445 {
        match tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await {
            Ok(listener) => {
                bound = Some((port, listener));
                break;
            }
            Err(_) => continue,
        }
    }
    let Some((port, listener)) = bound else {
        write_lock(&shared).log(
            "gateway",
            "gateway",
            "error",
            "Murmuration could not bind 127.0.0.1:11435-11444",
        );
        return;
    };
    {
        let mut state = write_lock(&shared);
        state.gateway_port = port;
        state.log(
            "gateway",
            "gateway",
            "info",
            format!("Murmuration OpenAI gateway listening on http://127.0.0.1:{port}/v1"),
        );
    }
    let _ = handle.emit("swarm-tick", read_lock(&shared).snapshot());
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/health", get(health))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat))
        .route("/v1/embeddings", post(embeddings))
        .route("/metrics", get(metrics))
        .route("/mcp", post(mcp_route))
        .fallback(not_found)
        .layer(CorsLayer::permissive())
        .with_state(Ctx { shared, client });
    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("murmuration gateway stopped: {err}");
    }
}

async fn health(State(ctx): State<Ctx>) -> Json<Value> {
    let state = read_lock(&ctx.shared);
    Json(json!({
        "ok": true,
        "name": "murmuration",
        "engine": "tensormesh",
        "transport": "swarmlink",
        "port": state.gateway_port,
        "mode": state.mode,
        "nodes": state.nodes.len(),
        "mesh": state.mesh_source
    }))
}

async fn models(State(ctx): State<Ctx>) -> Json<Value> {
    let state = read_lock(&ctx.shared);
    let mut data = vec![json!({
        "id": "murmuration",
        "object": "model",
        "owned_by": "swarm"
    })];
    for model in &state.models {
        data.push(json!({
            "id": model.name,
            "object": "model",
            "owned_by": model.host_node_id
        }));
    }
    Json(json!({ "object": "list", "data": data }))
}

async fn metrics(State(ctx): State<Ctx>) -> Response {
    let state = read_lock(&ctx.shared);
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        metrics_text(&state),
    )
        .into_response()
}

async fn not_found() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": {"message": "unknown Murmuration path", "type": "invalid_request_error"}})),
    )
}

async fn mcp_route(State(ctx): State<Ctx>, headers: axum::http::HeaderMap, Json(body): Json<Value>) -> Response {
    mcp::handle(ctx.shared, headers, body).await
}

async fn embeddings(State(ctx): State<Ctx>, Json(body): Json<Value>) -> Json<Value> {
    let input = body.get("input").map(|value| value.to_string()).unwrap_or_default();
    let vector = lexical_embed(&input);
    let decision = "Lexical fallback embedding. This is a hash projection, not a model embedding.";
    {
        let mut state = write_lock(&ctx.shared);
        state.push_traffic(new_traffic(
            "POST",
            "/v1/embeddings",
            200,
            1,
            "local",
            decision,
            &clip(&input, 240),
            "8-d lexical vector",
            "brief",
        ));
    }
    Json(json!({
        "object": "list",
        "model": "murmuration-lexical",
        "data": [{ "object": "embedding", "index": 0, "embedding": vector }]
    }))
}

async fn chat(State(ctx): State<Ctx>, Json(body): Json<Value>) -> Response {
    let requested = body.get("model").and_then(|value| value.as_str()).map(str::to_string);
    let stream = body.get("stream").and_then(|value| value.as_bool()).unwrap_or(false);
    let objective = user_text(&body);
    let preview = clip(&objective, 500);
    let decision = {
        let state = read_lock(&ctx.shared);
        orchestrator::route(&state, requested.as_deref())
    };
    let guard = InflightGuard::new(ctx.shared.clone());
    if let Some(url) = decision.proxy_url.clone() {
        match hop(&ctx, &body, &decision, &url, &preview, stream, guard).await {
            Hop::Served(response) => return response,
            Hop::Failed { guard, status, message, retryable } => {
                if retryable {
                    if let Some(alt) = decision.alt_proxy_url.clone() {
                        let alt_host = decision.alt_hostname.clone().unwrap_or_else(|| "backup".into());
                        write_lock(&ctx.shared).log(
                            "gateway",
                            "gateway",
                            "warn",
                            format!("{message} Retrying on {alt_host}."),
                        );
                        let mut next = decision.clone();
                        next.primary_hostname = alt_host;
                        next.proxy_url = Some(alt.clone());
                        next.reason = format!("Retried on {} after {} failed with {status}. {}", next.primary_hostname, decision.primary_hostname, decision.reason);
                        return match hop(&ctx, &body, &next, &alt, &preview, stream, guard).await {
                            Hop::Served(response) => response,
                            Hop::Failed { guard, status, message, .. } => upstream_failure(&ctx.shared, &next, &preview, status, &message, guard),
                        };
                    }
                }
                return upstream_failure(&ctx.shared, &decision, &preview, status, &message, guard);
            }
        }
    }
    brief(&ctx.shared, &objective, decision, preview, stream, guard)
}

enum Hop {
    Served(Response),
    Failed { guard: InflightGuard, status: u16, message: String, retryable: bool },
}

fn upstream_failure(shared: &Shared, decision: &RouteDecision, preview: &str, status: u16, message: &str, guard: InflightGuard) -> Response {
    let mut logged = decision.clone();
    logged.upstream = "error".into();
    record(shared, &logged, preview, message, 0, status);
    drop(guard);
    let body = json!({
        "error": {
            "message": message,
            "type": "upstream_error",
            "code": status
        },
        "murmuration": {
            "node": decision.primary_hostname,
            "upstream": "error",
            "reason": decision.reason
        }
    });
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-murmuration-upstream", "error")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn brief(
    shared: &Shared,
    objective: &str,
    decision: RouteDecision,
    preview: String,
    stream: bool,
    guard: InflightGuard,
) -> Response {
    let text = {
        let state = read_lock(shared);
        let plan = orchestrator::plan(
            &state,
            if objective.is_empty() { "Respond to the caller." } else { objective },
        );
        orchestrator::combined_reply(&plan, &decision)
    };
    if stream {
        brief_sse(shared.clone(), text, decision, preview, guard)
    } else {
        brief_json(shared, &text, &decision, &preview);
        drop(guard);
        json_completion(&text, &decision).into_response()
    }
}

fn brief_json(shared: &Shared, text: &str, decision: &RouteDecision, preview: &str) {
    let mut state = write_lock(shared);
    state.push_traffic(new_traffic(
        "POST",
        "/v1/chat/completions",
        200,
        1,
        &decision.primary_hostname,
        &decision.reason,
        preview,
        &clip(text, 400),
        &decision.upstream,
    ));
}

fn json_completion(text: &str, decision: &RouteDecision) -> Json<Value> {
    let prompt = 32;
    let completion = (text.len() / 4) as u32;
    Json(json!({
        "id": "chatcmpl-murmuration",
        "object": "chat.completion",
        "model": decision.primary_model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": text },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": prompt, "completion_tokens": completion, "total_tokens": prompt + completion },
        "murmuration": {
            "mode": decision.mode,
            "node": decision.primary_hostname,
            "upstream": decision.upstream,
            "reason": decision.reason
        }
    }))
}

fn brief_sse(shared: Shared, text: String, decision: RouteDecision, preview: String, guard: InflightGuard) -> Response {
    let words: Vec<String> = text.split_inclusive(' ').map(str::to_string).collect();
    struct Gate {
        words: Vec<String>,
        index: usize,
        phase: u8,
        recorded: bool,
        started: Instant,
        decision: RouteDecision,
        shared: Shared,
        preview: String,
        collected: String,
        _guard: InflightGuard,
    }
    let stream = futures_util::stream::unfold(
        Gate {
            words,
            index: 0,
            phase: 0,
            recorded: false,
            started: Instant::now(),
            decision,
            shared,
            preview,
            collected: String::new(),
            _guard: guard,
        },
        |mut gate| async move {
            if gate.phase >= 3 {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(14)).await;
            let data = if gate.phase == 0 {
                gate.phase = 1;
                chunk(&gate.decision, json!({"role": "assistant"}), None)
            } else if gate.phase == 1 && gate.index < gate.words.len() {
                let word = gate.words[gate.index].clone();
                gate.index += 1;
                gate.collected.push_str(&word);
                if !gate.recorded {
                    gate.recorded = true;
                    let ttft = gate.started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
                    record(&gate.shared, &gate.decision, &gate.preview, &gate.collected, ttft, 200);
                }
                chunk(&gate.decision, json!({"content": word}), None)
            } else if gate.phase == 1 {
                gate.phase = 2;
                chunk(&gate.decision, json!({}), Some("stop"))
            } else {
                gate.phase = 3;
                "[DONE]".to_string()
            };
            Some((Ok::<Event, Infallible>(Event::default().data(data)), gate))
        },
    );
    let mut response = Sse::new(stream).into_response();
    let _ = response.headers_mut().insert("x-murmuration-upstream", HeaderValue::from_static("brief"));
    response
}

async fn hop(
    ctx: &Ctx,
    body: &Value,
    decision: &RouteDecision,
    url: &str,
    preview: &str,
    stream: bool,
    guard: InflightGuard,
) -> Hop {
    let started = Instant::now();
    let sent = match ctx.client.post(url).json(body).send().await {
        Ok(response) => response,
        Err(err) => {
            return Hop::Failed {
                guard,
                status: 502,
                message: format!("Upstream {} did not answer. {err}", decision.primary_hostname),
                retryable: true,
            };
        }
    };
    let status = sent.status();
    if !status.is_success() {
        let text = sent.text().await.unwrap_or_default();
        let retryable = matches!(status.as_u16(), 502 | 503 | 504);
        return Hop::Failed {
            guard,
            status: status.as_u16(),
            message: format!("{} returned HTTP {status}. {}", decision.primary_hostname, clip(&text, 300)),
            retryable,
        };
    }
    if stream || sent.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or("").contains("text/event-stream") {
        return Hop::Served(stream_live(ctx, sent, decision, preview, started, guard));
    }
    let bytes = match sent.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            return Hop::Failed {
                guard,
                status: 502,
                message: format!("Upstream {} closed before the reply finished. {err}", decision.primary_hostname),
                retryable: true,
            };
        }
    };
    let ttft = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    let lifted = lift_json_bytes(&bytes);
    record(&ctx.shared, decision, preview, &clip(&String::from_utf8_lossy(&lifted), 400), ttft, 200);
    drop(guard);
    Hop::Served(
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-murmuration-upstream", "live")
            .body(Body::from(lifted))
            .unwrap_or_else(|_| Response::new(Body::empty())),
    )
}

fn stream_live(ctx: &Ctx, sent: reqwest::Response, decision: &RouteDecision, preview: &str, started: Instant, guard: InflightGuard) -> Response {
    let status = sent.status();
    let upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>> = Box::pin(sent.bytes_stream());
    struct Proxy {
        upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
        pending: Vec<u8>,
        recorded: bool,
        started: Instant,
        shared: Shared,
        decision: RouteDecision,
        preview: String,
        _guard: InflightGuard,
    }
    let mapped = futures_util::stream::unfold(
        Proxy {
            upstream,
            pending: Vec::new(),
            recorded: false,
            started,
            shared: ctx.shared.clone(),
            decision: decision.clone(),
            preview: preview.to_string(),
            _guard: guard,
        },
        |mut proxy| async move {
            loop {
                match proxy.upstream.next().await {
                    Some(Ok(chunk)) => {
                        proxy.pending.extend_from_slice(&chunk);
                        let Some(split) = proxy.pending.iter().rposition(|byte| *byte == b'\n') else {
                            continue;
                        };
                        let ready: Vec<u8> = proxy.pending.drain(..=split).collect();
                        let text = String::from_utf8_lossy(&ready);
                        let rewritten = rewrite_sse_block(&text);
                        if rewritten.is_empty() {
                            continue;
                        }
                        if !proxy.recorded {
                            proxy.recorded = true;
                            let ttft = proxy.started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
                            record(&proxy.shared, &proxy.decision, &proxy.preview, &clip(&rewritten, 400), ttft, 200);
                        }
                        return Some((Ok::<Bytes, std::io::Error>(Bytes::from(rewritten)), proxy));
                    }
                    Some(Err(err)) => {
                        record(
                            &proxy.shared,
                            &proxy.decision,
                            &proxy.preview,
                            &err.to_string(),
                            proxy.started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
                            502,
                        );
                        return Some((Err(std::io::Error::other(err.to_string())), proxy));
                    }
                    None => {
                        if proxy.pending.is_empty() {
                            return None;
                        }
                        let rest = String::from_utf8_lossy(&std::mem::take(&mut proxy.pending)).into_owned();
                        let rewritten = rewrite_sse_block(&rest);
                        if rewritten.is_empty() {
                            return None;
                        }
                        return Some((Ok(Bytes::from(rewritten)), proxy));
                    }
                }
            }
        },
    );
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header("x-murmuration-upstream", "live")
        .body(Body::from_stream(mapped))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn lift_json_bytes(bytes: &[u8]) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(bytes) else {
        return bytes.to_vec();
    };
    lift_reasoning(&mut value);
    serde_json::to_vec(&value).unwrap_or_else(|_| bytes.to_vec())
}

fn lift_reasoning(value: &mut Value) {
    let Some(choices) = value.get_mut("choices").and_then(|item| item.as_array_mut()) else {
        return;
    };
    for choice in choices {
        if let Some(message) = choice.get_mut("message") {
            lift_text(message);
        }
        if let Some(delta) = choice.get_mut("delta") {
            lift_text(delta);
        }
    }
}

fn lift_text(message: &mut Value) {
    let content = message.get("content").and_then(|item| item.as_str()).unwrap_or("").trim();
    if !content.is_empty() {
        return;
    }
    let reasoning = message
        .get("reasoning_content")
        .and_then(|item| item.as_str())
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .or_else(|| message.get("reasoning").and_then(|item| item.as_str()).filter(|text| !text.is_empty()).map(str::to_string));
    let Some(reasoning) = reasoning else {
        return;
    };
    if let Some(object) = message.as_object_mut() {
        object.insert("content".into(), Value::String(reasoning));
    }
}

fn rewrite_sse_block(block: &str) -> String {
    let mut out = String::new();
    for line in block.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let Some(rest) = trimmed.strip_prefix("data:") else {
            out.push_str(line);
            continue;
        };
        let payload = rest.trim_start();
        if payload.is_empty() || payload == "[DONE]" {
            out.push_str(line);
            continue;
        }
        let Ok(mut value) = serde_json::from_str::<Value>(payload) else {
            out.push_str(line);
            continue;
        };
        lift_reasoning(&mut value);
        let ending = &line[trimmed.len()..];
        out.push_str("data: ");
        out.push_str(&value.to_string());
        out.push_str(ending);
    }
    out
}

fn record(shared: &Shared, decision: &RouteDecision, preview: &str, response: &str, ttft: u32, status: u16) {
    let mut state = write_lock(shared);
    state.push_traffic(new_traffic(
        "POST",
        "/v1/chat/completions",
        status,
        ttft,
        &decision.primary_hostname,
        &decision.reason,
        preview,
        response,
        &decision.upstream,
    ));
}

fn chunk(decision: &RouteDecision, delta: Value, finish: Option<&str>) -> String {
    json!({
        "id": "chatcmpl-murmuration",
        "object": "chat.completion.chunk",
        "model": decision.primary_model,
        "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }]
    })
    .to_string()
}

fn user_text(body: &Value) -> String {
    let Some(messages) = body.get("messages").and_then(|value| value.as_array()) else {
        return String::new();
    };
    for message in messages.iter().rev() {
        if message.get("role").and_then(|value| value.as_str()) == Some("user") {
            return content_text(message.get("content").unwrap_or(&Value::Null));
        }
    }
    String::new()
}

fn content_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

fn lexical_embed(text: &str) -> Vec<f64> {
    let mut acc = [0u64; 8];
    for (index, byte) in text.bytes().enumerate() {
        acc[index % 8] = acc[index % 8].wrapping_add((byte as u64).wrapping_mul(index as u64 + 17));
    }
    acc.into_iter().map(|value| (value % 1000) as f64 / 1000.0).collect()
}

pub fn metrics_text(state: &AppState) -> String {
    let mut out = String::from(
        "# HELP murmuration_node_up Node can receive work\n# TYPE murmuration_node_up gauge\n# HELP murmuration_vram_allocated_bytes Allocated VRAM\n# TYPE murmuration_vram_allocated_bytes gauge\n# HELP murmuration_vram_total_bytes Total VRAM\n# TYPE murmuration_vram_total_bytes gauge\n# HELP murmuration_gpu_utilization_ratio GPU utilization\n# TYPE murmuration_gpu_utilization_ratio gauge\n# HELP murmuration_tokens_per_second Decode throughput\n# TYPE murmuration_tokens_per_second gauge\n# HELP murmuration_ping_milliseconds Tailscale ping\n# TYPE murmuration_ping_milliseconds gauge\n",
    );
    for node in &state.nodes {
        let up = if matches!(node.status.as_str(), "online" | "degraded") { 1 } else { 0 };
        let (total, allocated) = node_vram(node);
        let name = esc(&node.hostname);
        let util = node.gpus.first().map(|gpu| gpu.utilization / 100.0).unwrap_or(0.0);
        out.push_str(&format!(
            "murmuration_node_up{{node=\"{name}\",relay=\"{}\"}} {up}\n",
            esc(&node.relay)
        ));
        out.push_str(&format!(
            "murmuration_vram_total_bytes{{node=\"{name}\"}} {}\n",
            (total * 1e9) as u64
        ));
        out.push_str(&format!(
            "murmuration_vram_allocated_bytes{{node=\"{name}\"}} {}\n",
            (allocated * 1e9) as u64
        ));
        out.push_str(&format!("murmuration_gpu_utilization_ratio{{node=\"{name}\"}} {util:.4}\n"));
        out.push_str(&format!(
            "murmuration_tokens_per_second{{node=\"{name}\"}} {:.3}\n",
            node.tokens_per_sec
        ));
        out.push_str(&format!("murmuration_ping_milliseconds{{node=\"{name}\"}} {}\n", node.ping_ms));
    }
    out
}

fn esc(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn clip(value: &str, max: usize) -> String {
    let mut out: String = value.chars().take(max).collect();
    if value.chars().count() > max {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_content_is_copied_into_empty_content() {
        let mut value = json!({"choices":[{"message":{"role":"assistant","content":"","reasoning_content":"ready"}}]});
        lift_reasoning(&mut value);
        assert_eq!(value.pointer("/choices/0/message/content").and_then(|item| item.as_str()), Some("ready"));
        assert_eq!(value.pointer("/choices/0/message/reasoning_content").and_then(|item| item.as_str()), Some("ready"));
    }

    #[test]
    fn existing_content_is_kept() {
        let mut value = json!({"choices":[{"delta":{"content":"answer","reasoning_content":"think"}}]});
        lift_reasoning(&mut value);
        assert_eq!(value.pointer("/choices/0/delta/content").and_then(|item| item.as_str()), Some("answer"));
    }

    #[test]
    fn sse_reasoning_delta_gains_content() {
        let raw = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"ready\"}}]}\n\n";
        let out = rewrite_sse_block(raw);
        assert!(out.contains("\"content\":\"ready\""));
        assert!(out.contains("reasoning_content"));
        assert!(!out.contains("[DONE]"));
        let done = rewrite_sse_block("data: [DONE]\n");
        assert!(done.contains("[DONE]"));
    }
}
