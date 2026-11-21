use crate::discovery;
use crate::fit::{self, FitResult};
use crate::mcp;
use crate::orchestrator::{self, TaskPlan};
use crate::state::{assign_hardware, hardware_file_path, upsert_hardware, write_lock, HardwareEntry, InferenceParams, Shared, Snapshot, Transfer};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePayload {
    pub path: String,
    pub contents: String,
}

#[tauri::command]
pub fn get_snapshot(state: tauri::State<Shared>) -> Snapshot {
    crate::state::read_lock(state.inner()).snapshot()
}

#[tauri::command]
pub async fn refresh_discovery(state: tauri::State<'_, Shared>) -> Result<Snapshot, String> {
    let shared = state.inner().clone();
    let probe = tokio::task::spawn_blocking(discovery::probe)
        .await
        .map_err(|err| err.to_string())?;
    let mut st = write_lock(&shared);
    discovery::apply(&mut st, probe);
    Ok(st.snapshot())
}

#[tauri::command]
pub fn set_mode(state: tauri::State<Shared>, mode: String) -> Result<Snapshot, String> {
    if !matches!(mode.as_str(), "pipeline" | "shard" | "speculative") {
        return Err(format!("Unknown mode {mode}"));
    }
    let mut st = write_lock(state.inner());
    st.mode = mode.clone();
    st.params.speculative = mode == "speculative";
    st.log("gateway", "orchestrator", "info", format!("Distribution mode is now {mode}."));
    Ok(st.snapshot())
}

#[tauri::command]
pub fn activate_blueprint(state: tauri::State<Shared>, blueprint_id: String) -> Result<Snapshot, String> {
    let mut st = write_lock(state.inner());
    let blueprint = st
        .blueprints
        .iter()
        .find(|item| item.id == blueprint_id)
        .cloned()
        .ok_or_else(|| format!("Unknown blueprint {blueprint_id}"))?;
    st.active_blueprint_id = blueprint.id.clone();
    st.mode = blueprint.mode.clone();
    st.params.speculative = blueprint.mode == "speculative";
    orchestrator::rebalance(&mut st);
    Ok(st.snapshot())
}

#[tauri::command]
pub fn update_params(state: tauri::State<Shared>, params: InferenceParams) -> Snapshot {
    let mut st = write_lock(state.inner());
    let speculative = params.speculative;
    st.params = params;
    if speculative {
        st.mode = "speculative".into();
    } else if st.mode == "speculative" {
        st.mode = "pipeline".into();
    }
    st.snapshot()
}

#[tauri::command]
pub fn mount_model(
    state: tauri::State<Shared>,
    model_id: String,
    node_id: String,
    invoke_lms: bool,
) -> Result<Snapshot, String> {
    let mut st = write_lock(state.inner());
    orchestrator::mount_model(&mut st, &model_id, &node_id, invoke_lms)?;
    Ok(st.snapshot())
}

#[tauri::command]
pub fn pull_model(state: tauri::State<Shared>, model_id: String, node_id: String) -> Result<Snapshot, String> {
    let mut st = write_lock(state.inner());
    let model = st
        .models
        .iter()
        .find(|model| model.id == model_id)
        .cloned()
        .ok_or_else(|| format!("Unknown model {model_id}"))?;
    if !st.nodes.iter().any(|node| node.id == node_id) {
        return Err(format!("Unknown node {node_id}"));
    }
    if st.transfers.iter().any(|transfer| transfer.model_id == model.id && transfer.progress < 1.0) {
        return Ok(st.snapshot());
    }
    let transfer_id = format!("xfer-{}", st.tick);
    st.transfers.push(Transfer {
        id: transfer_id,
        model_id: model.id.clone(),
        model_name: model.name.clone(),
        from_node: model.host_node_id,
        to_node: node_id,
        progress: 0.08,
    });
    st.log(
        "gateway",
        "orchestrator",
        "info",
        format!("Catalog move started for {}. This does not copy weight bytes.", model.name),
    );
    Ok(st.snapshot())
}

#[tauri::command]
pub fn rebalance(state: tauri::State<Shared>) -> Snapshot {
    let mut st = write_lock(state.inner());
    orchestrator::rebalance(&mut st);
    st.snapshot()
}

#[tauri::command]
pub fn calculate_fit(
    state: tauri::State<Shared>,
    model_id: String,
    params: Option<InferenceParams>,
) -> Result<FitResult, String> {
    let st = crate::state::read_lock(state.inner());
    let model = st
        .models
        .iter()
        .find(|model| model.id == model_id)
        .ok_or_else(|| format!("Unknown model {model_id}"))?;
    let params = params.unwrap_or_else(|| st.params.clone());
    Ok(fit::calculate(model, &st.nodes, &params))
}

