import type { Blueprint, InferenceParams, LogLine, Snapshot, SwarmNode, TaskPlan, Transfer } from "../types";
import { calculateFit } from "./fit";
import { clip, nowHms, nowIso, uid, vramOf } from "./format";

type Bases = { ping: number; util: number; tokens: number };

const bases = new Map<string, Bases>();
let tick = 0;
let stressTicks = 0;
let mesh = createBoot();

function createBoot(): Snapshot {
  const snapshot: Snapshot = {
    nodes: [],
    models: [],
    logs: [],
    traffic: [],
    blueprints: blueprints(),
    activeBlueprintId: "refactor",
    mode: "pipeline",
    gatewayPort: 11435,
    gatewayToken: "browser-preview",
    demoMesh: false,
    livePeerCount: 0,
    params: {
      contextLength: 8192,
      kvQuant: "q8",
      flashAttention: true,
      gpuOffloadLayers: 256,
      speculative: false,
      draftNodeId: null,
      evictForFit: true,
    },
    transfers: [],
    inferenceActive: false,
    failoverNote: null,
    toolRoot: "",
    shellArmed: false,
    shardBackend: "",
    meshSource: "unavailable",
    linkStatus: "unavailable",
    linkMessage: "This window cannot see your LM Link. The desktop app reads lms link status for the signed-in account.",
  };
  log(snapshot, "gateway", "lms", "info", snapshot.linkMessage);
  return snapshot;
}

function role(roleName: string, hint: string, min: number) {
  return { role: roleName, modelHint: hint, minVramGb: min };
}

function blueprints(): Blueprint[] {
  return [
    {
      id: "refactor",
      name: "Full-Stack Repo Refactor",
      description: "Architect plans the change, the coder writes the patch, and the critic checks the diff before it lands.",
      mode: "pipeline",
      roles: [role("architect", "qwen2.5-32b-instruct", 20), role("coder", "qwen2.5-coder-14b", 12), role("critic", "llama-3.2-3b-instruct", 8)],
    },
    {
      id: "research",
      name: "Deep Research & Synthesis",
      description: "A large node gathers the argument, the critic compresses it, and the embedding model keeps the notes searchable.",
      mode: "pipeline",
      roles: [role("architect", "qwen2.5-32b-instruct", 20), role("critic", "llama-3.2-3b-instruct", 8), role("embedder", "nomic-embed-text", 4)],
    },
    {
      id: "rag",
      name: "Local RAG Pipeline",
      description: "Embeddings stay on the unified-memory node. The architect answers only after the retrieved notes are attached.",
      mode: "pipeline",
      roles: [role("embedder", "nomic-embed-text", 4), role("architect", "qwen2.5-32b-instruct", 20), role("critic", "llama-3.2-3b-instruct", 8)],
    },
    {
      id: "shard-70b",
      name: "70B Layer Shard",
      description: "Split a 70B-class model across the pooled cards when no single GPU can hold the weights and KV cache.",
      mode: "shard",
      roles: [role("shard", "llama-3.1-70b-instruct", 8)],
    },
    {
      id: "speculative",
      name: "Speculative Mesh Decode",
      description: "The laptop drafts tokens. The desktop GPU accepts or rejects them over the LM Link hop.",
      mode: "speculative",
      roles: [role("draft", "qwen2.5-coder-1.5b", 4), role("coder", "qwen2.5-coder-14b", 12)],
    },
  ];
}

function log(snapshot: Snapshot, nodeId: string, source: string, level: LogLine["level"], message: string) {
  snapshot.logs.push({ id: uid("log"), ts: nowHms(), nodeId, source, level, message });
  if (snapshot.logs.length > 400) snapshot.logs.splice(0, snapshot.logs.length - 400);
}

function healthy(node: SwarmNode): boolean {
  return node.status === "online" || node.status === "degraded";
}

function hot(snapshot: Snapshot, nodeId: string): boolean {
  return snapshot.models.some((model) => model.loaded && (model.hostNodeId === nodeId || model.shardNodes.includes(nodeId)));
}

export function currentMesh(): Snapshot {
  return structuredClone(mesh);
}

export function replaceMesh(next: Snapshot) {
  mesh = next;
}

