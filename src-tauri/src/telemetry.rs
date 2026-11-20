use crate::discovery;
use crate::orchestrator::{self, announce_transition};
use crate::state::{now_iso, write_lock, AppState, Shared, SwarmNode};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub fn spawn(handle: AppHandle, shared: Shared) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(1200));
        let snap = {
            let mut state = write_lock(&shared);
            tick(&mut state);
            state.snapshot()
        };
        let _ = handle.emit("swarm-tick", snap);
    });
}

fn tick(state: &mut AppState) {
    state.tick = state.tick.wrapping_add(1);
    if state.stress_ticks > 0 {
        state.stress_ticks -= 1;
    }
    let stress = state.stress_ticks > 0;
    let inference = state.inference_active;
    let tick = state.tick;

    let ids: Vec<String> = state.nodes.iter().map(|node| node.id.clone()).collect();
    for id in ids {
        let hot = orchestrator::node_is_hot(state, &id);
        if let Some(node) = state.node_mut(&id) {
            advance_node(node, tick, stress, inference, hot);
        }
        announce_transition(state, &id);
    }

    if tick % 4 == 0 {
        if let Some(sample) = discovery::sample_nvidia() {
            if let Some(local) = state.node_mut("local") {
                if !sample.gpus.is_empty() {
                    local.gpus = sample.gpus;
                    if let Some(temp) = sample.temp_c {
                        local.thermal = if temp >= 90.0 {
                            "throttling"
                        } else if temp >= 75.0 {
                            "hot"
                        } else if temp >= 55.0 {
                            "warm"
                        } else {
                            "cool"
                        }
                        .into();
                    }
                }
            }
        }
    }

    let peers: Vec<(String, String)> = state
        .nodes
        .iter()
        .filter(|node| !node.simulated && node.id != "local" && node.status != "offline")
        .map(|node| (node.id.clone(), node.tailscale_ip.clone()))
        .collect();
    if !peers.is_empty() {
        let (id, ip) = &peers[(tick as usize) % peers.len()];
        if let Some(ms) = discovery::ping_ms(ip) {
            if let Some(node) = state.node_mut(id) {
                node.ping_ms = ms;
                node.ping_base = ms;
                node.ping_misses = 0;
                node.last_seen = now_iso();
            }
        }
    }

    let finished: Vec<_> = state
        .transfers
        .iter()
        .map(|transfer| {
            let mut next = transfer.clone();
            next.progress = (next.progress + 0.17).min(1.0);
            next
        })
        .collect();
    state.transfers = finished.iter().filter(|transfer| transfer.progress < 1.0).cloned().collect();
    for transfer in finished.into_iter().filter(|transfer| transfer.progress >= 1.0) {
        if let Some(model) = state.model_mut(&transfer.model_id) {
            model.host_node_id = transfer.to_node.clone();
        }
        let dest = state
            .nodes
            .iter()
            .find(|node| node.id == transfer.to_node)
            .map(|node| node.hostname.clone())
            .unwrap_or(transfer.to_node);
        state.log(
            "gateway",
            "orchestrator",
            "info",
            format!(
                "Catalog pointer for {} now sits on {dest}. No byte transfer was started; LM Link has no pull API on this peer.",
                transfer.model_name
            ),
        );
    }
}

pub fn apply_synthetic_stress(node: &mut SwarmNode, tick: u64) {
    let wave = ((tick as f64) * 0.37).sin().abs();
    node.tokens_per_sec = 28.0 + wave * 14.0;
    node.throughput_mbps = 10.0 + wave * 6.0;
    node.thermal = "hot".into();
    node.last_seen = now_iso();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_stress_runs_without_a_gpu_sample() {
        let mut node = crate::state::bare_node("local", "FamilyRoom");
        apply_synthetic_stress(&mut node, 3);
        assert!(node.tokens_per_sec > 20.0);
        assert_eq!(node.thermal, "hot");
        assert!(node.gpus.is_empty());
        assert_eq!(node.status, "online");
    }
}

fn advance_node(node: &mut SwarmNode, tick: u64, stress: bool, inference: bool, hot: bool) {
    if matches!(node.status.as_str(), "offline" | "sleeping") {
        node.tokens_per_sec = 0.0;
        node.throughput_mbps = 0.1;
        if let Some(gpu) = node.gpus.first_mut() {
            gpu.utilization = (gpu.utilization * 0.8).max(0.0);
        }
        return;
    }

    if !node.simulated && stress && node.gpus.iter().all(|gpu| gpu.vram_total_gb <= 0.0) {
        apply_synthetic_stress(node, tick);
        return;
    }
    if !node.simulated && !stress && node.gpus.is_empty() && node.thermal == "hot" {
        node.thermal = "cool".into();
        node.tokens_per_sec = 0.0;
    }

    if node.simulated {
        let salt = node.id.bytes().map(|byte| byte as u64).sum::<u64>();
        let wave = ((tick as f64) * 0.37 + salt as f64 * 0.15).sin();
        let boost = if stress { 30.0 } else { 0.0 };
        let util = (node.util_base + wave * 7.0 + boost).clamp(3.0, 99.0);
        if let Some(gpu) = node.gpus.first_mut() {
            gpu.utilization = util;
        }
        node.ping_ms = (node.ping_base as i32 + (wave * 2.0).round() as i32).max(1) as u32;
        if stress && util > 93.0 {
            node.thermal = "throttling".into();
            node.status = "throttled".into();
        } else if node.status == "throttled" && !stress && util < 80.0 {
            node.status = "online".into();
            node.thermal = "warm".into();
        } else if node.status != "throttled" {
            node.thermal = if util > 85.0 {
                "hot"
            } else if util > 55.0 {
                "warm"
            } else {
                "cool"
            }
            .into();
        }
    }

    let util = node.gpus.first().map(|gpu| gpu.utilization).unwrap_or(0.0);
    if hot && matches!(node.status.as_str(), "online" | "degraded") {
        let inference_boost = if inference { 8.0 } else { 0.0 };
        node.tokens_per_sec = (node.token_base * (0.9 + (util / 400.0)) + inference_boost).max(0.0);
    } else {
        node.tokens_per_sec = 0.0;
    }

    let base = match node.relay.as_str() {
        "derp" => 16.0,
        "local" => 0.4,
        _ => 84.0,
    };
    let scale = if inference { 1.0 } else { 0.06 };
    node.throughput_mbps = f64::max(base * scale, 0.15);
    if matches!(node.status.as_str(), "online" | "degraded") {
        node.last_seen = now_iso();
    }
}