#[tauri::command]
pub fn commit_shard(state: tauri::State<Shared>, model_id: String) -> Result<Snapshot, String> {
    let mut st = write_lock(state.inner());
    orchestrator::commit_shard(&mut st, &model_id)?;
    Ok(st.snapshot())
}

#[tauri::command]
pub fn set_demo_mesh(state: tauri::State<Shared>, _enabled: bool) -> Snapshot {
    let mut st = write_lock(state.inner());
    st.demo_mesh = false;
    st.nodes.retain(|node| !node.simulated);
    st.models.retain(|model| !model.simulated);
    st.log(
        "gateway",
        "lms",
        "info",
        "Devices outside this LM Link stay off the list. Nothing was removed from disk.",
    );
    st.snapshot()
}

#[tauri::command]
pub fn inject_load(state: tauri::State<Shared>) -> Snapshot {
    let mut st = write_lock(state.inner());
    st.stress_ticks = 16;
    let measured = st.nodes.iter().any(|node| crate::state::node_vram(node).0 > 0.0);
    if measured {
        st.log("gateway", "telemetry", "warn", "Stress load injected. Thermal failover can trip if a node crosses 93%.");
    } else {
        st.log(
            "gateway",
            "telemetry",
            "warn",
            "No GPU utilization sample is available. Stress is a synthetic token load on the LM Link devices.",
        );
    }
    st.snapshot()
}

#[tauri::command]
pub fn set_node_hardware(
    state: tauri::State<Shared>,
    node_id: String,
    vram_gb: f64,
    ram_gb: f64,
    cpu: String,
    gpu: String,
) -> Result<Snapshot, String> {
    if !vram_gb.is_finite() || !ram_gb.is_finite() || vram_gb < 0.0 || ram_gb < 0.0 {
        return Err("VRAM and RAM must be zero or a positive number of GB.".into());
    }
    let mut st = write_lock(state.inner());
    let hostname = st
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .map(|node| node.hostname.clone())
        .ok_or_else(|| format!("Unknown node {node_id}"))?;
    let entry = HardwareEntry {
        hostname: hostname.clone(),
        vram_gb,
        ram_gb,
        cpu,
        gpu,
    };
    if let Some(node) = st.node_mut(&node_id) {
        assign_hardware(node, &entry);
    }
    upsert_hardware(&hardware_file_path(), entry)?;
    st.log("gateway", "telemetry", "info", format!("{hostname} hardware values saved."));
    Ok(st.snapshot())
}

#[tauri::command]
pub fn set_node_status(state: tauri::State<Shared>, node_id: String, status: String) -> Result<Snapshot, String> {
    if !matches!(status.as_str(), "online" | "degraded" | "throttled" | "offline" | "sleeping") {
        return Err(format!("Unknown status {status}"));
    }
    let mut st = write_lock(state.inner());
    let node = st.node_mut(&node_id).ok_or_else(|| format!("Unknown node {node_id}"))?;
    node.status = status;
    let id = node.id.clone();
    orchestrator::announce_transition(&mut st, &id);
    Ok(st.snapshot())
}

#[tauri::command]
pub fn plan_objective(state: tauri::State<Shared>, objective: String) -> TaskPlan {
    let st = crate::state::read_lock(state.inner());
    orchestrator::plan(&st, &objective)
}

#[tauri::command]
pub fn set_inference_active(state: tauri::State<Shared>, active: bool) -> Snapshot {
    let mut st = write_lock(state.inner());
    st.ui_inference = active;
    st.sync_inference();
    st.snapshot()
}

#[tauri::command]
pub async fn apply_exit_node(state: tauri::State<'_, Shared>, node_id: Option<String>, confirm: bool) -> Result<Snapshot, String> {
    if !confirm {
        return Err("Confirm before changing the Tailscale exit node.".into());
    }
    let shared = state.inner().clone();
    let target = {
        let st = crate::state::read_lock(&shared);
        match node_id {
            Some(id) => {
                let node = st.nodes.iter().find(|node| node.id == id).ok_or_else(|| format!("Unknown node {id}"))?;
                Some(node.tailscale_ip.clone())
            }
            None => None,
        }
    };
    let message = tokio::task::spawn_blocking(move || set_exit_node(target))
        .await
        .map_err(|err| err.to_string())?;
    let mut st = write_lock(&shared);
    match message {
        Ok(text) => st.log("local", "tailscale", "info", text),
        Err(err) => {
            st.log("local", "tailscale", "error", err.clone());
            return Err(err);
        }
    }
    Ok(st.snapshot())
}

