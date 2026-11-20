use chrono::{Local, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub const MAX_LOGS: usize = 400;
pub const MAX_TRAFFIC: usize = 160;

pub type Shared = Arc<RwLock<AppState>>;

pub fn initial() -> Shared {
    Arc::new(RwLock::new(AppState::boot()))
}

pub fn read_lock(shared: &Shared) -> RwLockReadGuard<'_, AppState> {
    shared.read().unwrap_or_else(|err| err.into_inner())
}

pub fn write_lock(shared: &Shared) -> RwLockWriteGuard<'_, AppState> {
    shared.write().unwrap_or_else(|err| err.into_inner())
}

fn uid(prefix: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("{prefix}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

pub fn now_hms() -> String {
    Local::now().format("%H:%M:%S%.3f").to_string()
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMetric {
    pub name: String,
    pub vram_total_gb: f64,
    pub vram_allocated_gb: f64,
    pub utilization: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwarmNode {
    pub id: String,
    pub hostname: String,
    pub tailscale_ip: String,
    pub os: String,
    pub ping_ms: u32,
    pub relay: String,
    pub derp_region: Option<String>,
    pub lm_link_bound: bool,
    pub lm_link_port: Option<u16>,
    pub upstream_alive: bool,
    pub gpus: Vec<GpuMetric>,
    pub unified_memory_gb: Option<f64>,
    pub configured_vram_gb: f64,
    pub inferred_vram_gb: f64,
    pub effective_vram_gb: f64,
    pub vram_source: String,
    pub ram_gb: f64,
    pub cpu_name: String,
    pub gpu_name: String,
    pub tokens_per_sec: f64,
    pub thermal: String,
    pub role: Option<String>,
    pub simulated: bool,
    pub status: String,
    pub throughput_mbps: f64,
    pub color: String,
    pub last_seen: String,
    #[serde(skip)]
    pub ping_base: u32,
    #[serde(skip)]
    pub util_base: f64,
    #[serde(skip)]
    pub token_base: f64,
    #[serde(skip)]
    pub announced_down: bool,
    #[serde(skip)]
    pub ping_misses: u32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelArtifact {
    pub id: String,
    pub name: String,
    pub format: String,
    pub quant: String,
    pub size_gb: f64,
    pub params_b: f64,
    pub host_node_id: String,
    pub architecture: String,
    pub context_max: u32,
    pub layers: u32,
    pub kv_heads: u32,
    pub head_dim: u32,
    pub loaded: bool,
    pub simulated: bool,
    pub sharded: bool,
    pub shard_nodes: Vec<String>,
    pub path: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub id: String,
    pub ts: String,
    pub node_id: String,
    pub source: String,
    pub level: String,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficEntry {
    pub id: String,
    pub ts: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub ttft_ms: u32,
    pub routed_to: String,
    pub decision: String,
    pub request_preview: String,
    pub response_preview: String,
    pub upstream: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintRole {
    pub role: String,
    pub model_hint: String,
    pub min_vram_gb: f64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blueprint {
    pub id: String,
    pub name: String,
    pub description: String,
    pub mode: String,
    pub roles: Vec<BlueprintRole>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceParams {
    pub context_length: u32,
    pub kv_quant: String,
    pub flash_attention: bool,
    pub gpu_offload_layers: u32,
    pub speculative: bool,
    pub draft_node_id: Option<String>,
    pub evict_for_fit: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    pub id: String,
    pub model_id: String,
    pub model_name: String,
    pub from_node: String,
    pub to_node: String,
    pub progress: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub nodes: Vec<SwarmNode>,
    pub models: Vec<ModelArtifact>,
    pub logs: Vec<LogLine>,
    pub traffic: Vec<TrafficEntry>,
    pub blueprints: Vec<Blueprint>,
    pub active_blueprint_id: String,
    pub mode: String,
    pub gateway_port: u16,
    pub gateway_token: String,
    pub demo_mesh: bool,
    pub live_peer_count: usize,
    pub params: InferenceParams,
    pub transfers: Vec<Transfer>,
    pub inference_active: bool,
    pub failover_note: Option<String>,
    pub tool_root: String,
    pub shell_armed: bool,
    pub shard_backend: String,
    pub mesh_source: String,
    pub link_status: String,
    pub link_message: String,
}

pub struct AppState {
    pub nodes: Vec<SwarmNode>,
    pub models: Vec<ModelArtifact>,
    pub logs: VecDeque<LogLine>,
    pub traffic: VecDeque<TrafficEntry>,
    pub blueprints: Vec<Blueprint>,
    pub active_blueprint_id: String,
    pub mode: String,
    pub gateway_port: u16,
    pub gateway_token: String,
    pub demo_mesh: bool,
    pub live_peer_count: usize,
    pub params: InferenceParams,
    pub transfers: Vec<Transfer>,
    pub inference_active: bool,
    pub failover_note: Option<String>,
    pub tool_root: String,
    pub shell_armed: bool,
    pub shard_backend: String,
    pub mesh_source: String,
    pub link_status: String,
    pub link_message: String,
    pub tick: u64,
    pub stress_ticks: u32,
    pub inflight: u32,
    pub ui_inference: bool,
}

impl AppState {
    pub fn boot() -> Self {
        let mut state = Self {
            nodes: Vec::new(),
            models: Vec::new(),
            logs: VecDeque::new(),
            traffic: VecDeque::new(),
            blueprints: demo_blueprints(),
            active_blueprint_id: "refactor".into(),
            mode: "pipeline".into(),
            gateway_port: 11435,
            gateway_token: format!(
                "tm_{:x}{:x}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|dur| dur.subsec_nanos())
                    .unwrap_or(0)
            ),
            demo_mesh: false,
            live_peer_count: 0,
            params: InferenceParams {
                context_length: 8192,
                kv_quant: "q8".into(),
                flash_attention: true,
                gpu_offload_layers: 256,
                speculative: false,
                draft_node_id: None,
                evict_for_fit: true,
            },
            transfers: Vec::new(),
            inference_active: false,
            failover_note: None,
            tool_root: String::new(),
            shell_armed: false,
            shard_backend: String::new(),
            mesh_source: "pending".into(),
            link_status: "pending".into(),
            link_message: "Checking LM Link…".into(),
            tick: 0,
            stress_ticks: 0,
            inflight: 0,
            ui_inference: false,
        };
        state.log("gateway", "lms", "info", "Checking LM Link for the devices on this account.");
        state
    }

    #[cfg(test)]
    pub fn demo() -> Self {
        let mut state = Self {
            nodes: demo_nodes(),
            models: demo_models(),
            logs: VecDeque::new(),
            traffic: VecDeque::new(),
            blueprints: demo_blueprints(),
            active_blueprint_id: "refactor".into(),
            mode: "pipeline".into(),
            gateway_port: 11435,
            gateway_token: format!(
                "tm_{:x}{:x}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|dur| dur.subsec_nanos())
                    .unwrap_or(0)
            ),
            demo_mesh: true,
            live_peer_count: 0,
            params: InferenceParams {
                context_length: 8192,
                kv_quant: "q8".into(),
                flash_attention: true,
                gpu_offload_layers: 256,
                speculative: false,
                draft_node_id: Some("kiln".into()),
                evict_for_fit: true,
            },
            transfers: Vec::new(),
            inference_active: false,
            failover_note: None,
            tool_root: String::new(),
            shell_armed: false,
            shard_backend: String::new(),
            mesh_source: "demo".into(),
            link_status: "demo".into(),
            link_message: "Fit tests use an in-memory pool. It is not an LM Link.".into(),
            tick: 0,
            stress_ticks: 0,
            inflight: 0,
            ui_inference: false,
        };
        state.log("gateway", "gateway", "info", "Murmuration gateway bound for 127.0.0.1:11435/v1");
        state.log("gateway", "orchestrator", "info", "Blueprint Full-Stack Repo Refactor is active. Roles follow VRAM rank.");
        state.log("atlas", "lms", "info", "qwen2.5-32b-instruct resident. KV budget included in the 20.4 GB allocation.");
        state.log("loom", "lms", "info", "qwen2.5-coder-14b resident on the coder node.");
        state.log("harbor", "llmster", "info", "Critic and embedding models are warm on unified memory.");
        state.log("kiln", "tailscale", "warn", "kiln is on a DERP relay (iad). Draft tokens will pay that hop.");
        state
    }

    pub fn log(&mut self, node_id: &str, source: &str, level: &str, message: impl Into<String>) {
        self.logs.push_back(LogLine {
            id: uid("log"),
            ts: now_hms(),
            node_id: node_id.into(),
            source: source.into(),
            level: level.into(),
            message: message.into(),
        });
        while self.logs.len() > MAX_LOGS {
            self.logs.pop_front();
        }
    }

    pub fn push_traffic(&mut self, entry: TrafficEntry) {
        self.traffic.push_back(entry);
        while self.traffic.len() > MAX_TRAFFIC {
            self.traffic.pop_front();
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            nodes: self.nodes.clone(),
            models: self.models.clone(),
            logs: self.logs.iter().cloned().collect(),
            traffic: self.traffic.iter().cloned().collect(),
            blueprints: self.blueprints.clone(),
            active_blueprint_id: self.active_blueprint_id.clone(),
            mode: self.mode.clone(),
            gateway_port: self.gateway_port,
            gateway_token: self.gateway_token.clone(),
            demo_mesh: self.demo_mesh,
            live_peer_count: self.live_peer_count,
            params: self.params.clone(),
            transfers: self.transfers.clone(),
            inference_active: self.inference_active,
            failover_note: self.failover_note.clone(),
            tool_root: self.tool_root.clone(),
            shell_armed: self.shell_armed,
            shard_backend: self.shard_backend.clone(),
            mesh_source: self.mesh_source.clone(),
            link_status: self.link_status.clone(),
            link_message: self.link_message.clone(),
        }
    }

    pub fn sync_inference(&mut self) {
        self.inference_active = self.inflight > 0 || self.ui_inference;
    }

    pub fn node_mut(&mut self, id: &str) -> Option<&mut SwarmNode> {
        self.nodes.iter_mut().find(|node| node.id == id)
    }

    pub fn model_mut(&mut self, id: &str) -> Option<&mut ModelArtifact> {
        self.models.iter_mut().find(|model| model.id == id)
    }
}

pub fn new_traffic(
    method: &str,
    path: &str,
    status: u16,
    ttft_ms: u32,
    routed_to: &str,
    decision: &str,
    request_preview: &str,
    response_preview: &str,
    upstream: &str,
) -> TrafficEntry {
    TrafficEntry {
        id: uid("req"),
        ts: now_iso(),
        method: method.into(),
        path: path.into(),
        status,
        ttft_ms,
        routed_to: routed_to.into(),
        decision: decision.into(),
        request_preview: clip(request_preview, 700),
        response_preview: clip(response_preview, 700),
        upstream: upstream.into(),
    }
}

fn clip(value: &str, max: usize) -> String {
    let mut out: String = value.chars().take(max).collect();
    if value.chars().count() > max {
        out.push('…');
    }
    out
}

#[cfg(test)]
fn gpu(name: &str, total: f64, allocated: f64, util: f64) -> GpuMetric {
    GpuMetric {
        name: name.into(),
        vram_total_gb: total,
        vram_allocated_gb: allocated,
        utilization: util,
    }
}

#[cfg(test)]
fn node(
    id: &str,
    hostname: &str,
    ip: &str,
    os: &str,
    ping: u32,
    relay: &str,
    derp: Option<&str>,
    gpus: Vec<GpuMetric>,
    unified: Option<f64>,
    tokens: f64,
    role: &str,
    color: &str,
) -> SwarmNode {
    let util_base = gpus.first().map(|item| item.utilization).unwrap_or(8.0);
    SwarmNode {
        id: id.into(),
        hostname: hostname.into(),
        tailscale_ip: ip.into(),
        os: os.into(),
        ping_ms: ping,
        relay: relay.into(),
        derp_region: derp.map(str::to_string),
        lm_link_bound: true,
        lm_link_port: Some(1234),
        upstream_alive: false,
        gpus,
        unified_memory_gb: unified,
        configured_vram_gb: 0.0,
        inferred_vram_gb: 0.0,
        effective_vram_gb: 0.0,
        vram_source: "none".into(),
        ram_gb: 0.0,
        cpu_name: String::new(),
        gpu_name: String::new(),
        tokens_per_sec: tokens,
        thermal: "warm".into(),
        role: Some(role.into()),
        simulated: true,
        status: "online".into(),
        throughput_mbps: if relay == "derp" { 12.0 } else { 4.0 },
        color: color.into(),
        last_seen: now_iso(),
        ping_base: ping,
        util_base,
        token_base: tokens,
        announced_down: false,
        ping_misses: 0,
    }
}

#[cfg(test)]
fn demo_nodes() -> Vec<SwarmNode> {
    vec![
        SwarmNode {
            id: "local".into(),
            hostname: hostname(),
            tailscale_ip: "127.0.0.1".into(),
            os: std::env::consts::OS.into(),
            ping_ms: 0,
            relay: "local".into(),
            derp_region: None,
            lm_link_bound: false,
            lm_link_port: Some(1234),
            upstream_alive: false,
            gpus: Vec::new(),
            unified_memory_gb: None,
            configured_vram_gb: 0.0,
            inferred_vram_gb: 0.0,
            effective_vram_gb: 0.0,
            vram_source: "none".into(),
            ram_gb: 0.0,
            cpu_name: String::new(),
            gpu_name: String::new(),
            tokens_per_sec: 0.0,
            thermal: "cool".into(),
            role: Some("standby".into()),
            simulated: false,
            status: "online".into(),
            throughput_mbps: 0.0,
            color: "#8AB4FF".into(),
            last_seen: now_iso(),
            ping_base: 0,
            util_base: 6.0,
            token_base: 0.0,
            announced_down: false,
            ping_misses: 0,
        },
        node(
            "atlas",
            "atlas-4090",
            "100.94.12.4",
            "windows",
            4,
            "direct",
            None,
            vec![gpu("NVIDIA GeForce RTX 4090", 24.0, 20.4, 46.0)],
            None,
            36.0,
            "architect",
            "#7EE0C6",
        ),
        node(
            "loom",
            "loom-4080",
            "100.94.12.11",
            "linux",
            6,
            "direct",
            None,
            vec![gpu("NVIDIA GeForce RTX 4080", 16.0, 12.6, 38.0)],
            None,
            52.0,
            "coder",
            "#D2A8FF",
        ),
        node(
            "harbor",
            "harbor-mbp",
            "100.94.12.18",
            "macos",
            11,
            "direct",
            None,
            vec![gpu("Apple M3 Max", 16.0, 11.4, 27.0)],
            Some(36.0),
            24.0,
            "critic",
            "#E6B35A",
        ),
        node(
            "kiln",
            "kiln-laptop",
            "100.94.12.27",
            "windows",
            48,
            "derp",
            Some("iad"),
            vec![gpu("NVIDIA GeForce RTX 4060 Laptop", 8.0, 3.6, 18.0)],
            None,
            91.0,
            "draft",
            "#F07178",
        ),
    ]
}

#[cfg(test)]
fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "this-machine".into())
}

#[cfg(test)]
fn model(
    id: &str,
    name: &str,
    format: &str,
    quant: &str,
    size: f64,
    params: f64,
    host: &str,
    arch: &str,
    ctx: u32,
    layers: u32,
    kv_heads: u32,
    head_dim: u32,
    loaded: bool,
) -> ModelArtifact {
    ModelArtifact {
        id: id.into(),
        name: name.into(),
        format: format.into(),
        quant: quant.into(),
        size_gb: size,
        params_b: params,
        host_node_id: host.into(),
        architecture: arch.into(),
        context_max: ctx,
        layers,
        kv_heads,
        head_dim,
        loaded,
        simulated: true,
        sharded: false,
        shard_nodes: Vec::new(),
        path: None,
    }
}

#[cfg(test)]
fn demo_models() -> Vec<ModelArtifact> {
    vec![
        model("qwen-32b", "qwen2.5-32b-instruct", "gguf", "Q4_K_M", 18.8, 32.0, "atlas", "qwen2", 32768, 64, 8, 128, true),
        model("qwen-coder-14b", "qwen2.5-coder-14b", "gguf", "Q5_K_M", 10.2, 14.0, "loom", "qwen2", 32768, 48, 8, 128, true),
        model("llama-3b", "llama-3.2-3b-instruct", "mlx", "Q8", 3.4, 3.2, "harbor", "llama", 131072, 28, 8, 128, true),
        model("nomic-embed", "nomic-embed-text", "gguf", "Q8_0", 0.6, 0.1, "harbor", "nomic", 8192, 12, 8, 128, true),
        model("qwen-1.5b", "qwen2.5-coder-1.5b", "gguf", "Q8_0", 1.8, 1.5, "kiln", "qwen2", 32768, 28, 2, 128, true),
        model("llama-70b", "llama-3.1-70b-instruct", "gguf", "Q4_K_M", 39.5, 70.0, "atlas", "llama", 131072, 80, 8, 128, false),
        model("llama-405b", "llama-3.1-405b-instruct", "gguf", "Q3_K_M", 190.0, 405.0, "unassigned", "llama", 131072, 126, 8, 128, false),
    ]
}

fn role(role: &str, hint: &str, min_vram: f64) -> BlueprintRole {
    BlueprintRole {
        role: role.into(),
        model_hint: hint.into(),
        min_vram_gb: min_vram,
    }
}

fn demo_blueprints() -> Vec<Blueprint> {
    vec![
        Blueprint {
            id: "refactor".into(),
            name: "Full-Stack Repo Refactor".into(),
            description: "Architect plans the change, the coder writes the patch, and the critic checks the diff before it lands.".into(),
            mode: "pipeline".into(),
            roles: vec![
                role("architect", "qwen2.5-32b-instruct", 20.0),
                role("coder", "qwen2.5-coder-14b", 12.0),
                role("critic", "llama-3.2-3b-instruct", 8.0),
            ],
        },
        Blueprint {
            id: "research".into(),
            name: "Deep Research & Synthesis".into(),
            description: "A large node gathers the argument, the critic compresses it, and the embedding model keeps the notes searchable.".into(),
            mode: "pipeline".into(),
            roles: vec![
                role("architect", "qwen2.5-32b-instruct", 20.0),
                role("critic", "llama-3.2-3b-instruct", 8.0),
                role("embedder", "nomic-embed-text", 4.0),
            ],
        },
        Blueprint {
            id: "rag".into(),
            name: "Local RAG Pipeline".into(),
            description: "Embeddings stay on the unified-memory node. The architect answers only after the retrieved notes are attached.".into(),
            mode: "pipeline".into(),
            roles: vec![
                role("embedder", "nomic-embed-text", 4.0),
                role("architect", "qwen2.5-32b-instruct", 20.0),
                role("critic", "llama-3.2-3b-instruct", 8.0),
            ],
        },
        Blueprint {
            id: "shard-70b".into(),
            name: "70B Layer Shard".into(),
            description: "Split a 70B-class model across the pooled cards when no single GPU can hold the weights and KV cache.".into(),
            mode: "shard".into(),
            roles: vec![role("shard", "llama-3.1-70b-instruct", 8.0)],
        },
        Blueprint {
            id: "speculative".into(),
            name: "Speculative Mesh Decode".into(),
            description: "The laptop drafts tokens. The 4090 accepts or rejects them over the LM Link hop.".into(),
            mode: "speculative".into(),
            roles: vec![
                role("draft", "qwen2.5-coder-1.5b", 4.0),
                role("coder", "qwen2.5-coder-14b", 12.0),
            ],
        },
    ]
}

#[cfg(test)]
pub fn bare_node(id: &str, hostname: &str) -> SwarmNode {
    SwarmNode {
        id: id.into(),
        hostname: hostname.into(),
        tailscale_ip: String::new(),
        os: "LM Link".into(),
        ping_ms: 0,
        relay: "lm-link".into(),
        derp_region: None,
        lm_link_bound: true,
        lm_link_port: Some(1234),
        upstream_alive: false,
        gpus: Vec::new(),
        unified_memory_gb: None,
        configured_vram_gb: 0.0,
        inferred_vram_gb: 0.0,
        effective_vram_gb: 0.0,
        vram_source: "none".into(),
        ram_gb: 0.0,
        cpu_name: String::new(),
        gpu_name: String::new(),
        tokens_per_sec: 0.0,
        thermal: "cool".into(),
        role: Some("standby".into()),
        simulated: false,
        status: "online".into(),
        throughput_mbps: 0.0,
        color: "#8AB4FF".into(),
        last_seen: now_iso(),
        ping_base: 0,
        util_base: 0.0,
        token_base: 0.0,
        announced_down: false,
        ping_misses: 0,
    }
}

pub fn node_vram(node: &SwarmNode) -> (f64, f64) {
    let total = node.gpus.iter().map(|gpu| gpu.vram_total_gb).sum();
    let allocated = node.gpus.iter().map(|gpu| gpu.vram_allocated_gb).sum();
    (total, allocated)
}

/// Measured hardware wins. A manifest override is next. A model footprint is the floor.
/// The second value is the tier (3 measured, 2 configured, 1 inferred) so equal sizes keep the measured node ahead.
pub fn vram_rank(node: &SwarmNode) -> (f64, u8) {
    let measured = node_vram(node).0;
    if measured > 0.0 {
        return (measured, 3);
    }
    if node.configured_vram_gb > 0.0 {
        return (node.configured_vram_gb, 2);
    }
    if node.inferred_vram_gb > 0.0 {
        return (node.inferred_vram_gb, 1);
    }
    (0.0, 0)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareEntry {
    pub hostname: String,
    pub vram_gb: f64,
    pub ram_gb: f64,
    pub cpu: String,
    pub gpu: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct HardwareFile {
    nodes: Vec<HardwareEntry>,
}

#[allow(dead_code)]
pub fn hardware_matches(node: &SwarmNode, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    let (measured, _) = node_vram(node);
    let blob = format!(
        "{} {} {} {} {} {} {} {} {} {} {}",
        node.hostname,
        node.status,
        node.role.as_deref().unwrap_or(""),
        node.cpu_name,
        node.gpu_name,
        node.gpus.iter().map(|gpu| gpu.name.as_str()).collect::<Vec<_>>().join(" "),
        node.vram_source,
        trim_num(node.ram_gb),
        trim_num(node.configured_vram_gb),
        trim_num(node.effective_vram_gb),
        trim_num(measured),
    )
    .to_ascii_lowercase();
    query.split_whitespace().all(|part| blob.contains(part))
}

#[allow(dead_code)]
fn trim_num(value: f64) -> String {
    if value <= 0.0 {
        return String::new();
    }
    if (value - value.round()).abs() < 0.05 {
        format!("{:.0}", value.round())
    } else {
        format!("{value:.1}")
    }
}

pub fn assign_hardware(node: &mut SwarmNode, entry: &HardwareEntry) {
    node.configured_vram_gb = entry.vram_gb.max(0.0);
    node.ram_gb = entry.ram_gb.max(0.0);
    node.cpu_name = entry.cpu.trim().to_string();
    node.gpu_name = entry.gpu.trim().to_string();
    refresh_vram_fields(node);
}

pub fn hardware_file_path() -> std::path::PathBuf {
    if let Ok(custom) = std::env::var("MURMURATION_HARDWARE_FILE") {
        return std::path::PathBuf::from(custom);
    }
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("Murmuration").join("hardware.json")
}

pub fn load_hardware_entries(path: &std::path::Path) -> Vec<HardwareEntry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<HardwareFile>(&text).map(|file| file.nodes).unwrap_or_default()
}

pub fn save_hardware_entries(path: &std::path::Path, entries: &[HardwareEntry]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let body = serde_json::to_string_pretty(&HardwareFile { nodes: entries.to_vec() }).map_err(|err| err.to_string())?;
    std::fs::write(path, body).map_err(|err| err.to_string())
}

pub fn upsert_hardware(path: &std::path::Path, entry: HardwareEntry) -> Result<(), String> {
    let mut entries = load_hardware_entries(path);
    if let Some(existing) = entries.iter_mut().find(|item| item.hostname.eq_ignore_ascii_case(&entry.hostname)) {
        *existing = entry;
    } else {
        entries.push(entry);
    }
    save_hardware_entries(path, &entries)
}

pub fn apply_saved_hardware(state: &mut AppState) {
    let entries = load_hardware_entries(&hardware_file_path());
    for node in &mut state.nodes {
        if let Some(entry) = entries.iter().find(|item| item.hostname.eq_ignore_ascii_case(&node.hostname)) {
            assign_hardware(node, entry);
        }
    }
}

pub fn refresh_vram_fields(node: &mut SwarmNode) {
    let (gb, tier) = vram_rank(node);
    node.effective_vram_gb = gb;
    node.vram_source = match tier {
        3 => "measured",
        2 => "configured",
        1 => "inferred",
        _ => "none",
    }
    .into();
}

#[cfg(test)]
mod hardware_tests {
    use super::*;

    #[test]
    fn search_matches_cpu_gpu_ram_and_vram() {
        let mut node = bare_node("local", "FamilyRoom");
        assign_hardware(
            &mut node,
            &HardwareEntry {
                hostname: "FamilyRoom".into(),
                vram_gb: 24.0,
                ram_gb: 64.0,
                cpu: "Ryzen 9 7950X".into(),
                gpu: "RTX 4090".into(),
            },
        );
        assert!(hardware_matches(&node, "4090"));
        assert!(hardware_matches(&node, "ryzen 64"));
        assert!(hardware_matches(&node, "24"));
        assert!(!hardware_matches(&node, "intel"));
        assert_eq!(node.vram_source, "configured");
        assert!((node.effective_vram_gb - 24.0).abs() < 0.01);
    }

    #[test]
    fn saved_hardware_roundtrips_and_applies_by_hostname() {
        let path = std::env::temp_dir().join(format!("murmuration-hardware-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        upsert_hardware(
            &path,
            HardwareEntry {
                hostname: "GamingPC".into(),
                vram_gb: 16.0,
                ram_gb: 32.0,
                cpu: "i7".into(),
                gpu: "RTX 4080".into(),
            },
        )
        .unwrap();
        let mut state = AppState::boot();
        state.nodes.push(bare_node("lm-game", "GamingPC"));
        let entries = load_hardware_entries(&path);
        for node in &mut state.nodes {
            if let Some(entry) = entries.iter().find(|item| item.hostname.eq_ignore_ascii_case(&node.hostname)) {
                assign_hardware(node, entry);
            }
        }
        let node = &state.nodes[0];
        assert_eq!(node.cpu_name, "i7");
        assert_eq!(node.gpu_name, "RTX 4080");
        assert!((node.ram_gb - 32.0).abs() < 0.01);
        assert!(hardware_matches(node, "4080 32"));
        let _ = std::fs::remove_file(&path);
    }
}

pub fn hostname_for(state: &AppState, id: &str) -> String {
    state
        .nodes
        .iter()
        .find(|node| node.id == id)
        .map(|node| node.hostname.clone())
        .unwrap_or_else(|| id.to_string())
}
