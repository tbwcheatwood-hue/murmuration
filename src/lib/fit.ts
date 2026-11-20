import type { FitResult, InferenceParams, ModelArtifact, ShardSlice, SwarmNode } from "../types";
import { vramOf } from "./format";

export function calculateFit(model: ModelArtifact, nodes: SwarmNode[], params: InferenceParams): FitResult {
  const bytes = params.kvQuant === "fp16" ? 2 : params.kvQuant === "q4" ? 0.5 : 1;
  const kvGb = (2 * model.layers * model.kvHeads * model.headDim * params.contextLength * bytes) / 1e9;
  const scratchFull = (model.layers / 40) * 0.45;
  const scratchGb = params.flashAttention ? scratchFull * 0.55 : scratchFull;
  const offloadLayers = Math.min(params.gpuOffloadLayers, model.layers);
  const ratio = model.layers === 0 ? 1 : offloadLayers / model.layers;
  const gpuWeights = model.sizeGb * ratio;
  const cpuWeights = model.sizeGb - gpuWeights;
  const gpuNeed = gpuWeights + kvGb + scratchGb;
  const capacityOf = (node: SwarmNode) => {
    const { total, allocated } = vramOf(node);
    return params.evictForFit ? total : Math.max(0, total - allocated);
  };
  const pool = nodes
    .filter((node) => (node.status === "online" || node.status === "degraded") && capacityOf(node) > 0.2)
    .sort((a, b) => capacityOf(b) - capacityOf(a) || a.hostname.localeCompare(b.hostname));
  const pooledGb = pool.reduce((sum, node) => sum + capacityOf(node), 0);
  const warnings: string[] = [];
  const slices: ShardSlice[] = [];
  if (cpuWeights > 0.2) {
    warnings.push(`${cpuWeights.toFixed(1)} GB of weights stay on CPU. GPU offload is ${offloadLayers}/${model.layers} layers.`);
    slices.push({
      nodeId: "cpu",
      hostname: "CPU",
      layers: Math.max(0, model.layers - offloadLayers),
      vramGb: cpuWeights,
      kind: "cpu",
    });
  }
  if (nodes.some((node) => ["throttled", "offline", "sleeping"].includes(node.status) && node.gpus.length > 0)) {
    warnings.push("Unhealthy nodes were left out of the slice.");
  }
  let remaining = gpuNeed;
  const placed: { node: SwarmNode; take: number }[] = [];
  for (const node of pool) {
    if (remaining <= 0.05) break;
    const take = Math.min(remaining, capacityOf(node));
    if (take <= 0.05) continue;
    placed.push({ node, take });
    remaining -= take;
  }
  const placedGb = placed.reduce((sum, item) => sum + item.take, 0);
  const gbFits = remaining <= 0.2;
  let layersLeft = offloadLayers;
  placed.forEach((item, index) => {
    const last = index + 1 === placed.length;
    let layers = 0;
    if (placedGb > 0.01) {
      layers = gbFits && last ? layersLeft : Math.round((item.take / gpuNeed) * offloadLayers);
    }
    layers = Math.min(layers, layersLeft);
    layersLeft -= layers;
    slices.push({
      nodeId: item.node.id,
      hostname: item.node.hostname,
      layers,
      vramGb: item.take,
      kind: "resident",
    });
    if (item.node.relay === "derp" || item.node.pingMs > 40) {
      warnings.push(`${item.node.hostname} is a slow hop (${item.node.pingMs} ms, relay or DERP). Layers placed there add a pipeline bubble.`);
    }
  });
  const fits = remaining <= 0.2;
  if (!fits) {
    warnings.push(`${remaining.toFixed(1)} GB does not fit in the ${pooledGb.toFixed(1)} GB pool.`);
    slices.push({ nodeId: "unplaced", hostname: "Unplaced", layers: layersLeft, vramGb: remaining, kind: "unplaced" });
  }
  if (!params.evictForFit && gpuNeed > pooledGb + 0.2) {
    warnings.push("Turn on planning against total VRAM if current residents can be evicted.");
  }
  return {
    modelId: model.id,
    weightsGb: model.sizeGb,
    kvGb,
    scratchGb,
    totalGb: model.sizeGb + kvGb + scratchGb,
    pooledGb,
    fits,
    warnings,
    slices,
  };
}