export function tickMesh() {
  tick += 1;
  if (stressTicks > 0) stressTicks -= 1;
  const stress = stressTicks > 0;
  for (const node of mesh.nodes) {
    if (node.status === "offline" || node.status === "sleeping") {
      node.tokensPerSec = 0;
      node.throughputMbps = 0.1;
      if (node.gpus[0]) node.gpus[0].utilization *= 0.8;
      continue;
    }
    const base = bases.get(node.id) ?? { ping: node.pingMs, util: 20, tokens: node.tokensPerSec };
    if (node.simulated) {
      const salt = [...node.id].reduce((sum, ch) => sum + ch.charCodeAt(0), 0);
      const wave = Math.sin(tick * 0.37 + salt * 0.15);
      const util = Math.min(99, Math.max(3, base.util + wave * 7 + (stress ? 30 : 0)));
      if (node.gpus[0]) node.gpus[0].utilization = util;
      node.pingMs = Math.max(1, Math.round(base.ping + wave * 2));
      if (stress && util > 93) {
        node.thermal = "throttling";
        node.status = "throttled";
        if (!mesh.failoverNote?.includes(node.hostname)) {
          mesh.failoverNote = `${node.hostname} is throttled. New prompts will skip it.`;
          log(mesh, node.id, "orchestrator", "warn", mesh.failoverNote);
        }
      } else if (node.status === "throttled" && !stress && util < 80) {
        node.status = "online";
        node.thermal = "warm";
        mesh.failoverNote = null;
        log(mesh, node.id, "orchestrator", "info", `${node.hostname} is back on the mesh.`);
      } else if (node.status !== "throttled") {
        node.thermal = util > 85 ? "hot" : util > 55 ? "warm" : "cool";
      }
    }
    if (hot(mesh, node.id) && healthy(node)) {
      node.tokensPerSec = base.tokens * 0.92 + (mesh.inferenceActive ? 8 : 0);
    } else {
      node.tokensPerSec = 0;
    }
    const link = node.relay === "derp" ? 16 : node.relay === "local" ? 0.4 : 84;
    node.throughputMbps = Math.max(0.15, link * (mesh.inferenceActive ? 1 : 0.06));
    node.lastSeen = nowIso();
  }
  const pending: Transfer[] = [];
  for (const transfer of mesh.transfers) {
    transfer.progress = Math.min(1, transfer.progress + 0.17);
    if (transfer.progress >= 1) {
      const found = mesh.models.find((model) => model.id === transfer.modelId);
      if (found) found.hostNodeId = transfer.toNode;
      const dest = mesh.nodes.find((node) => node.id === transfer.toNode)?.hostname ?? transfer.toNode;
      log(mesh, "gateway", "orchestrator", "info", `Catalog pointer for ${transfer.modelName} now sits on ${dest}. No byte transfer was started.`);
    } else {
      pending.push(transfer);
    }
  }
  mesh.transfers = pending;
}

export function startBrowserMesh(push: (snapshot: Snapshot) => void): () => void {
  push(currentMesh());
  const timer = window.setInterval(() => {
    tickMesh();
    push(currentMesh());
  }, 1200);
  return () => window.clearInterval(timer);
}

function mutate(change: (snapshot: Snapshot) => void): Snapshot {
  change(mesh);
  return currentMesh();
}

