export type NodeStatus = "online" | "degraded" | "throttled" | "offline" | "sleeping";
export type RelayKind = "direct" | "derp" | "local" | "unknown";
export type Thermal = "cool" | "warm" | "hot" | "throttling";
export type SwarmMode = "pipeline" | "shard" | "speculative";

export interface GpuMetric {
  name: string;
  vramTotalGb: number;
  vramAllocatedGb: number;
  utilization: number;
}

export interface SwarmNode {
  id: string;
  hostname: string;
  tailscaleIp: string;
  os: string;
  pingMs: number;
  relay: RelayKind | string;
  derpRegion: string | null;
  lmLinkBound: boolean;
  lmLinkPort: number | null;
  upstreamAlive: boolean;
  gpus: GpuMetric[];
  unifiedMemoryGb: number | null;
  tokensPerSec: number;
  thermal: Thermal | string;
  role: string | null;
  simulated: boolean;
  status: NodeStatus | string;
  throughputMbps: number;
  color: string;
  lastSeen: string;
}

export interface ModelArtifact {
  id: string;
  name: string;
  format: string;
  quant: string;
  sizeGb: number;
  paramsB: number;
  hostNodeId: string;
  architecture: string;
  contextMax: number;
  layers: number;
  kvHeads: number;
  headDim: number;
  loaded: boolean;
  simulated: boolean;
  sharded: boolean;
  shardNodes: string[];
  path: string | null;
}

export interface LogLine {
  id: string;
  ts: string;
  nodeId: string;
  source: string;
  level: "info" | "warn" | "error" | string;
  message: string;
}

export interface TrafficEntry {
  id: string;
  ts: string;
  method: string;
  path: string;
  status: number;
  ttftMs: number;
  routedTo: string;
  decision: string;
  requestPreview: string;
  responsePreview: string;
  upstream: string;
}

export interface BlueprintRole {
  role: string;
  modelHint: string;
  minVramGb: number;
}

export interface Blueprint {
  id: string;
  name: string;
  description: string;
  mode: SwarmMode | string;
  roles: BlueprintRole[];
}

export interface InferenceParams {
  contextLength: number;
  kvQuant: string;
  flashAttention: boolean;
  gpuOffloadLayers: number;
  speculative: boolean;
  draftNodeId: string | null;
  evictForFit: boolean;
}

export interface Transfer {
  id: string;
  modelId: string;
  modelName: string;
  fromNode: string;
  toNode: string;
  progress: number;
}

export interface Snapshot {
  nodes: SwarmNode[];
  models: ModelArtifact[];
  logs: LogLine[];
  traffic: TrafficEntry[];
  blueprints: Blueprint[];
  activeBlueprintId: string;
  mode: SwarmMode | string;
  gatewayPort: number;
  gatewayToken: string;
  demoMesh: boolean;
  livePeerCount: number;
  params: InferenceParams;
  transfers: Transfer[];
  inferenceActive: boolean;
  failoverNote: string | null;
  toolRoot: string;
  shellArmed: boolean;
  shardBackend: string;
  meshSource: string;
  linkStatus: string;
  linkMessage: string;
}

export interface ShardSlice {
  nodeId: string;
  hostname: string;
  layers: number;
  vramGb: number;
  kind: "resident" | "cpu" | "unplaced" | string;
}

export interface FitResult {
  modelId: string;
  weightsGb: number;
  kvGb: number;
  scratchGb: number;
  totalGb: number;
  pooledGb: number;
  fits: boolean;
  warnings: string[];
  slices: ShardSlice[];
}

export interface PlanStep {
  id: string;
  role: string;
  nodeId: string;
  hostname: string;
  model: string;
  live: boolean;
  output: string;
}

export interface TaskPlan {
  id: string;
  objective: string;
  steps: PlanStep[];
  artifact: string;
  warnings: string[];
  combinedPreview: string;
}

export type Activity = "nodes" | "models" | "blueprints" | "extensions";
export type WorkspaceTab = "topology" | "workbench" | "traffic";
