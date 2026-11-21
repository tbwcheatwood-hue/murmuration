use crate::state::{now_iso, AppState, GpuMetric, ModelArtifact, SwarmNode};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Probe {
    pub local_ip: String,
    pub local_os: String,
    pub local_gpus: Option<Vec<GpuMetric>>,
    pub local_thermal: Option<String>,
    pub local_upstream: bool,
    pub scanned_models: Vec<ModelArtifact>,
    pub notes: Vec<Note>,
    pub lm_link: Option<LmNetwork>,
    pub link_error: Option<String>,
    pub local_inferred_gb: f64,
}

#[derive(Clone)]
pub struct Note {
    pub node_id: String,
    pub source: String,
    pub level: String,
    pub message: String,
}

pub struct NvidiaSample {
    pub gpus: Vec<GpuMetric>,
    pub temp_c: Option<f64>,
}

pub enum NvidiaStatus {
    Sample(NvidiaSample),
    Missing,
    Unreadable(String),
}

pub fn probe() -> Probe {
    let mut notes = Vec::new();
    let mut local_ip = "127.0.0.1".to_string();
    let mut local_os = std::env::consts::OS.to_string();

    match capture("tailscale", &["status", "--json"], Duration::from_millis(1800)) {
        Some(raw) => match parse_tailscale(&raw) {
            Ok(parsed) => {
                if let Some(ip) = parsed.ip {
                    local_ip = ip;
                }
                if let Some(os) = parsed.os {
                    local_os = os;
                }
                notes.push(Note {
                    node_id: "local".into(),
                    source: "tailscale".into(),
                    level: "info".into(),
                    message: format!(
                        "Tailscale self is {}. {} peers stay off the LM Link list.",
                        parsed.hostname.unwrap_or_else(|| "unknown".into()),
                        parsed.peers.len()
                    ),
                });
            }
            Err(err) => notes.push(Note {
                node_id: "local".into(),
                source: "tailscale".into(),
                level: "warn".into(),
                message: format!("tailscale status --json could not be parsed: {err}"),
            }),
        },
        None => notes.push(Note {
            node_id: "local".into(),
            source: "tailscale".into(),
            level: "warn".into(),
            message: "tailscale status is not available. LM Link membership does not use it.".into(),
        }),
    }

    let local_upstream = port_open("127.0.0.1", 1234);
    if local_upstream {
        notes.push(Note {
            node_id: "local".into(),
            source: "lms".into(),
            level: "info".into(),
            message: "LM Studio is accepting connections on 127.0.0.1:1234.".into(),
        });
    }

    let (lm_link, link_error) = match capture_lms(&["link", "status", "--json"], Duration::from_millis(2500)) {
        Some(raw) => match parse_lm_link(&raw) {
            Ok(network) => {
                let names = std::iter::once(network.self_device.name.as_str())
                    .chain(network.peers.iter().map(|peer| peer.name.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ");
                notes.push(Note {
                    node_id: "local".into(),
                    source: "lms".into(),
                    level: "info".into(),
                    message: format!("LM Link lists {}.", names),
                });
                (Some(network), None)
            }
            Err(err) => {
                let message = format!("lms link status could not be read: {err}");
                notes.push(Note {
                    node_id: "local".into(),
                    source: "lms".into(),
                    level: "warn".into(),
                    message: message.clone(),
                });
                (None, Some(message))
            }
        },
        None => {
            let message = "lms link status is not available. This list stays empty until LM Studio reports the devices on this account.".into();
            notes.push(Note {
                node_id: "local".into(),
                source: "lms".into(),
                level: "warn".into(),
                message: message,
            });
            (None, Some("lms link status is not available.".into()))
        }
    };

    let (local_gpus, local_thermal) = match explain_nvidia() {
        NvidiaStatus::Sample(sample) => {
            let count = sample.gpus.len();
            notes.push(Note {
                node_id: "local".into(),
                source: "telemetry".into(),
                level: "info".into(),
                message: format!("nvidia-smi reported {count} GPU(s)."),
            });
            let thermal = sample.temp_c.map(thermal_from_temp);
            (Some(sample.gpus), thermal)
        }
        NvidiaStatus::Unreadable(detail) => {
            notes.push(Note {
                node_id: "local".into(),
                source: "telemetry".into(),
                level: "warn".into(),
                message: format!("nvidia-smi ran and did not report VRAM. {detail} Local measured VRAM stays 0 until the GPU responds."),
            });
            (None, None)
        }
        NvidiaStatus::Missing => {
            notes.push(Note {
                node_id: "local".into(),
                source: "telemetry".into(),
                level: "info".into(),
                message: "nvidia-smi is not available (PATH, System32, and NVSMI were checked). Local measured VRAM stays 0 until a probe or sidecar succeeds.".into(),
            });
            (None, None)
        }
    };

    let mut scanned_models = scan_model_dirs();
    if scanned_models.is_empty() {
        notes.push(Note {
            node_id: "local".into(),
            source: "lms".into(),
            level: "info".into(),
            message: "No local .gguf or .mlx files were found in the LM Studio model folders.".into(),
        });
    } else {
        notes.push(Note {
            node_id: "local".into(),
            source: "lms".into(),
            level: "info".into(),
            message: format!("Cataloged {} model file(s) on this machine.", scanned_models.len()),
        });
    }

    if let Some(raw) = capture("lms", &["ls", "--json"], Duration::from_millis(1600)) {
        match models_from_lms(&raw) {
            Some(found) if !found.is_empty() => {
                notes.push(Note {
                    node_id: "local".into(),
                    source: "lms".into(),
                    level: "info".into(),
                    message: format!("lms ls reported {} model(s).", found.len()),
                });
                for model in found {
                    if !scanned_models.iter().any(|item| item.id == model.id) {
                        scanned_models.push(model);
                    }
                }
            }
            Some(_) => {}
            None => notes.push(Note {
                node_id: "local".into(),
                source: "lms".into(),
                level: "warn".into(),
                message: "lms ls did not return JSON. The file scan is the catalog.".into(),
            }),
        }
    }

    Probe {
        local_ip,
        local_os,
        local_gpus,
        local_thermal,
        local_upstream,
        scanned_models,
        notes,
        lm_link,
        link_error,
        local_inferred_gb: inferred_from_lms_ps(),
    }
}

pub fn apply(state: &mut AppState, probe: Probe) {
    if let Some(network) = probe.lm_link.clone() {
        state.nodes = reconcile_lm_link(&state.nodes, &network);
        state.demo_mesh = false;
        state.live_peer_count = network.peers.iter().filter(|peer| link_is_up(&peer.status)).count();
        state.mesh_source = "lm-link".into();
        state.link_status = if network.status.is_empty() { "online".into() } else { network.status.clone() };
        state.link_message = format!("{} · {} other device(s) on this LM Link.", network.self_device.name, network.peers.len());
    } else {
        state.nodes.retain(|node| !node.simulated);
        state.mesh_source = "unavailable".into();
        state.link_status = "unavailable".into();
        state.link_message = probe
            .link_error
            .clone()
            .unwrap_or_else(|| "LM Link did not identify the devices on this account.".into());
    }

    if let Some(local) = state.node_mut("local") {
        if !probe.local_ip.is_empty() && probe.local_ip != "127.0.0.1" {
            local.tailscale_ip = probe.local_ip;
        }
        if !probe.local_os.is_empty() {
            local.os = probe.local_os;
        }
        local.upstream_alive = probe.local_upstream;
        if probe.local_upstream {
            local.lm_link_bound = true;
        }
        local.last_seen = now_iso();
        if let Some(thermal) = probe.local_thermal.clone() {
            local.thermal = thermal;
        }
        if let Some(gpus) = probe.local_gpus.clone() {
            if !gpus.is_empty() {
                local.gpus = gpus;
            }
        }
    }

    for model in probe.scanned_models {
        if state.models.iter().any(|item| item.id == model.id || (item.path.is_some() && item.path == model.path)) {
            continue;
        }
        state.models.push(model);
    }
    state.models.retain(|model| !model.simulated);

    let file_floor = state
        .models
        .iter()
        .filter(|model| !model.simulated)
        .map(|model| model.size_gb)
        .fold(0.0_f64, f64::max);
    apply_capacity(state, probe.local_inferred_gb, file_floor);

    for note in probe.notes {
        state.log(&note.node_id, &note.source, &note.level, note.message);
    }
}

pub fn sample_nvidia() -> Option<NvidiaSample> {
    match explain_nvidia() {
        NvidiaStatus::Sample(sample) => Some(sample),
        NvidiaStatus::Missing | NvidiaStatus::Unreadable(_) => None,
    }
}

pub fn explain_nvidia() -> NvidiaStatus {
    let query = [
        "--query-gpu=name,memory.total,memory.free,utilization.gpu,temperature.gpu",
        "--format=csv,noheader,nounits",
    ];
    for program in nvidia_smi_candidates() {
        if program.contains('\\') || program.contains('/') {
            if !Path::new(&program).exists() {
                continue;
            }
        }
        if let Some((stdout, stderr)) = capture_streams(&program, &query, Duration::from_millis(1200)) {
            let parsed = parse_nvidia_csv(&stdout);
            if !parsed.gpus.is_empty() {
                return NvidiaStatus::Sample(parsed);
            }
            let detail = first_line(&stdout).or_else(|| first_line(&stderr)).unwrap_or_else(|| "nvidia-smi returned no GPU rows.".into());
            return NvidiaStatus::Unreadable(detail);
        }
    }
    NvidiaStatus::Missing
}

fn first_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let mut line = line.to_string();
    if line.len() > 180 {
        line.truncate(177);
        line.push_str("...");
    }
    Some(line)
}

pub fn nvidia_smi_candidates() -> Vec<String> {
    let mut paths = vec!["nvidia-smi".to_string()];
    if cfg!(windows) {
        paths.push(r"C:\Windows\System32\nvidia-smi.exe".into());
        paths.push(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe".into());
    } else {
        paths.push("/usr/bin/nvidia-smi".into());
        paths.push("/usr/local/nvidia/bin/nvidia-smi".into());
    }
    paths
}

pub fn ping_ms(ip: &str) -> Option<u32> {
    if ip.parse::<std::net::Ipv4Addr>().is_err() {
        return None;
    }
    let output = capture("ping", &["-n", "1", "-w", "400", ip], Duration::from_millis(900))?;
    if output.contains("time<1") {
        return Some(1);
    }
    let marker = "time=";
    let index = output.find(marker)?;
    let rest = &output[index + marker.len()..];
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

pub(crate) struct ParsedTailscale {
    hostname: Option<String>,
    ip: Option<String>,
    os: Option<String>,
    peers: Vec<SwarmNode>,
}

pub fn parse_tailscale(raw: &str) -> Result<ParsedTailscale, String> {
    let status: TsStatus = serde_json::from_str(raw).map_err(|err| err.to_string())?;
    let hostname = status.this.as_ref().and_then(|peer| peer.host_name.clone());
    let ip = status.this.as_ref().and_then(first_v4);
    let os = status.this.as_ref().and_then(|peer| peer.os.clone());
    let mut peers = Vec::new();
    if let Some(map) = status.peer {
        let mut entries: Vec<_> = map.into_iter().collect();
        entries.sort_by(|a, b| a.1.host_name.cmp(&b.1.host_name));
        for (index, (_key, peer)) in entries.into_iter().take(24).enumerate() {
            let Some(ip) = first_v4(&peer) else {
                continue;
            };
            let hostname = peer
                .host_name
                .clone()
                .or_else(|| peer.dns_name.clone().map(|dns| dns.split('.').next().unwrap_or("peer").to_string()))
                .unwrap_or_else(|| format!("peer-{index}"));
            if hostname.eq_ignore_ascii_case(hostname_of(status.this.as_ref())) {
                continue;
            }
            let online = peer.online.unwrap_or(false);
            let relay_name = peer.relay.unwrap_or_default();
            let cur = peer.cur_addr.unwrap_or_default();
            let relay = if !cur.is_empty() {
                "direct"
            } else if !relay_name.is_empty() {
                "derp"
            } else {
                "unknown"
            };
            peers.push(SwarmNode {
                id: format!("ts-{}", slug(&hostname)),
                hostname,
                tailscale_ip: ip,
                os: peer.os.unwrap_or_else(|| "unknown".into()),
                ping_ms: if relay == "derp" { 45 } else { 8 },
                relay: relay.into(),
                derp_region: if relay == "derp" && !relay_name.is_empty() {
                    Some(relay_name)
                } else {
                    None
                },
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
                status: if online { "online" } else { "offline" }.into(),
                throughput_mbps: 0.2,
                color: PEER_COLORS[index % PEER_COLORS.len()].into(),
                last_seen: now_iso(),
                ping_base: if relay == "derp" { 45 } else { 8 },
                util_base: 0.0,
                token_base: 0.0,
                announced_down: !online,
                ping_misses: 0,
            });
        }
    }
    Ok(ParsedTailscale {
        hostname,
        ip,
        os,
        peers,
    })
}

fn hostname_of(peer: Option<&TsPeer>) -> &str {
    peer.and_then(|item| item.host_name.as_deref()).unwrap_or("")
}

pub fn parse_nvidia_csv(text: &str) -> NvidiaSample {
    let mut gpus = Vec::new();
    let mut temp_c = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').map(|part| part.trim()).collect();
        if parts.len() < 4 {
            continue;
        }
        let total_mib: f64 = parts[1].parse().unwrap_or(0.0);
        let free_mib: f64 = parts[2].parse().unwrap_or(0.0);
        let util: f64 = parts[3].parse().unwrap_or(0.0);
        if total_mib <= 0.0 {
            continue;
        }
        if parts.len() >= 5 {
            if let Ok(temp) = parts[4].parse::<f64>() {
                temp_c = Some(temp);
            }
        }
        let used_mib = (total_mib - free_mib).max(0.0);
        gpus.push(GpuMetric {
            name: parts[0].to_string(),
            vram_total_gb: round1(total_mib / 1024.0),
            vram_allocated_gb: round1(used_mib / 1024.0),
            utilization: util,
        });
    }
    NvidiaSample { gpus, temp_c }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn thermal_from_temp(temp: f64) -> String {
    if temp >= 90.0 {
        "throttling".into()
    } else if temp >= 75.0 {
        "hot".into()
    } else if temp >= 55.0 {
        "warm".into()
    } else {
        "cool".into()
    }
}

fn scan_model_dirs() -> Vec<ModelArtifact> {
    let mut roots = Vec::new();
    if let Ok(custom) = std::env::var("LMSTUDIO_MODELS") {
        roots.push(PathBuf::from(custom));
    }
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let home = PathBuf::from(home);
        roots.push(home.join(".lmstudio").join("models"));
        roots.push(home.join(".cache").join("lm-studio").join("models"));
    }
    let mut out = Vec::new();
    let mut budget = 200usize;
    for root in roots {
        if root.is_dir() {
            scan_dir(&root, &mut out, 5, &mut budget);
        }
    }
    out
}

fn scan_dir(dir: &Path, out: &mut Vec<ModelArtifact>, depth: u8, budget: &mut usize) {
    if depth == 0 || *budget == 0 {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        if *budget == 0 {
            return;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "node_modules" || name == ".git" || name == "target" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if name.ends_with(".mlx") {
                *budget = budget.saturating_sub(1);
                out.push(model_from_path(&path, dir_size(&path)));
            } else {
                scan_dir(&path, out, depth - 1, budget);
            }
        } else if name.ends_with(".gguf") || name.ends_with(".mlx") {
            *budget = budget.saturating_sub(1);
            let len = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            out.push(model_from_path(&path, len));
        }
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(read) = std::fs::read_dir(path) else {
        return 0;
    };
    read.flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum()
}

fn model_from_path(path: &Path, bytes: u64) -> ModelArtifact {
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("model")
        .to_string();
    let format = if path.to_string_lossy().ends_with(".mlx") { "mlx" } else { "gguf" };
    let params = params_from_name(&name);
    let (layers, kv_heads, head_dim, ctx) = arch_for(params);
    let quant = quant_from_name(&name);
    ModelArtifact {
        id: format!("disk-{:x}", fnv(&path.to_string_lossy())),
        name,
        format: format.into(),
        quant,
        size_gb: round1(bytes as f64 / 1_073_741_824.0),
        params_b: params,
        host_node_id: "local".into(),
        architecture: "local".into(),
        context_max: ctx,
        layers,
        kv_heads,
        head_dim,
        loaded: false,
        simulated: false,
        sharded: false,
        shard_nodes: Vec::new(),
        path: Some(path.display().to_string()),
    }
}

fn models_from_lms(raw: &str) -> Option<Vec<ModelArtifact>> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let list = value
        .as_array()
        .cloned()
        .or_else(|| value.get("models").and_then(|item| item.as_array()).cloned())
        .or_else(|| value.get("data").and_then(|item| item.as_array()).cloned())?;
    let mut models = Vec::new();
    for item in list {
        let name = item
            .get("name")
            .or_else(|| item.get("modelKey"))
            .or_else(|| item.get("id"))
            .and_then(|value| value.as_str())
            .unwrap_or("model")
            .to_string();
        let path = item
            .get("path")
            .or_else(|| item.get("file"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let bytes = item
            .get("sizeBytes")
            .or_else(|| item.get("size"))
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let mut model = model_from_path(Path::new(&path.clone().unwrap_or(name.clone())), bytes);
        model.name = name;
        model.path = path;
        model.id = format!("lms-{:x}", fnv(&model.name));
        models.push(model);
    }
    Some(models)
}

fn params_from_name(name: &str) -> f64 {
    let lower = name.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit() {
            let start = index;
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
                index += 1;
            }
            if index < bytes.len() && bytes[index] == b'b' {
                if let Ok(value) = lower[start..index].parse::<f64>() {
                    return value;
                }
            }
        }
        index += 1;
    }
    7.0
}

fn quant_from_name(name: &str) -> String {
    let upper = name.to_ascii_uppercase();
    for token in ["Q8_0", "Q6_K", "Q5_K_M", "Q5_K_S", "Q4_K_M", "Q4_K_S", "Q3_K_M", "MXFP4", "FP16"] {
        if upper.contains(token) {
            return token.to_string();
        }
    }
    "unknown".into()
}

fn arch_for(params_b: f64) -> (u32, u32, u32, u32) {
    if params_b >= 300.0 {
        (126, 8, 128, 32768)
    } else if params_b >= 60.0 {
        (80, 8, 128, 32768)
    } else if params_b >= 24.0 {
        (64, 8, 128, 32768)
    } else if params_b >= 12.0 {
        (48, 8, 128, 32768)
    } else if params_b >= 6.0 {
        (32, 8, 128, 32768)
    } else if params_b >= 2.0 {
        (28, 8, 128, 32768)
    } else {
        (24, 2, 128, 8192)
    }
}

fn fnv(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Clone)]
pub struct LmDevice {
    pub identifier: String,
    pub name: String,
    pub status: String,
    pub loaded: Vec<String>,
    pub loaded_bytes: u64,
}

#[derive(Clone)]
pub struct LmNetwork {
    pub status: String,
    pub self_device: LmDevice,
    pub peers: Vec<LmDevice>,
    pub preferred_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LmLinkStatusJson {
    status: Option<String>,
    device_identifier: Option<String>,
    device_name: Option<String>,
    preferred_device_identifier: Option<String>,
    peers: Option<Vec<LmPeerJson>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LmPeerJson {
    device_identifier: Option<String>,
    device_name: Option<String>,
    status: Option<String>,
    loaded_models: Option<Vec<Value>>,
}

pub fn parse_lm_link(raw: &str) -> Result<LmNetwork, String> {
    let status: LmLinkStatusJson = serde_json::from_str(raw).map_err(|err| err.to_string())?;
    let identifier = status.device_identifier.unwrap_or_default();
    let name = status.device_name.unwrap_or_default();
    if identifier.is_empty() || name.is_empty() {
        return Err("lms link status did not include this device.".into());
    }
    let mut peers = Vec::new();
    for peer in status.peers.unwrap_or_default() {
        let Some(device) = device_from(peer.device_identifier, peer.device_name, peer.status, peer.loaded_models) else {
            continue;
        };
        if device.identifier == identifier || peers.iter().any(|item: &LmDevice| item.identifier == device.identifier) {
            continue;
        }
        peers.push(device);
    }
    peers.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    let link_status = status.status.clone().unwrap_or_else(|| "online".into());
    Ok(LmNetwork {
        status: link_status.clone(),
        self_device: LmDevice {
            identifier,
            name,
            status: link_status,
            loaded: Vec::new(),
            loaded_bytes: 0,
        },
        peers,
        preferred_id: status.preferred_device_identifier.unwrap_or_default(),
    })
}

fn device_from(identifier: Option<String>, name: Option<String>, status: Option<String>, loaded: Option<Vec<Value>>) -> Option<LmDevice> {
    let identifier = identifier.unwrap_or_default();
    let name = name.unwrap_or_default();
    if identifier.is_empty() || name.is_empty() {
        return None;
    }
    let models = loaded.unwrap_or_default();
    let loaded_bytes = models.iter().map(model_bytes).sum();
    Some(LmDevice {
        identifier,
        name,
        status: status.unwrap_or_else(|| "unknown".into()),
        loaded: models.iter().filter_map(model_label).collect(),
        loaded_bytes,
    })
}

fn model_bytes(value: &Value) -> u64 {
    let Some(object) = value.as_object() else {
        return 0;
    };
    for key in ["sizeBytes", "size_bytes", "fileSize"] {
        if let Some(bytes) = object.get(key).and_then(Value::as_u64) {
            return bytes;
        }
        if let Some(bytes) = object.get(key).and_then(Value::as_f64) {
            if bytes > 0.0 {
                return bytes as u64;
            }
        }
    }
    0
}

fn inferred_gb_from_loaded(device: &LmDevice) -> f64 {
    if device.loaded_bytes > 0 {
        return round1(device.loaded_bytes as f64 / 1_000_000_000.0);
    }
    if device.loaded.is_empty() {
        0.0
    } else {
        device.loaded.len() as f64
    }
}

fn model_label(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        let text = text.trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    let object = value.as_object()?;
    for key in ["displayName", "modelKey", "identifier", "name", "id"] {
        if let Some(text) = object.get(key).and_then(Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

pub fn reconcile_lm_link(existing: &[SwarmNode], network: &LmNetwork) -> Vec<SwarmNode> {
    let mut used = vec![false; existing.len()];
    let mut nodes = Vec::with_capacity(1 + network.peers.len());
    nodes.push(take_device(existing, &mut used, &network.self_device, true, &network.preferred_id, 0));
    for (index, peer) in network.peers.iter().enumerate() {
        nodes.push(take_device(existing, &mut used, peer, false, &network.preferred_id, index + 1));
    }
    nodes
}

fn take_device(existing: &[SwarmNode], used: &mut [bool], device: &LmDevice, is_self: bool, preferred_id: &str, color_index: usize) -> SwarmNode {
    let id = if is_self { "local".to_string() } else { format!("lm-{}", device.identifier) };
    let found = existing.iter().enumerate().find(|(index, node)| {
        if used[*index] || node.simulated {
            return false;
        }
        if is_self && node.id == "local" {
            return true;
        }
        node.id == id || (!device.name.is_empty() && node.hostname.eq_ignore_ascii_case(&device.name))
    });
    let mut node = if let Some((index, previous)) = found {
        used[index] = true;
        previous.clone()
    } else {
        blank_link_node(&id, color_index)
    };
    node.id = id;
    node.hostname = device.name.clone();
    node.status = map_link_status(&device.status);
    node.lm_link_bound = true;
    node.simulated = false;
    node.relay = "lm-link".into();
    node.role = link_role(device, is_self, preferred_id);
    let from_models = inferred_gb_from_loaded(device);
    if from_models > node.inferred_vram_gb {
        node.inferred_vram_gb = from_models;
    }
    node.upstream_alive = node.upstream_alive || (is_self && link_is_up(&device.status));
    node.last_seen = now_iso();
    node
}

fn blank_link_node(id: &str, color_index: usize) -> SwarmNode {
    SwarmNode {
        id: id.into(),
        hostname: String::new(),
        tailscale_ip: String::new(),
        os: "LM Link".into(),
        ping_ms: 0,
        relay: "lm-link".into(),
        derp_region: None,
        lm_link_bound: true,
        lm_link_port: None,
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
        role: None,
        simulated: false,
        status: "offline".into(),
        throughput_mbps: 0.0,
        color: PEER_COLORS[color_index % PEER_COLORS.len()].into(),
        last_seen: now_iso(),
        ping_base: 0,
        util_base: 0.0,
        token_base: 0.0,
        announced_down: false,
        ping_misses: 0,
    }
}

fn link_role(device: &LmDevice, is_self: bool, preferred_id: &str) -> Option<String> {
    let mut parts = Vec::new();
    if is_self {
        parts.push("this device".to_string());
    }
    if !preferred_id.is_empty() && device.identifier == preferred_id {
        parts.push("preferred".to_string());
    }
    if !device.loaded.is_empty() {
        parts.push(device.loaded.join(", "));
    }
    if parts.is_empty() { None } else { Some(parts.join(" · ")) }
}

fn map_link_status(status: &str) -> String {
    match status.to_ascii_lowercase().as_str() {
        "online" | "connected" | "ready" => "online".into(),
        "offline" | "disconnected" | "unavailable" => "offline".into(),
        _ => "degraded".into(),
    }
}

fn link_is_up(status: &str) -> bool {
    matches!(status.to_ascii_lowercase().as_str(), "online" | "connected" | "ready")
}

fn capture_lms(args: &[&str], timeout: Duration) -> Option<String> {
    if let Some(raw) = capture("lms", args, timeout) {
        return Some(raw);
    }
    let home = std::env::var("USERPROFILE").ok()?;
    let exe = Path::new(&home).join(".lmstudio").join("bin").join("lms.exe");
    if !exe.exists() {
        return None;
    }
    capture(exe.to_str()?, args, timeout)
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MeshManifest {
    #[serde(default)]
    telemetry_port: Option<u16>,
    #[serde(default)]
    nodes: Vec<ManifestNode>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestNode {
    hostname: String,
    #[serde(default)]
    vram_gb: Option<f64>,
    #[serde(default)]
    telemetry_host: Option<String>,
}

fn load_manifest() -> MeshManifest {
    for path in manifest_paths() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(manifest) = serde_json::from_str::<MeshManifest>(&text) {
            return manifest;
        }
    }
    MeshManifest {
        telemetry_port: None,
        nodes: Vec::new(),
    }
}

fn manifest_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(custom) = std::env::var("MURMURATION_MESH_MANIFEST") {
        paths.push(PathBuf::from(custom));
    }
    paths.push(PathBuf::from("mesh.manifest.json"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("mesh.manifest.json"));
        }
    }
    paths
}

fn telemetry_port(manifest: &MeshManifest) -> u16 {
    if let Ok(raw) = std::env::var("MURMURATION_TELEMETRY_PORT") {
        if let Ok(port) = raw.parse::<u16>() {
            if port > 0 {
                return port;
            }
        }
    }
    manifest.telemetry_port.unwrap_or(9471)
}

pub fn apply_capacity(state: &mut AppState, local_inferred_gb: f64, local_file_gb: f64) {
    let manifest = load_manifest();
    let port = telemetry_port(&manifest);
    let hosts: Vec<(String, String, bool)> = state
        .nodes
        .iter()
        .map(|node| {
            let entry = manifest
                .nodes
                .iter()
                .find(|item| item.hostname.eq_ignore_ascii_case(&node.hostname));
            let host = entry
                .and_then(|item| item.telemetry_host.clone())
                .filter(|host| !host.is_empty())
                .or_else(|| {
                    if node.id != "local" && !node.tailscale_ip.is_empty() {
                        Some(node.tailscale_ip.clone())
                    } else if node.id == "local" {
                        Some("127.0.0.1".into())
                    } else {
                        None
                    }
                });
            (node.id.clone(), host.unwrap_or_default(), node.id == "local")
        })
        .collect();

    for node in &mut state.nodes {
        if let Some(entry) = manifest.nodes.iter().find(|item| item.hostname.eq_ignore_ascii_case(&node.hostname)) {
            if let Some(gb) = entry.vram_gb {
                if gb > 0.0 {
                    node.configured_vram_gb = gb;
                }
            }
        }
        if node.id == "local" {
            let floor = if local_inferred_gb > 0.0 { local_inferred_gb } else { local_file_gb };
            if floor > node.inferred_vram_gb {
                node.inferred_vram_gb = round1(floor);
            }
        }
    }

    for (id, host, is_local) in hosts {
        let needs_measure = state
            .node_mut(&id)
            .map(|node| node.gpus.iter().all(|gpu| gpu.vram_total_gb <= 0.0))
            .unwrap_or(false);
        if !needs_measure || host.is_empty() {
            continue;
        }
        if is_local && state.node_mut(&id).and_then(|node| node.gpus.first()).is_some() {
            continue;
        }
        if let Some(gpus) = fetch_peer_metrics(&host, port) {
            if let Some(node) = state.node_mut(&id) {
                if !gpus.is_empty() {
                    node.gpus = gpus;
                    node.upstream_alive = node.upstream_alive || !is_local;
                }
            }
        }
    }

    crate::state::apply_saved_hardware(state);
    let mut summary = Vec::new();
    for node in &mut state.nodes {
        crate::state::refresh_vram_fields(node);
        summary.push(format!("{} {:.1} GB {}", node.hostname, node.effective_vram_gb, node.vram_source));
    }
    if !summary.is_empty() {
        state.log("local", "telemetry", "info", format!("Effective VRAM: {}.", summary.join(", ")));
    }
}

fn inferred_from_lms_ps() -> f64 {
    let Some(raw) = capture_lms(&["ps", "--json"], Duration::from_millis(1600)) else {
        return 0.0;
    };
    inferred_gb_from_ps(&raw)
}

pub fn inferred_gb_from_ps(raw: &str) -> f64 {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return 0.0;
    };
    let Some(models) = value.as_array() else {
        return 0.0;
    };
    let bytes: u64 = models.iter().map(model_bytes).sum();
    if bytes > 0 {
        return round1(bytes as f64 / 1_000_000_000.0);
    }
    if models.is_empty() { 0.0 } else { models.len() as f64 }
}

pub fn gpus_from_metrics(body: &str) -> Vec<GpuMetric> {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    if let Some(list) = value.get("gpus").and_then(Value::as_array) {
        let gpus: Vec<GpuMetric> = list
            .iter()
            .filter_map(|item| {
                let total = item.get("vram_total_mb").and_then(Value::as_f64).unwrap_or(0.0);
                if total <= 0.0 {
                    return None;
                }
                let free = item.get("vram_free_mb").and_then(Value::as_f64).unwrap_or(total);
                let util = item.get("gpu_util_pct").and_then(Value::as_f64).unwrap_or(0.0);
                let name = item.get("name").and_then(Value::as_str).unwrap_or("GPU").to_string();
                Some(GpuMetric {
                    name,
                    vram_total_gb: round1(total / 1024.0),
                    vram_allocated_gb: round1((total - free).max(0.0) / 1024.0),
                    utilization: util,
                })
            })
            .collect();
        if !gpus.is_empty() {
            return gpus;
        }
    }
    let total = value.get("vram_total_mb").and_then(Value::as_f64).unwrap_or(0.0);
    if total <= 0.0 {
        return Vec::new();
    }
    let free = value.get("vram_free_mb").and_then(Value::as_f64).unwrap_or(total);
    let util = value.get("gpu_util_pct").and_then(Value::as_f64).unwrap_or(0.0);
    let count = value.get("gpu_count").and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
    let share = total / count as f64;
    let free_share = free / count as f64;
    (0..count)
        .map(|index| GpuMetric {
            name: format!("GPU {index}"),
            vram_total_gb: round1(share / 1024.0),
            vram_allocated_gb: round1((share - free_share).max(0.0) / 1024.0),
            utilization: util,
        })
        .collect()
}

fn fetch_peer_metrics(host: &str, port: u16) -> Option<Vec<GpuMetric>> {
    use std::io::{Read, Write};
    use std::net::ToSocketAddrs;
    let socket = if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        std::net::SocketAddr::new(ip, port)
    } else {
        format!("{host}:{port}").to_socket_addrs().ok()?.find(|addr| addr.is_ipv4())?
    };
    let mut stream = std::net::TcpStream::connect_timeout(&socket, Duration::from_millis(350)).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(350))).ok()?;
    let request = format!("GET /metrics HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).ok()?;
    let body = raw.split_once("\r\n\r\n").map(|(_, body)| body).unwrap_or(raw.as_str());
    let gpus = gpus_from_metrics(body);
    if gpus.is_empty() { None } else { Some(gpus) }
}

fn slug(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "peer".into() } else { out }
}

fn first_v4(peer: &TsPeer) -> Option<String> {
    peer.ips.as_ref()?.iter().find(|ip| ip.contains('.')).cloned()
}

fn port_open(ip: &str, port: u16) -> bool {
    let Ok(addr) = format!("{ip}:{port}").parse::<std::net::SocketAddr>() else {
        return false;
    };
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(180)).is_ok()
}

pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW. The app is a GUI process, so a console child
        // otherwise flashes a window on every nvidia-smi, lms, or ping call.
        command.creation_flags(0x08000000);
    }
    command
}

fn capture(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    capture_streams(program, args, timeout).map(|(stdout, _)| stdout)
}

fn capture_streams(program: &str, args: &[&str], timeout: Duration) -> Option<(String, String)> {
    let mut child = command(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let mut out = String::new();
                let mut err = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut out);
                }
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr.read_to_string(&mut err);
                }
                return Some((out, err));
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }
}

const PEER_COLORS: [&str; 6] = ["#9AD0F5", "#F0A6CA", "#C6E07E", "#F0C674", "#B4A7FF", "#7ED0C8"];

#[derive(Deserialize)]
struct TsStatus {
    #[serde(rename = "Self")]
    this: Option<TsPeer>,
    #[serde(rename = "Peer")]
    peer: Option<HashMap<String, TsPeer>>,
}

#[derive(Deserialize)]
struct TsPeer {
    #[serde(rename = "HostName")]
    host_name: Option<String>,
    #[serde(rename = "DNSName")]
    dns_name: Option<String>,
    #[serde(rename = "TailscaleIPs")]
    ips: Option<Vec<String>>,
    #[serde(rename = "Online")]
    online: Option<bool>,
    #[serde(rename = "OS")]
    os: Option<String>,
    #[serde(rename = "Relay")]
    relay: Option<String>,
    #[serde(rename = "CurAddr")]
    cur_addr: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_direct_and_derp_peers() {
        let raw = r#"{
            "Self": {"HostName": "devbox", "TailscaleIPs": ["100.64.1.2"], "Online": true, "OS": "windows"},
            "Peer": {
                "a": {"HostName": "studio", "TailscaleIPs": ["100.64.1.9"], "Online": true, "OS": "linux", "Relay": "", "CurAddr": "203.0.113.8:41641"},
                "b": {"HostName": "mbp", "TailscaleIPs": ["100.64.1.10"], "Online": false, "OS": "macOS", "Relay": "iad", "CurAddr": ""}
            }
        }"#;
        let parsed = parse_tailscale(raw).unwrap();
        assert_eq!(parsed.hostname.as_deref(), Some("devbox"));
        assert_eq!(parsed.peers.len(), 2);
        let studio = parsed.peers.iter().find(|peer| peer.hostname == "studio").unwrap();
        assert_eq!(studio.relay, "direct");
        assert_eq!(studio.status, "online");
        let mbp = parsed.peers.iter().find(|peer| peer.hostname == "mbp").unwrap();
        assert_eq!(mbp.relay, "derp");
        assert_eq!(mbp.status, "offline");
        assert_eq!(mbp.derp_region.as_deref(), Some("iad"));
    }

    const LINK: &str = r#"{
        "status": "online",
        "peers": [
            {"deviceIdentifier": "peer-tim", "deviceName": "Timmys-PC", "status": "connected", "loadedModels": []},
            {"deviceIdentifier": "peer-game", "deviceName": "GamingPC", "status": "connected", "loadedModels": [{"identifier": "qwen"}]}
        ],
        "deviceIdentifier": "self-id",
        "deviceName": "FamilyRoom",
        "preferredDeviceIdentifier": "peer-tim"
    }"#;

    fn named(id: &str, hostname: &str, simulated: bool) -> SwarmNode {
        let mut node = blank_link_node(id, 0);
        node.hostname = hostname.into();
        node.simulated = simulated;
        node
    }

    #[test]
    fn filters_outsiders_and_adds_missing_lm_link_devices() {
        let network = parse_lm_link(LINK).unwrap();
        let mut gaming = named("old-game", "GamingPC", false);
        gaming.gpus.push(GpuMetric {
            name: "RTX".into(),
            vram_total_gb: 24.0,
            vram_allocated_gb: 1.0,
            utilization: 3.0,
        });
        let existing = vec![
            named("local", "this-machine", false),
            named("atlas", "atlas-4090", true),
            named("stranger", "not-on-link", false),
            gaming,
        ];
        let next = reconcile_lm_link(&existing, &network);
        let names: Vec<_> = next.iter().map(|node| node.hostname.as_str()).collect();
        assert_eq!(names, vec!["FamilyRoom", "GamingPC", "Timmys-PC"]);
        assert!(next.iter().all(|node| !node.simulated));
        let gaming = next.iter().find(|node| node.hostname == "GamingPC").unwrap();
        assert_eq!(gaming.id, "lm-peer-game");
        assert_eq!(gaming.gpus.len(), 1);
        assert_eq!(gaming.gpus[0].vram_total_gb, 24.0);
        assert_eq!(gaming.role.as_deref(), Some("qwen"));
        let preferred = next.iter().find(|node| node.hostname == "Timmys-PC").unwrap();
        assert_eq!(preferred.id, "lm-peer-tim");
        assert_eq!(preferred.role.as_deref(), Some("preferred"));
        assert_eq!(next[0].role.as_deref(), Some("this device"));
    }

    #[test]
    fn adds_every_lm_link_device_when_the_list_was_empty() {
        let network = parse_lm_link(LINK).unwrap();
        let next = reconcile_lm_link(&[], &network);
        assert_eq!(next.len(), 3);
        assert_eq!(next[0].id, "local");
        assert_eq!(next[0].hostname, "FamilyRoom");
        assert!(next.iter().any(|node| node.hostname == "Timmys-PC"));
        assert!(next.iter().any(|node| node.hostname == "GamingPC"));
    }

    #[test]
    fn apply_hides_outsiders_without_dropping_unrelated_models() {
        let mut state = AppState::boot();
        state.nodes.push(named("atlas", "atlas-4090", true));
        state.models.push(ModelArtifact {
            id: "local-file".into(),
            name: "kept-on-disk".into(),
            format: "gguf".into(),
            quant: "Q4".into(),
            size_gb: 1.0,
            params_b: 1.0,
            host_node_id: "local".into(),
            architecture: "llama".into(),
            context_max: 2048,
            layers: 1,
            kv_heads: 1,
            head_dim: 1,
            loaded: false,
            simulated: false,
            sharded: false,
            shard_nodes: Vec::new(),
            path: Some("C:/models/kept.gguf".into()),
        });
        let network = parse_lm_link(LINK).unwrap();
        apply(
            &mut state,
            Probe {
                local_ip: String::new(),
                local_os: "windows".into(),
                local_gpus: None,
                local_thermal: None,
                local_upstream: false,
                scanned_models: Vec::new(),
                notes: Vec::new(),
                lm_link: Some(network),
                link_error: None,
                local_inferred_gb: 0.0,
            },
        );
        let names: Vec<_> = state.nodes.iter().map(|node| node.hostname.as_str()).collect();
        assert_eq!(names, vec!["FamilyRoom", "GamingPC", "Timmys-PC"]);
        assert_eq!(state.models.len(), 1);
        assert_eq!(state.models[0].id, "local-file");
        assert_eq!(state.mesh_source, "lm-link");
    }

    #[test]
    fn parses_nvidia_csv() {
        let sample = parse_nvidia_csv("NVIDIA GeForce RTX 4090, 24564, 2048, 15, 47\n");
        assert_eq!(sample.gpus.len(), 1);
        assert_eq!(sample.gpus[0].name, "NVIDIA GeForce RTX 4090");
        assert!((sample.gpus[0].vram_total_gb - 24.0).abs() < 0.1);
        assert!((sample.gpus[0].vram_allocated_gb - 22.0).abs() < 0.1);
        assert_eq!(sample.gpus[0].utilization, 15.0);
        assert_eq!(sample.temp_c, Some(47.0));
    }

    #[test]
    fn nvidia_candidates_cover_windows_and_linux_paths() {
        let paths = nvidia_smi_candidates();
        assert!(paths.iter().any(|path| path == "nvidia-smi"));
        if cfg!(windows) {
            assert!(paths.iter().any(|path| path.contains("System32")));
            assert!(paths.iter().any(|path| path.contains("NVSMI")));
        } else {
            assert!(paths.iter().any(|path| path == "/usr/bin/nvidia-smi"));
            assert!(paths.iter().any(|path| path.contains("/usr/local/nvidia/bin")));
        }
    }

    #[test]
    fn loaded_model_bytes_become_inferred_vram() {
        let network = parse_lm_link(
            r#"{"status":"online","deviceIdentifier":"self","deviceName":"FamilyRoom","peers":[{"deviceIdentifier":"peer","deviceName":"GamingPC","status":"connected","loadedModels":[{"identifier":"qwen","sizeBytes":10400000000}]}]}"#,
        )
        .unwrap();
        let nodes = reconcile_lm_link(&[], &network);
        let gaming = nodes.iter().find(|node| node.hostname == "GamingPC").unwrap();
        assert!(gaming.inferred_vram_gb > 10.0);
        let mut state = AppState::boot();
        state.nodes = nodes;
        apply_capacity(&mut state, 9.7, 0.0);
        let local = state.nodes.iter().find(|node| node.id == "local").unwrap();
        assert!((local.inferred_vram_gb - 9.7).abs() < 0.05);
        assert_eq!(local.vram_source, "inferred");
        assert!(local.effective_vram_gb > 0.0);
    }

    #[test]
    fn metrics_json_reports_total_free_and_utilization() {
        let gpus = gpus_from_metrics(r#"{"vram_total_mb":24576,"vram_free_mb":8192,"gpu_util_pct":12.5,"gpu_count":1}"#);
        assert_eq!(gpus.len(), 1);
        assert!((gpus[0].vram_total_gb - 24.0).abs() < 0.1);
        assert!((gpus[0].vram_allocated_gb - 16.0).abs() < 0.1);
        assert_eq!(gpus[0].utilization, 12.5);
    }

    #[test]
    fn lms_ps_size_is_an_inferred_floor() {
        let gb = inferred_gb_from_ps(r#"[{"identifier":"qwen","sizeBytes":10449205696}]"#);
        assert!((gb - 10.4).abs() < 0.1);
    }
}