export const browser = {
  snapshot: () => currentMesh(),
  setMode(mode: string): Snapshot {
    return mutate((snapshot) => {
      snapshot.mode = mode;
      snapshot.params.speculative = mode === "speculative";
      log(snapshot, "gateway", "orchestrator", "info", `Distribution mode is now ${mode}.`);
    });
  },
  activateBlueprint(id: string): Snapshot {
    return mutate((snapshot) => {
      const blueprint = snapshot.blueprints.find((item) => item.id === id);
      if (!blueprint) return;
      snapshot.activeBlueprintId = id;
      snapshot.mode = blueprint.mode;
      snapshot.params.speculative = blueprint.mode === "speculative";
      rebalance(snapshot);
    });
  },
  updateParams(params: InferenceParams): Snapshot {
    return mutate((snapshot) => {
      snapshot.params = params;
      if (params.speculative) snapshot.mode = "speculative";
      else if (snapshot.mode === "speculative") snapshot.mode = "pipeline";
    });
  },
  mount(modelId: string, nodeId: string): string | Snapshot {
    const model = mesh.models.find((item) => item.id === modelId);
    const node = mesh.nodes.find((item) => item.id === nodeId);
    if (!model || !node) return "Unknown model or node.";
    const { total, allocated } = vramOf(node);
    if (allocated + model.sizeGb > total + 0.25) {
      return `${node.hostname} has ${Math.max(0, total - allocated).toFixed(1)} GB free and ${model.sizeGb.toFixed(1)} GB is required. Shard it or pick another node.`;
    }
    return mutate((snapshot) => {
      const target = snapshot.nodes.find((item) => item.id === nodeId)!;
      const artifact = snapshot.models.find((item) => item.id === modelId)!;
      if (target.gpus[0]) target.gpus[0].vramAllocatedGb = Math.min(target.gpus[0].vramTotalGb, target.gpus[0].vramAllocatedGb + artifact.sizeGb);
      artifact.loaded = true;
      artifact.sharded = false;
      artifact.shardNodes = [];
      artifact.hostNodeId = nodeId;
      const base = bases.get(nodeId);
      if (base) base.tokens = Math.max(base.tokens, 20);
      log(snapshot, nodeId, "lms", "info", `Reserved ${artifact.name} on ${target.hostname}. lms load was not called.`);
    });
  },
  pull(modelId: string, nodeId: string): Snapshot {
    return mutate((snapshot) => {
      const model = snapshot.models.find((item) => item.id === modelId);
      if (!model || snapshot.transfers.some((transfer) => transfer.modelId === modelId)) return;
      snapshot.transfers.push({
        id: uid("xfer"),
        modelId,
        modelName: model.name,
        fromNode: model.hostNodeId,
        toNode: nodeId,
        progress: 0.08,
      });
      log(snapshot, "gateway", "orchestrator", "info", `Catalog move started for ${model.name}. This does not copy weight bytes.`);
    });
  },
  rebalance(): Snapshot {
    return mutate((snapshot) => rebalance(snapshot));
  },
  commitShard(modelId: string): string | Snapshot {
    const model = mesh.models.find((item) => item.id === modelId);
    if (!model) return "Unknown model.";
    const fit = calculateFit(model, mesh.nodes, mesh.params);
    if (!fit.fits) return fit.warnings.join(" ");
    return mutate((snapshot) => {
      if (snapshot.params.evictForFit) {
        for (const item of snapshot.models) {
          item.loaded = false;
          item.sharded = false;
          item.shardNodes = [];
        }
        for (const node of snapshot.nodes) {
          for (const gpu of node.gpus) gpu.vramAllocatedGb = 0;
          const base = bases.get(node.id);
          if (base) base.tokens = 0;
          node.tokensPerSec = 0;
        }
      }
      const shardNodes: string[] = [];
      const notes: string[] = [];
      for (const slice of fit.slices.filter((item) => item.kind === "resident")) {
        const node = snapshot.nodes.find((item) => item.id === slice.nodeId);
        if (!node?.gpus[0]) continue;
        node.gpus[0].vramAllocatedGb = Math.min(node.gpus[0].vramTotalGb, node.gpus[0].vramAllocatedGb + slice.vramGb);
        node.role = "shard";
        const base = bases.get(node.id);
        if (base) base.tokens = 18;
        node.tokensPerSec = 18;
        shardNodes.push(node.id);
        notes.push(`${node.hostname} ${slice.layers}L/${slice.vramGb.toFixed(1)}GB`);
      }
      const artifact = snapshot.models.find((item) => item.id === modelId)!;
      artifact.loaded = true;
      artifact.sharded = true;
      artifact.shardNodes = shardNodes;
      snapshot.mode = "shard";
      log(snapshot, "gateway", "orchestrator", "info", `Sharded ${artifact.name} across ${notes.join(", ")}`);
      if (!snapshot.shardBackend) {
        log(snapshot, "gateway", "orchestrator", "warn", "Shard map is committed in the orchestrator. Set a llama.cpp RPC backend before weights actually split.");
      }
    });
  },
  setDemo(_enabled: boolean): Snapshot {
    return mutate((snapshot) => {
      snapshot.demoMesh = false;
      snapshot.nodes = snapshot.nodes.filter((node) => !node.simulated);
      snapshot.models = snapshot.models.filter((model) => !model.simulated);
      log(snapshot, "gateway", "lms", "info", "Devices outside this LM Link stay off the list. Nothing was removed from disk.");
    });
  },
  injectLoad(): Snapshot {
    stressTicks = 16;
    return mutate((snapshot) => {
      log(snapshot, "gateway", "telemetry", "warn", "Stress load injected. Thermal failover can trip if a node crosses 93%.");
    });
  },
  setHardware(nodeId: string, vramGb: number, ramGb: number, cpu: string, gpu: string): Snapshot {
    return mutate((snapshot) => {
      const node = snapshot.nodes.find((item) => item.id === nodeId);
      if (!node) return;
      node.configuredVramGb = Math.max(0, vramGb);
      node.ramGb = Math.max(0, ramGb);
      node.cpuName = cpu.trim();
      node.gpuName = gpu.trim();
      const measured = node.gpus.reduce((sum, gpuMetric) => sum + gpuMetric.vramTotalGb, 0);
      if (measured > 0) {
        node.effectiveVramGb = measured;
        node.vramSource = "measured";
      } else if (node.configuredVramGb > 0) {
        node.effectiveVramGb = node.configuredVramGb;
        node.vramSource = "configured";
      } else if (node.inferredVramGb > 0) {
        node.effectiveVramGb = node.inferredVramGb;
        node.vramSource = "inferred";
      } else {
        node.effectiveVramGb = 0;
        node.vramSource = "none";
      }
      log(snapshot, node.id, "telemetry", "info", `${node.hostname} hardware values saved.`);
    });
  },
  setStatus(nodeId: string, status: string): Snapshot {
    return mutate((snapshot) => {
      const node = snapshot.nodes.find((item) => item.id === nodeId);
      if (!node) return;
      node.status = status;
      if (status === "offline" || status === "sleeping" || status === "throttled") {
        snapshot.failoverNote = `${node.hostname} is ${status}. New prompts will skip it.`;
        log(snapshot, node.id, "orchestrator", "warn", snapshot.failoverNote);
      } else {
        snapshot.failoverNote = null;
        log(snapshot, node.id, "orchestrator", "info", `${node.hostname} is ${status}.`);
      }
    });
  },
  setInference(active: boolean): Snapshot {
    return mutate((snapshot) => {
      snapshot.inferenceActive = active;
    });
  },
  setPolicy(root: string, shellArmed: boolean, shardBackend: string): Snapshot {
    return mutate((snapshot) => {
      snapshot.toolRoot = root.trim();
      snapshot.shellArmed = shellArmed;
      snapshot.shardBackend = shardBackend.trim();
      log(snapshot, "gateway", "mcp", "info", `Tool root ${snapshot.toolRoot || "unset"} · shell ${shellArmed ? "armed" : "disarmed"}`);
    });
  },
  plan(objective: string): TaskPlan {
    return buildPlan(mesh, objective);
  },
  recordTraffic(entry: Omit<Snapshot["traffic"][number], "id" | "ts">) {
    mutate((snapshot) => {
      snapshot.traffic.push({ ...entry, id: uid("req"), ts: nowIso() });
      if (snapshot.traffic.length > 160) snapshot.traffic.splice(0, snapshot.traffic.length - 160);
    });
  },
};