fn set_exit_node(ip: Option<String>) -> Result<String, String> {
    let arg = match ip {
        Some(ip) => {
            if ip.parse::<std::net::Ipv4Addr>().is_err() {
                return Err("Exit node address is not an IPv4 Tailscale address.".into());
            }
            format!("--exit-node={ip}")
        }
        None => "--exit-node=".into(),
    };
    let output = discovery::command("tailscale").args(["set", &arg]).output().map_err(|err| format!("tailscale set failed: {err}"))?;
    if output.status.success() {
        Ok(format!("tailscale set {arg}"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[tauri::command]
pub fn set_tool_policy(state: tauri::State<Shared>, root: String, shell_armed: bool, shard_backend: String) -> Result<Snapshot, String> {
    if !shard_backend.is_empty() && !(shard_backend.starts_with("http://") || shard_backend.starts_with("https://")) {
        return Err("Shard backend must be an http(s) URL.".into());
    }
    let mut st = write_lock(state.inner());
    st.tool_root = root.trim().to_string();
    st.shell_armed = shell_armed;
    st.shard_backend = shard_backend.trim().to_string();
    mcp::note_policy(&mut st);
    Ok(st.snapshot())
}

#[tauri::command]
pub async fn git_worktree_apply(
    repo: String,
    branch: String,
    files: Vec<FilePayload>,
    message: String,
    confirm: bool,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || apply_worktree(&repo, &branch, &files, &message, confirm))
        .await
        .map_err(|err| err.to_string())?
}

pub fn apply_worktree(
    repo: &str,
    branch: &str,
    files: &[FilePayload],
    message: &str,
    confirm: bool,
) -> Result<String, String> {
    if !confirm {
        return Err("Confirm before creating a git worktree.".into());
    }
    if branch.is_empty()
        || !branch
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '/' | '.'))
        || branch.contains("..")
        || branch.starts_with('/')
    {
        return Err("Branch name must stay inside a simple path.".into());
    }
    if message.trim().is_empty() {
        return Err("Commit message is empty.".into());
    }
    for file in files {
        if file.path.is_empty() || file.path.contains("..") || file.path.starts_with('/') || file.path.contains('\\') || file.path.contains(':') {
            return Err(format!("Refusing file path {}", file.path));
        }
    }
    let repo_path = PathBuf::from(repo);
    if !repo_path.join(".git").exists() {
        return Err("That folder is not a git checkout.".into());
    }
    let name = repo_path
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("repo");
    let parent = repo_path.parent().unwrap_or(std::path::Path::new("."));
    let slug = branch.replace('/', "-");
    let worktree = parent.join(format!("{name}-murmuration")).join(slug);
    if worktree.exists() {
        return Err(format!("{} already exists.", worktree.display()));
    }
    if let Some(parent_dir) = worktree.parent() {
        std::fs::create_dir_all(parent_dir).map_err(|err| err.to_string())?;
    }
    let created = discovery::command("git")
        .args(["worktree", "add", "-b", branch, &worktree.display().to_string()])
        .current_dir(&repo_path)
        .output()
        .map_err(|err| format!("git worktree failed: {err}"))?;
    if !created.status.success() {
        return Err(String::from_utf8_lossy(&created.stderr).trim().to_string());
    }
    for file in files {
        let dest = worktree.join(&file.path);
        if let Some(parent_dir) = dest.parent() {
            std::fs::create_dir_all(parent_dir).map_err(|err| err.to_string())?;
        }
        std::fs::write(&dest, &file.contents).map_err(|err| err.to_string())?;
    }
    let add = discovery::command("git")
        .args(["add", "--"])
        .args(files.iter().map(|file| file.path.as_str()))
        .current_dir(&worktree)
        .output()
        .map_err(|err| err.to_string())?;
    if !add.status.success() {
        return Err(String::from_utf8_lossy(&add.stderr).trim().to_string());
    }
    let commit = discovery::command("git")
        .args(["commit", "-m", message])
        .current_dir(&worktree)
        .output()
        .map_err(|err| err.to_string())?;
    if !commit.status.success() {
        let stderr = String::from_utf8_lossy(&commit.stderr);
        let stdout = String::from_utf8_lossy(&commit.stdout);
        return Err(format!("{stderr}{stdout}").trim().to_string());
    }
    let stdout = String::from_utf8_lossy(&commit.stdout);
    Ok(format!("Worktree {}\n{}", worktree.display(), stdout.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_requires_confirm() {
        let err = apply_worktree(".", "branch", &[], "message", false).unwrap_err();
        assert!(err.contains("Confirm"));
    }

    #[test]
    fn worktree_rejects_parent_paths() {
        let files = vec![FilePayload {
            path: "../secret".into(),
            contents: "no".into(),
        }];
        let err = apply_worktree(".", "murmuration/demo", &files, "message", true).unwrap_err();
        assert!(err.contains("Refusing") || err.contains("not a git"));
    }
}
