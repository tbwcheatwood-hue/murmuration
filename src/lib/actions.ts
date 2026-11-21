import type { CatalogHit } from "./tim/types";
import type { FitResult, InferenceParams, Snapshot, TaskPlan } from "../types";
import { browser, currentMesh, previewTraffic } from "./mesh";

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

export async function getSnapshot(): Promise<Snapshot> {
  if (!inTauri()) return currentMesh();
  return call<Snapshot>("get_snapshot");
}

export async function refreshDiscovery(): Promise<Snapshot> {
  if (!inTauri()) return currentMesh();
  return call("refresh_discovery");
}

export async function setMode(mode: string): Promise<Snapshot> {
  if (!inTauri()) return browser.setMode(mode);
  return call("set_mode", { mode });
}

export async function activateBlueprint(blueprintId: string): Promise<Snapshot> {
  if (!inTauri()) return browser.activateBlueprint(blueprintId);
  return call("activate_blueprint", { blueprintId });
}

export async function updateParams(params: InferenceParams): Promise<Snapshot> {
  if (!inTauri()) return browser.updateParams(params);
  return call("update_params", { params });
}

export async function mountModel(modelId: string, nodeId: string): Promise<Snapshot> {
  if (!inTauri()) {
    const result = browser.mount(modelId, nodeId);
    if (typeof result === "string") throw new Error(result);
    return result;
  }
  return call("mount_model", { modelId, nodeId, invokeLms: false });
}

export async function pullModel(modelId: string, nodeId: string): Promise<Snapshot> {
  if (!inTauri()) return browser.pull(modelId, nodeId);
  return call("pull_model", { modelId, nodeId });
}

export async function rebalance(): Promise<Snapshot> {
  if (!inTauri()) return browser.rebalance();
  return call("rebalance");
}

export async function calculateFit(modelId: string, params: InferenceParams): Promise<FitResult> {
  if (!inTauri()) {
    const { calculateFit: fit } = await import("./fit");
    const snapshot = currentMesh();
    const model = snapshot.models.find((item) => item.id === modelId);
    if (!model) throw new Error(`Unknown model ${modelId}`);
    return fit(model, snapshot.nodes, params);
  }
  return call("calculate_fit", { modelId, params });
}

export async function commitShard(modelId: string): Promise<Snapshot> {
  if (!inTauri()) {
    const result = browser.commitShard(modelId);
    if (typeof result === "string") throw new Error(result);
    return result;
  }
  return call("commit_shard", { modelId });
}

export async function setDemoMesh(enabled: boolean): Promise<Snapshot> {
  if (!inTauri()) return browser.setDemo(enabled);
  return call("set_demo_mesh", { enabled });
}

export async function injectLoad(): Promise<Snapshot> {
  if (!inTauri()) return browser.injectLoad();
  return call("inject_load");
}

export async function setNodeHardware(nodeId: string, vramGb: number, ramGb: number, cpu: string, gpu: string): Promise<Snapshot> {
  if (!inTauri()) return browser.setHardware(nodeId, vramGb, ramGb, cpu, gpu);
  return call("set_node_hardware", { nodeId, vramGb, ramGb, cpu, gpu });
}

export async function setNodeStatus(nodeId: string, status: string): Promise<Snapshot> {
  if (!inTauri()) return browser.setStatus(nodeId, status);
  return call("set_node_status", { nodeId, status });
}

export async function planObjective(objective: string): Promise<TaskPlan> {
  if (!inTauri()) {
    previewTraffic(objective, currentMesh().gatewayPort);
    return browser.plan(objective);
  }
  return call("plan_objective", { objective });
}

export async function setInferenceActive(active: boolean): Promise<Snapshot> {
  if (!inTauri()) return browser.setInference(active);
  return call("set_inference_active", { active });
}

export async function applyExitNode(nodeId: string | null, confirm: boolean): Promise<Snapshot> {
  if (!inTauri()) {
    if (!confirm) throw new Error("Confirm before changing the Tailscale exit node.");
    throw new Error("Exit-node changes run in the desktop shell, against the local tailscale binary.");
  }
  return call("apply_exit_node", { nodeId, confirm });
}

export async function setToolPolicy(root: string, shellArmed: boolean, shardBackend: string): Promise<Snapshot> {
  if (!inTauri()) return browser.setPolicy(root, shellArmed, shardBackend);
  return call("set_tool_policy", { root, shellArmed, shardBackend });
}

export async function gitWorktreeApply(repo: string, branch: string, files: { path: string; contents: string }[], message: string, confirm: boolean): Promise<string> {
  if (!confirm) throw new Error("Confirm before creating a git worktree.");
  if (!inTauri()) {
    const lines = [
      "The preview cannot create a worktree. In the desktop shell this runs:",
      `git -C ${repo} worktree add -b ${branch} <sibling>/${branch.replaceAll("/", "-")}`,
      ...files.map((file) => `write ${file.path} (${file.contents.length} bytes)`),
      `git commit -m ${JSON.stringify(message)}`,
    ];
    return lines.join("\n");
  }
  return call("git_worktree_apply", { repo, branch, files, message, confirm });
}

export async function listFlowCatalog(platform: string): Promise<CatalogHit[]> {
  if (!inTauri()) throw new Error("Live catalog pull runs in the desktop app.");
  return call("tim_list_catalog", { platform });
}

export async function fetchFlowWorkflow(rawUrl: string): Promise<string> {
  if (!inTauri()) throw new Error("Remote workflow download runs in the desktop app.");
  return call("tim_fetch_workflow", { rawUrl });
}

export type FoxStatus = { manager: string; endpoint: string; online: boolean; models: string[]; message: string };

export async function foxStatus(): Promise<FoxStatus> {
  if (!inTauri()) return { manager: "lmstudio", endpoint: "http://127.0.0.1:1234/v1", online: false, models: [], message: "FOX status runs in the desktop app." };
  return call("fox_status");
}

export async function foxChat(prompt: string, model: string, temperature: number, maxTokens: number, profile: string, tools: string[]): Promise<string> {
  if (!inTauri()) throw new Error("FOX chat runs in the desktop app, through LM Studio.");
  return call("fox_chat", { prompt, model, temperature, maxTokens, profile, tools });
}

export async function runCanvasTool(server: string, args: Record<string, unknown>, workspace: string, approved: boolean): Promise<string> {
  if (!inTauri()) throw new Error("Canvas tools run in the desktop app.");
  return call("tim_run_tool", { server, args, workspace, approved });
}