function capacityOf(node: SwarmNode) {
  const measured = vramOf(node).total;
  if (measured > 0) return { gb: measured, tier: 3 };
  if (node.configuredVramGb > 0) return { gb: node.configuredVramGb, tier: 2 };
  if (node.inferredVramGb > 0) return { gb: node.inferredVramGb, tier: 1 };
  return { gb: 0, tier: 0 };
}

function rebalance(snapshot: Snapshot) {
  const blueprint = snapshot.blueprints.find((item) => item.id === snapshot.activeBlueprintId);
  if (!blueprint) return;
  for (const node of snapshot.nodes) {
    const capacity = capacityOf(node);
    node.effectiveVramGb = capacity.gb;
    node.vramSource = capacity.tier === 3 ? "measured" : capacity.tier === 2 ? "configured" : capacity.tier === 1 ? "inferred" : "none";
  }
  const ranked = snapshot.nodes
    .filter((node) => healthy(node) && capacityOf(node).gb > 0)
    .sort((a, b) => capacityOf(b).gb - capacityOf(a).gb || capacityOf(b).tier - capacityOf(a).tier || a.hostname.localeCompare(b.hostname));
  for (const node of snapshot.nodes) {
    if (healthy(node)) node.role = "standby";
  }
  const notes: string[] = [];
  if (blueprint.mode === "speculative" && ranked.length >= 2) {
    const target = ranked[0];
    const draft = ranked[ranked.length - 1];
    if (target && draft) {
      target.role = "coder";
      draft.role = "draft";
      snapshot.params.draftNodeId = draft.id;
      snapshot.params.speculative = true;
      notes.push(`${draft.hostname} drafts for ${target.hostname}`);
    }
  } else {
    blueprint.roles.forEach((item, index) => {
      const node = ranked[index];
      if (node) {
        node.role = item.role;
        notes.push(`${node.hostname} → ${item.role}`);
      } else notes.push(`no healthy node for ${item.role}`);
    });
  }
  log(snapshot, "gateway", "orchestrator", "info", `Rebalanced ${blueprint.name} (${notes.join(", ")})`);
}

