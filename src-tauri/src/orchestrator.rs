use crate::fit;
use crate::state::{hostname_for, node_vram, refresh_vram_fields, vram_rank, AppState, SwarmNode};
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteDecision {
    pub mode: String,
    pub primary_node_id: String,
    pub primary_hostname: String,
    pub primary_model: String,
    pub draft_node_id: Option<String>,
    pub draft_hostname: Option<String>,
    pub draft_model: Option<String>,
    pub shard_nodes: Vec<String>,
    pub reason: String,
    pub failover_from: Option<String>,
    pub proxy_url: Option<String>,
    pub upstream: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub id: String,
    pub role: String,
    pub node_id: String,
    pub hostname: String,
    pub model: String,
    pub live: bool,
    pub output: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPlan {
    pub id: String,
    pub objective: String,
    pub steps: Vec<PlanStep>,
    pub artifact: String,
    pub warnings: Vec<String>,
    pub combined_preview: String,
}

fn healthy(node: &SwarmNode) -> bool {
    matches!(node.status.as_str(), "online" | "degraded")
}

fn total_vram(node: &SwarmNode) -> f64 {
    vram_rank(node).0
}

fn capacity_order(a: &SwarmNode, b: &SwarmNode) -> std::cmp::Ordering {
    let (left_gb, left_tier) = vram_rank(a);
    let (right_gb, right_tier) = vram_rank(b);
    right_gb
        .total_cmp(&left_gb)
        .then(right_tier.cmp(&left_tier))
        .then(a.hostname.cmp(&b.hostname))
}

pub fn route(state: &AppState, requested_model: Option<&str>) -> RouteDecision {
    let mut eligible: Vec<&SwarmNode> = state.nodes.iter().filter(|node| healthy(node) && total_vram(node) > 0.0).collect();
    eligible.sort_by(|a, b| capacity_order(a, b));

    let model_name = |id: &str| {
        state
            .models
            .iter()
            .find(|model| model.host_node_id == id && model.loaded)
            .map(|model| model.name.clone())
            .unwrap_or_else(|| requested_model.unwrap_or("swarm").to_string())
    };

    if eligible.is_empty() {
        return RouteDecision {
            mode: state.mode.clone(),
            primary_node_id: "none".into(),
            primary_hostname: "none".into(),
            primary_model: requested_model.unwrap_or("swarm").into(),
            draft_node_id: None,
            draft_hostname: None,
            draft_model: None,
            shard_nodes: Vec::new(),
            reason: "No online node has measured, configured, or model-inferred VRAM. The gateway will answer with an orchestrator brief.".into(),
            failover_from: down_hostname(state),
            proxy_url: None,
            upstream: "brief".into(),
        };
    }

    let mode = state.mode.as_str();
    let (primary, failover_from) = pick_primary(state, &eligible);
    let draft = if mode == "speculative" || state.params.speculative {
        eligible.iter().rev().find(|node| node.id != primary.id).copied()
    } else {
        None
    };

    let shard_nodes = if mode == "shard" {
        eligible.iter().map(|node| node.id.clone()).collect()
    } else {
        Vec::new()
    };

    let direct = proxy_for(state, primary);
    let backend = shard_backend_url(state, mode);
    let proxy_url = direct.or(backend);
    let upstream = if proxy_url.is_some() { "live" } else { "brief" };

    let mut reason = match mode {
        "shard" => format!(
            "Layer shard across {} healthy nodes. Tensor RPC is used only when a shard backend URL is set.",
            eligible.len()
        ),
        "speculative" => "Draft node proposes tokens. The target node is the OpenAI response.".into(),
        _ => "Pipeline routes the request to the architect role, then the workbench fans the brief out to the other roles.".into(),
    };
    if let Some(from) = &failover_from {
        reason = format!("Failover from {from}. {reason}");
    }
    if mode == "shard" && proxy_url.is_none() {
        reason.push_str(" No llama.cpp RPC backend is configured, so this response is the shard plan rather than split weights.");
    }

    RouteDecision {
        mode: state.mode.clone(),
        primary_node_id: primary.id.clone(),
        primary_hostname: primary.hostname.clone(),
        primary_model: model_name(&primary.id),
        draft_node_id: draft.map(|node| node.id.clone()),
        draft_hostname: draft.map(|node| node.hostname.clone()),
        draft_model: draft.map(|node| model_name(&node.id)),
        shard_nodes,
        reason,
        failover_from,
        proxy_url,
        upstream: upstream.into(),
    }
}

fn shard_backend_url(state: &AppState, mode: &str) -> Option<String> {
    let backend = state.shard_backend.trim();
    if mode == "shard" && (backend.starts_with("http://") || backend.starts_with("https://")) {
        Some(format!("{}/v1/chat/completions", backend.trim_end_matches('/')))
    } else {
        None
    }
}

fn proxy_for(state: &AppState, node: &SwarmNode) -> Option<String> {
    if node.simulated || !node.upstream_alive {
        return None;
    }
    let port = node.lm_link_port.unwrap_or(1234);
    let ip = if node.id == "local" { "127.0.0.1" } else { node.tailscale_ip.as_str() };
    let _ = state;
    Some(format!("http://{ip}:{port}/v1/chat/completions"))
}

fn down_hostname(state: &AppState) -> Option<String> {
    state
        .nodes
        .iter()
        .find(|node| matches!(node.status.as_str(), "offline" | "sleeping" | "throttled") && node.role.as_deref() == Some("architect"))
        .map(|node| node.hostname.clone())
}

fn pick_primary<'a>(state: &AppState, eligible: &'a [&'a SwarmNode]) -> (&'a SwarmNode, Option<String>) {
    if state.mode == "shard" {
        return (eligible[0], None);
    }
    if state.mode == "speculative" || state.params.speculative {
        return (eligible[0], None);
    }
    if let Some(architect) = eligible.iter().copied().find(|node| node.role.as_deref() == Some("architect")) {
        return (architect, None);
    }
    let missing = state
        .nodes
        .iter()
        .find(|node| node.role.as_deref() == Some("architect") && !healthy(node))
        .map(|node| node.hostname.clone());
    (eligible[0], missing)
}

pub fn rebalance(state: &mut AppState) -> String {
    let blueprint = state
        .blueprints
        .iter()
        .find(|item| item.id == state.active_blueprint_id)
        .cloned();
    let Some(blueprint) = blueprint else {
        return "No blueprint is active.".into();
    };

    for node in &mut state.nodes {
        refresh_vram_fields(node);
    }
    let mut ranked: Vec<usize> = (0..state.nodes.len())
        .filter(|index| healthy(&state.nodes[*index]) && total_vram(&state.nodes[*index]) > 0.0)
        .collect();
    ranked.sort_by(|a, b| capacity_order(&state.nodes[*a], &state.nodes[*b]));

    for node in &mut state.nodes {
        if healthy(node) {
            node.role = Some("standby".into());
        }
    }

    let mut notes = Vec::new();
    if blueprint.mode == "speculative" && ranked.len() >= 2 {
        let target = ranked[0];
        let draft = ranked[ranked.len() - 1];
        state.nodes[target].role = Some("coder".into());
        state.nodes[draft].role = Some("draft".into());
        state.params.draft_node_id = Some(state.nodes[draft].id.clone());
        state.params.speculative = true;
        notes.push(format!(
            "{} drafts for {}",
            state.nodes[draft].hostname, state.nodes[target].hostname
        ));
    } else {
        for (offset, role) in blueprint.roles.iter().enumerate() {
            if let Some(index) = ranked.get(offset) {
                state.nodes[*index].role = Some(role.role.clone());
                let (gb, _) = vram_rank(&state.nodes[*index]);
                notes.push(format!(
                    "{} → {} ({:.1} GB {})",
                    state.nodes[*index].hostname,
                    role.role,
                    gb,
                    state.nodes[*index].vram_source
                ));
            } else {
                notes.push(format!("no healthy node for {}", role.role));
            }
        }
    }

    let summary = format!("Rebalanced {} ({})", blueprint.name, notes.join(", "));
    state.log("gateway", "orchestrator", "info", summary.clone());
    summary
}

pub fn commit_shard(state: &mut AppState, model_id: &str) -> Result<String, String> {
    let model = state
        .models
        .iter()
        .find(|item| item.id == model_id)
        .cloned()
        .ok_or_else(|| format!("Unknown model {model_id}"))?;
    let fit = fit::calculate(&model, &state.nodes, &state.params);
    if !fit.fits {
        return Err(fit.warnings.join(" "));
    }

    if state.params.evict_for_fit {
        for item in &mut state.models {
            item.loaded = false;
            item.sharded = false;
            item.shard_nodes.clear();
        }
        for node in &mut state.nodes {
            for gpu in &mut node.gpus {
                gpu.vram_allocated_gb = 0.0;
            }
            node.token_base = 0.0;
            node.tokens_per_sec = 0.0;
        }
    }

    let mut shard_nodes = Vec::new();
    let mut layer_notes = Vec::new();
    for slice in fit.slices.iter().filter(|slice| slice.kind == "resident") {
        if let Some(node) = state.node_mut(&slice.node_id) {
            if let Some(gpu) = node.gpus.first_mut() {
                gpu.vram_allocated_gb = (gpu.vram_allocated_gb + slice.vram_gb).min(gpu.vram_total_gb);
            }
            node.token_base = 18.0;
            node.tokens_per_sec = 18.0;
            node.role = Some("shard".into());
            shard_nodes.push(node.id.clone());
            layer_notes.push(format!("{} {}L/{:.1}GB", node.hostname, slice.layers, slice.vram_gb));
        }
    }

    if let Some(model) = state.model_mut(model_id) {
        model.loaded = true;
        model.sharded = true;
        model.shard_nodes = shard_nodes;
    }
    state.mode = "shard".into();
    let summary = format!("Sharded {} across {}", model.name, layer_notes.join(", "));
    state.log("gateway", "orchestrator", "info", summary.clone());
    if state.shard_backend.is_empty() {
        state.log(
            "gateway",
            "orchestrator",
            "warn",
            "Shard map is committed in the orchestrator. Set a llama.cpp RPC / EXO backend URL before weights actually split.",
        );
    }
    Ok(summary)
}

pub fn mount_model(state: &mut AppState, model_id: &str, node_id: &str, invoke_lms: bool) -> Result<String, String> {
    let size = state
        .models
        .iter()
        .find(|model| model.id == model_id)
        .map(|model| model.size_gb)
        .ok_or_else(|| format!("Unknown model {model_id}"))?;
    let host_name = hostname_for(state, node_id);
    let simulated = state.nodes.iter().find(|node| node.id == node_id).map(|node| node.simulated).unwrap_or(true);
    let (total, allocated) = state
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .map(node_vram)
        .ok_or_else(|| format!("Unknown node {node_id}"))?;
    if allocated + size > total + 0.25 {
        return Err(format!(
            "{host_name} has {:.1} GB free and {size:.1} GB is required. Shard the model or pick another node.",
            (total - allocated).max(0.0)
        ));
    }

    if let Some(node) = state.node_mut(node_id) {
        if let Some(gpu) = node.gpus.first_mut() {
            gpu.vram_allocated_gb = (gpu.vram_allocated_gb + size).min(gpu.vram_total_gb);
        }
        node.token_base = node.token_base.max(20.0);
    }
    let path = {
        let model = state.model_mut(model_id).ok_or_else(|| format!("Unknown model {model_id}"))?;
        model.loaded = true;
        model.sharded = false;
        model.shard_nodes.clear();
        model.host_node_id = node_id.to_string();
        model.path.clone()
    };
    let name = state.models.iter().find(|model| model.id == model_id).map(|model| model.name.clone()).unwrap_or_default();
    let mut summary = format!("Reserved {name} on {host_name}");
    if invoke_lms {
        if simulated {
            summary.push_str(". LM Link on a simulated peer cannot run lms load.");
        } else if node_id == "local" {
            if let Some(path) = path {
                match std::process::Command::new("lms").args(["load", &path]).spawn() {
                    Ok(child) => summary.push_str(&format!(". lms load started (pid {}).", child.id())),
                    Err(err) => summary.push_str(&format!(". lms load was not started: {err}.")),
                }
            } else {
                summary.push_str(". No on-disk path is known, so lms load was not called.");
            }
        } else {
            summary.push_str(". Remote lms load is not invoked from this machine.");
        }
    } else {
        summary.push_str(". lms load was not called.");
    }
    state.log(node_id, "lms", "info", summary.clone());
    Ok(summary)
}

pub fn plan(state: &AppState, objective: &str) -> TaskPlan {
    let blueprint = state
        .blueprints
        .iter()
        .find(|item| item.id == state.active_blueprint_id)
        .cloned()
        .unwrap_or_else(|| state.blueprints[0].clone());
    let mut warnings = Vec::new();
    let mut steps = Vec::new();

    if state.mode == "shard" {
        let model = state.models.iter().find(|item| item.sharded).or_else(|| {
            state.models.iter().find(|item| item.id == "llama-70b")
        });
        let fit = model.map(|item| fit::calculate(item, &state.nodes, &state.params));
        let map = fit
            .as_ref()
            .map(|item| {
                item.slices
                    .iter()
                    .filter(|slice| slice.kind == "resident")
                    .map(|slice| format!("{}: {} layers, {:.1} GB", slice.hostname, slice.layers, slice.vram_gb))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|| "No shard map.".into());
        if state.shard_backend.is_empty() {
            warnings.push("No RPC backend is configured. This is the placement plan, not a live tensor split.".into());
        }
        let name = model.map(|item| item.name.as_str()).unwrap_or("model");
        steps.push(step(state, "shard", &format!("Shard map for {name}\n{map}\n\nObjective\n{objective}")));
    } else if blueprint.mode == "speculative" || state.mode == "speculative" {
        steps.push(step(
            state,
            "draft",
            &format!("Draft pass\nPropose a short patch for:\n{objective}\n\nKeep the draft under a few hundred tokens so the target can reject early."),
        ));
        steps.push(step(
            state,
            "coder",
            &format!("Target pass\nAccept the draft only where it matches the repository contracts.\nObjective: {objective}"),
        ));
    } else {
        for role in &blueprint.roles {
            let body = match role.role.as_str() {
                "architect" => format!(
                    "Plan\n1. Name the files and contracts the objective touches.\n2. Keep the patch small enough for one worktree.\n3. Hand the coder a concrete edit list.\n\nObjective\n{objective}"
                ),
                "coder" => format!(
                    "Patch brief\nApply the architect's edit list for:\n{objective}\n\nWrite the change as a reviewable diff. Do not restyle unrelated code."
                ),
                "critic" => format!(
                    "Review\nCheck the diff against: {objective}\nLook for missing edge states, unsafe shell use, and tests that would have caught a regression."
                ),
                "embedder" => format!(
                    "Retrieval notes\nIndex the local files that mention the terms in:\n{objective}\nReturn the top passages before the architect answers."
                ),
                "draft" => format!("Draft a compact answer for:\n{objective}"),
                _ => format!("{} owns this step.\n{objective}", role.role),
            };
            steps.push(step(state, &role.role, &body));
        }
    }

    if steps.is_empty() {
        warnings.push("The blueprint has no roles that map onto a healthy node.".into());
    }
    for step in &steps {
        if step.node_id == "unassigned" {
            warnings.push(format!("No healthy node is assigned to {}.", step.role));
        }
    }
    if state.nodes.iter().any(|node| node.relay == "derp" && healthy(node) && node.role.as_deref() != Some("standby")) {
        warnings.push("A role is pinned to a DERP peer. Expect extra time to first token.".into());
    }

    let artifact = artifact_for(&blueprint.id, objective);
    let combined_preview = steps
        .iter()
        .map(|step| format!("## {}\n{}", step.role, step.output))
        .collect::<Vec<_>>()
        .join("\n\n");

    TaskPlan {
        id: format!("plan-{}", crate::state::now_hms().replace([':', '.'], "")),
        objective: objective.into(),
        steps,
        artifact,
        warnings,
        combined_preview,
    }
}

fn step(state: &AppState, role: &str, output: &str) -> PlanStep {
    let node = state
        .nodes
        .iter()
        .find(|node| node.role.as_deref() == Some(role) && healthy(node))
        .or_else(|| state.nodes.iter().find(|node| healthy(node) && total_vram(node) > 0.0));
    let model = node
        .and_then(|item| {
            state
                .models
                .iter()
                .find(|model| model.loaded && (model.host_node_id == item.id || model.shard_nodes.contains(&item.id)))
        })
        .map(|model| model.name.clone())
        .unwrap_or_else(|| "unmounted".into());
    PlanStep {
        id: format!("step-{role}"),
        role: role.into(),
        node_id: node.map(|item| item.id.clone()).unwrap_or_else(|| "unassigned".into()),
        hostname: node.map(|item| item.hostname.clone()).unwrap_or_else(|| "unassigned".into()),
        model,
        live: node.map(|item| item.upstream_alive && !item.simulated).unwrap_or(false),
        output: output.into(),
    }
}

fn artifact_for(blueprint_id: &str, objective: &str) -> String {
    let slug = objective
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == ' ')
        .collect::<String>();
    let title = slug.split_whitespace().take(8).collect::<Vec<_>>().join(" ");
    match blueprint_id {
        "research" => format!("# Synthesis\n\nObjective: {objective}\n\n## Findings\n- The claim needs a source note before it is treated as a result.\n- Disagreement between nodes should stay visible in the final note.\n\n## Open questions\n- Which file or paper actually decides this?\n"),
        "rag" => format!("# retrieval\nquery: {title}\ntop_k: 6\nembed_node: harbor-mbp\nanswer_node: atlas-4090\nverify_node: harbor-mbp\n"),
        "shard-70b" => format!("# shard commit\nmodel: llama-3.1-70b-instruct\nobjective: {objective}\nbackend: unset\n"),
        _ => format!(
            "diff --git a/src/swarm_change.ts b/src/swarm_change.ts\n--- a/src/swarm_change.ts\n+++ b/src/swarm_change.ts\n@@\n+// Murmuration worktree: {title}\n+export function applyObjective() {{\n+  return {objective:?};\n+}}\n"
        ),
    }
}

pub fn combined_reply(plan: &TaskPlan, decision: &RouteDecision) -> String {
    format!(
        "{}\n\n---\nRouted to {} ({}) via {} mode.",
        plan.combined_preview, decision.primary_hostname, decision.primary_model, decision.mode
    )
}

pub fn node_is_hot(state: &AppState, node_id: &str) -> bool {
    state.models.iter().any(|model| {
        model.loaded && (model.host_node_id == node_id || model.shard_nodes.iter().any(|id| id == node_id))
    })
}

pub fn announce_transition(state: &mut AppState, node_id: &str) {
    let Some(node) = state.nodes.iter().find(|node| node.id == node_id).cloned() else {
        return;
    };
    let down = matches!(node.status.as_str(), "offline" | "sleeping" | "throttled");
    if down && !node.announced_down {
        if let Some(item) = state.node_mut(node_id) {
            item.announced_down = true;
        }
        let message = format!(
            "{} is {}. New prompts will skip it.",
            node.hostname, node.status
        );
        state.failover_note = Some(message.clone());
        state.log(&node.id, "orchestrator", "warn", message);
    } else if !down && node.announced_down {
        if let Some(item) = state.node_mut(node_id) {
            item.announced_down = false;
        }
        let message = format!("{} is back on the mesh.", node.hostname);
        state.failover_note = None;
        state.log(&node.id, "orchestrator", "info", message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failover_skips_a_down_architect() {
        let mut state = AppState::demo();
        state.node_mut("atlas").unwrap().status = "offline".into();
        let decision = route(&state, None);
        assert_ne!(decision.primary_node_id, "atlas");
        assert!(decision.failover_from.unwrap().contains("atlas"));
    }

    #[test]
    fn commit_rejects_a_model_the_pool_cannot_hold() {
        let mut state = AppState::demo();
        let err = commit_shard(&mut state, "llama-405b").unwrap_err();
        assert!(err.contains("does not fit") || err.contains("GB"));
        assert!(state.models.iter().any(|model| model.id == "qwen-32b" && model.loaded));
    }

    #[test]
    fn rebalance_ranks_online_nodes_with_inferred_or_configured_vram() {
        let mut state = AppState::boot();
        state.active_blueprint_id = "refactor".into();
        let mut family = crate::state::bare_node("local", "FamilyRoom");
        family.inferred_vram_gb = 10.4;
        let mut gaming = crate::state::bare_node("lm-game", "GamingPC");
        gaming.configured_vram_gb = 16.0;
        let mut timmy = crate::state::bare_node("lm-tim", "Timmys-PC");
        timmy.gpus.push(crate::state::GpuMetric {
            name: "RTX".into(),
            vram_total_gb: 8.0,
            vram_allocated_gb: 1.0,
            utilization: 4.0,
        });
        state.nodes = vec![family, gaming, timmy];
        let summary = rebalance(&mut state);
        let role = |name: &str| state.nodes.iter().find(|node| node.hostname == name).unwrap().role.clone();
        assert_eq!(role("GamingPC").as_deref(), Some("architect"));
        assert_eq!(role("FamilyRoom").as_deref(), Some("coder"));
        assert_eq!(role("Timmys-PC").as_deref(), Some("critic"));
        assert!(!summary.contains("no healthy node"));
        assert!(state.nodes.iter().all(|node| node.role.as_deref() != Some("standby")));
    }

    #[test]
    fn measured_vram_outranks_the_same_configured_size() {
        let mut state = AppState::boot();
        state.active_blueprint_id = "refactor".into();
        let mut measured = crate::state::bare_node("lm-measured", "Measured");
        measured.gpus.push(crate::state::GpuMetric {
            name: "RTX".into(),
            vram_total_gb: 16.0,
            vram_allocated_gb: 0.0,
            utilization: 1.0,
        });
        let mut configured = crate::state::bare_node("lm-config", "Configured");
        configured.configured_vram_gb = 16.0;
        state.nodes = vec![configured, measured];
        rebalance(&mut state);
        let architect = state.nodes.iter().find(|node| node.role.as_deref() == Some("architect")).unwrap();
        assert_eq!(architect.hostname, "Measured");
        assert_eq!(architect.vram_source, "measured");
    }
}
