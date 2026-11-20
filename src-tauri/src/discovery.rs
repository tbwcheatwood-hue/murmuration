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

    let nvidia = sample_nvidia();
    let (local_gpus, local_thermal) = if let Some(sample) = nvidia {
        notes.push(Note {
            node_id: "local".into(),
            source: "telemetry".into(),
            level: "info".into(),
            message: format!("nvidia-smi reported {} GPU(s).", sample.gpus.len()),
        });
        let thermal = sample.temp_c.map(|temp| thermal_from_temp(temp));
        (Some(sample.gpus), thermal)
    } else {
        notes.push(Note {
            node_id: "local".into(),
            source: "telemetry".into(),
            level: "info".into(),
            message: "nvidia-smi is not available on this host. Local VRAM stays unknown until a probe succeeds.".into(),
        });
        (None, None)
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

    for note in probe.notes {
        state.log(&note.node_id, &note.source, &note.level, note.message);
    }
}

pub fn sample_nvidia() -> Option<NvidiaSample> {
    let raw = capture(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total,memory.used,utilization.gpu,temperature.gpu",
            "--format=csv,noheader,nounits",
        ],
        Duration::from_millis(1200),
    )?;
    let parsed = parse_nvidia_csv(&raw);
    if parsed.gpus.is_empty() {
        None
    } else {
        Some(parsed)
    }
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
        let used_mib: f64 = parts[2].parse().unwrap_or(0.0);
        let util: f64 = parts[3].parse().unwrap_or(0.0);
        if total_mib <= 0.0 {
            continue;
        }
        if parts.len() >= 5 {
            if let Ok(temp) = parts[4].parse::<f64>() {
                temp_c = Some(temp);
            }
        }
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
    Some(LmDevice {
        identifier,
        name,
        status: status.unwrap_or_else(|| "unknown".into()),
        loaded: loaded.unwrap_or_default().iter().filter_map(model_label).collect(),
    })
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

fn capture(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new(program)
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
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut out);
                }
                return Some(out);
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
        assert_eq!(sample.temp_c, Some(47.0));
    }
}
