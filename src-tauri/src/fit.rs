use crate::state::{node_vram, InferenceParams, ModelArtifact, SwarmNode};
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShardSlice {
    pub node_id: String,
    pub hostname: String,
    pub layers: u32,
    pub vram_gb: f64,
    pub kind: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FitResult {
    pub model_id: String,
    pub weights_gb: f64,
    pub kv_gb: f64,
    pub scratch_gb: f64,
    pub total_gb: f64,
    pub pooled_gb: f64,
    pub fits: bool,
    pub warnings: Vec<String>,
    pub slices: Vec<ShardSlice>,
}

pub fn kv_bytes(quant: &str) -> f64 {
    match quant {
        "fp16" => 2.0,
        "q4" => 0.5,
        _ => 1.0,
    }
}

pub fn calculate(model: &ModelArtifact, nodes: &[SwarmNode], params: &InferenceParams) -> FitResult {
    let bytes = kv_bytes(&params.kv_quant);
    let kv_gb = 2.0
        * model.layers as f64
        * model.kv_heads as f64
        * model.head_dim as f64
        * params.context_length as f64
        * bytes
        / 1e9;
    let scratch_full = (model.layers as f64 / 40.0) * 0.45;
    let scratch_gb = if params.flash_attention {
        scratch_full * 0.55
    } else {
        scratch_full
    };
    let offload_layers = params.gpu_offload_layers.min(model.layers);
    let offload_ratio = if model.layers == 0 {
        1.0
    } else {
        offload_layers as f64 / model.layers as f64
    };
    let gpu_weights = model.size_gb * offload_ratio;
    let cpu_weights = model.size_gb - gpu_weights;
    let gpu_need = gpu_weights + kv_gb + scratch_gb;

    let capacity_of = |node: &SwarmNode| {
        let (total, allocated) = node_vram(node);
        if params.evict_for_fit {
            total
        } else {
            (total - allocated).max(0.0)
        }
    };

    let mut pool: Vec<&SwarmNode> = nodes
        .iter()
        .filter(|node| matches!(node.status.as_str(), "online" | "degraded") && capacity_of(node) > 0.2)
        .collect();
    pool.sort_by(|a, b| {
        capacity_of(b)
            .total_cmp(&capacity_of(a))
            .then_with(|| a.hostname.cmp(&b.hostname))
    });

    let pooled_gb: f64 = pool.iter().map(|node| capacity_of(node)).sum();
    let mut warnings = Vec::new();
    let mut slices = Vec::new();

    if cpu_weights > 0.2 {
        warnings.push(format!(
            "{cpu_weights:.1} GB of weights stay on CPU. GPU offload is {offload_layers}/{} layers.",
            model.layers
        ));
        slices.push(ShardSlice {
            node_id: "cpu".into(),
            hostname: "CPU".into(),
            layers: model.layers.saturating_sub(offload_layers),
            vram_gb: cpu_weights,
            kind: "cpu".into(),
        });
    }

    if nodes.iter().any(|node| {
        matches!(node.status.as_str(), "throttled" | "offline" | "sleeping") && !node.gpus.is_empty()
    }) {
        warnings.push("Unhealthy nodes were left out of the slice.".into());
    }

    let mut remaining_gb = gpu_need;
    let mut placed: Vec<(String, String, f64, bool, u32)> = Vec::new();

    for node in &pool {
        if remaining_gb <= 0.05 {
            break;
        }
        let cap = capacity_of(node);
        let take = remaining_gb.min(cap);
        if take <= 0.05 {
            continue;
        }
        placed.push((
            node.id.clone(),
            node.hostname.clone(),
            take,
            node.relay == "derp" || node.ping_ms > 40,
            node.ping_ms,
        ));
        remaining_gb -= take;
    }

    let placed_gb: f64 = placed.iter().map(|item| item.2).sum();
    let gb_fits = remaining_gb <= 0.2;
    let mut layers_left = offload_layers;
    for (index, (id, hostname, take, slow, ping)) in placed.iter().enumerate() {
        let last = index + 1 == placed.len();
        let layers = if placed_gb <= 0.01 {
            0
        } else if gb_fits && last {
            layers_left
        } else {
            ((*take / gpu_need) * offload_layers as f64).round() as u32
        };
        let layers = layers.min(layers_left);
        layers_left = layers_left.saturating_sub(layers);
        slices.push(ShardSlice {
            node_id: id.clone(),
            hostname: hostname.clone(),
            layers,
            vram_gb: *take,
            kind: "resident".into(),
        });
        if *slow {
            warnings.push(format!(
                "{hostname} is a slow hop ({ping} ms, relay or DERP). Layers placed there add a pipeline bubble."
            ));
        }
    }
    let remaining_layers = layers_left;

    let fits = remaining_gb <= 0.2;
    if !fits {
        warnings.push(format!(
            "{remaining_gb:.1} GB does not fit in the {pooled_gb:.1} GB pool."
        ));
        slices.push(ShardSlice {
            node_id: "unplaced".into(),
            hostname: "Unplaced".into(),
            layers: remaining_layers,
            vram_gb: remaining_gb,
            kind: "unplaced".into(),
        });
    }

    if !params.evict_for_fit && gpu_need > pooled_gb + 0.2 {
        warnings.push("Turn on planning against total VRAM if current residents can be evicted.".into());
    }

    FitResult {
        model_id: model.id.clone(),
        weights_gb: model.size_gb,
        kv_gb,
        scratch_gb,
        total_gb: model.size_gb + kv_gb + scratch_gb,
        pooled_gb,
        fits,
        warnings,
        slices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    #[test]
    fn seventy_b_fits_when_residents_can_move() {
        let state = AppState::demo();
        let model = state.models.iter().find(|item| item.id == "llama-70b").unwrap();
        let fit = calculate(model, &state.nodes, &state.params);
        assert!(fit.fits, "{:?}", fit.warnings);
        assert!(fit.slices.iter().filter(|slice| slice.kind == "resident").count() >= 2);
        let placed: f64 = fit
            .slices
            .iter()
            .filter(|slice| slice.kind == "resident")
            .map(|slice| slice.vram_gb)
            .sum();
        assert!((placed - (fit.weights_gb + fit.kv_gb + fit.scratch_gb)).abs() < 0.3);
    }

    #[test]
    fn four_oh_five_does_not_fit_the_demo_pool() {
        let state = AppState::demo();
        let model = state.models.iter().find(|item| item.id == "llama-405b").unwrap();
        let fit = calculate(model, &state.nodes, &state.params);
        assert!(!fit.fits);
        assert!(fit.slices.iter().any(|slice| slice.kind == "unplaced"));
    }

    #[test]
    fn kv_grows_with_context() {
        let state = AppState::demo();
        let model = state.models.iter().find(|item| item.id == "llama-70b").unwrap();
        let mut wide = state.params.clone();
        wide.context_length = 32768;
        wide.kv_quant = "fp16".into();
        let narrow = calculate(model, &state.nodes, &state.params);
        let broad = calculate(model, &state.nodes, &wide);
        assert!(broad.kv_gb > narrow.kv_gb * 4.0);
    }
}
