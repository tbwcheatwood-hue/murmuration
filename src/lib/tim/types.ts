export type NodeKind = "agent" | "llm" | "tool" | "router" | "chat_in" | "chat_out";
export type FoxProfile = "safe" | "power" | "vision";
export type Relation = "flow" | "brain" | "tool";
export type ToolServer =
  | "filesystem"
  | "bash"
  | "git"
  | "sqlite"
  | "memory"
  | "web_search"
  | "fetch_url"
  | "workspace_info"
  | "mcp_bridge";

export type AgentData = {
  kind: "agent";
  name: string;
  role: string;
  systemPrompt: string;
  broadcast: boolean;
  humanGate: boolean;
  remember: boolean;
};

export type LlmData = {
  kind: "llm";
  model: string;
  gpuLayers: number;
  nCtx: number;
  threads: number;
  temperature: number;
  topK: number;
  topP: number;
  repeatPenalty: number;
};

export type ToolData = {
  kind: "tool";
  server: ToolServer;
  label: string;
  approval: boolean;
};

export type RouterData = {
  kind: "router";
  name: string;
  routes: { id: string; label: string; match: string }[];
  fallback: string;
};

export type ChatIntegration = {
  provider: "lmstudio";
  baseUrl: string;
  model: string;
  foxProfile: FoxProfile;
  temperature: number;
  maxTokens: number;
  tools: ToolServer[];
  advisorOnly: boolean;
};

export type ChatInData = {
  kind: "chat_in";
  name: string;
  placeholder: string;
  text: string;
  integration: ChatIntegration;
};

export type ChatOutData = {
  kind: "chat_out";
  name: string;
  text: string;
  integration: ChatIntegration;
};

export type NodeData = AgentData | LlmData | ToolData | RouterData | ChatInData | ChatOutData;

export type GraphNode = {
  id: string;
  type: NodeKind;
  position: { x: number; y: number };
  data: NodeData;
};

export type GraphEdge = {
  id: string;
  source: string;
  target: string;
  sourceHandle?: string | null;
  targetHandle?: string | null;
  type?: string;
  data?: { relation?: Relation };
};

export type WorkflowPlatform = "langflow" | "flowise" | "n8n" | "unknown";

export type ConversionLine = {
  severity: "ok" | "warning" | "unsupported";
  message: string;
  sourceRef?: string;
};

export type WorkflowImportMeta = {
  source: WorkflowPlatform;
  originalName?: string;
  description?: string;
  importedAt: string;
  sourceNodeCount?: number;
  sourceEdgeCount?: number;
  notSemanticEquivalent: true;
};

export type GraphWithMeta = { nodes: GraphNode[]; edges: GraphEdge[]; meta?: WorkflowImportMeta };

export type ImportConversionResult = {
  ok: boolean;
  error?: string;
  name: string;
  graph?: GraphWithMeta;
  summary: ConversionLine[];
  detectedSource: WorkflowPlatform;
};

export type CatalogHit = {
  platform: string;
  name: string;
  path: string;
  rawUrl: string;
};

export function redactSecret(value: unknown): string {
  if (typeof value !== "string") return "[redacted]";
  const text = value.trim();
  if (text.length <= 8) return "[redacted]";
  return `${text.slice(0, 3)}…[redacted]`;
}