function buildPlan(snapshot: Snapshot, objective: string): TaskPlan {
  const blueprint = snapshot.blueprints.find((item) => item.id === snapshot.activeBlueprintId) ?? snapshot.blueprints[0]!;
  const warnings: string[] = [];
  const steps = [];
  const textFor = (roleName: string) => {
    if (roleName === "architect") return `Plan\n1. Name the files and contracts the objective touches.\n2. Keep the patch small enough for one worktree.\n3. Hand the coder a concrete edit list.\n\nObjective\n${objective}`;
    if (roleName === "coder") return `Patch brief\nApply the architect's edit list for:\n${objective}\n\nWrite the change as a reviewable diff. Do not restyle unrelated code.`;
    if (roleName === "critic") return `Review\nCheck the diff against: ${objective}\nLook for missing edge states and tests that would have caught a regression.`;
    if (roleName === "embedder") return `Retrieval notes\nIndex the local files that mention the terms in:\n${objective}\nReturn the top passages before the architect answers.`;
    if (roleName === "draft") return `Draft pass\nPropose a short patch for:\n${objective}`;
    return `${roleName}\n${objective}`;
  };
  const roles = snapshot.mode === "speculative" ? ["draft", "coder"] : blueprint.roles.map((item) => item.role);
  for (const roleName of roles) {
    const node = snapshot.nodes.find((item) => item.role === roleName && healthy(item)) ?? snapshot.nodes.find((item) => healthy(item) && vramOf(item).total > 0);
    const resident = snapshot.models.find((item) => item.loaded && node && (item.hostNodeId === node.id || item.shardNodes.includes(node.id)));
    steps.push({
      id: `step-${roleName}`,
      role: roleName,
      nodeId: node?.id ?? "unassigned",
      hostname: node?.hostname ?? "unassigned",
      model: resident?.name ?? "unmounted",
      live: Boolean(node && node.upstreamAlive && !node.simulated),
      output: textFor(roleName),
    });
  }
  if (snapshot.nodes.some((node) => node.relay === "derp" && healthy(node) && node.role && node.role !== "standby")) {
    warnings.push("A role is pinned to a DERP peer. Expect extra time to first token.");
  }
  const title = objective.split(/\s+/).slice(0, 8).join(" ");
  const artifact =
    blueprint.id === "research"
      ? `# Synthesis\n\nObjective: ${objective}\n\n## Findings\n- The claim needs a source note before it is treated as a result.\n`
      : blueprint.id === "rag"
        ? `# retrieval\nquery: ${title}\ntop_k: 6\nembed_node: harbor-mbp\nanswer_node: atlas-4090\n`
        : `diff --git a/src/swarm_change.ts b/src/swarm_change.ts\n--- a/src/swarm_change.ts\n+++ b/src/swarm_change.ts\n@@\n+// Murmuration worktree: ${title}\n+export function applyObjective() {\n+  return ${JSON.stringify(objective)};\n+}\n`;
  return {
    id: uid("plan"),
    objective,
    steps,
    artifact,
    warnings,
    combinedPreview: steps.map((step) => `## ${step.role}\n${step.output}`).join("\n\n"),
  };
}

export function previewTraffic(objective: string, port: number) {
  const routed = mesh.nodes.find((node) => node.role === "architect" && healthy(node)) ?? mesh.nodes.find((node) => healthy(node));
  browser.recordTraffic({
    method: "POST",
    path: "/v1/chat/completions",
    status: 200,
    ttftMs: routed?.relay === "derp" ? 280 : 96,
    routedTo: routed?.hostname ?? "none",
    decision: `Pipeline brief via ${routed?.hostname ?? "none"}. Gateway ${port} is the desktop shell; this preview recorded the decision locally.`,
    requestPreview: clip(objective, 280),
    responsePreview: clip(buildPlan(mesh, objective).combinedPreview, 280),
    upstream: "brief",
  });
}
