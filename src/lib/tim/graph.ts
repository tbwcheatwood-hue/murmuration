import type { AgentData, ChatInData, ChatIntegration, ChatOutData, FoxProfile, GraphNode, LlmData, NodeKind, RouterData, ToolData, ToolServer } from "./types";

export const LM_STUDIO_BASE = "http://127.0.0.1:1234/v1";

export const FOX_PROFILES: { id: FoxProfile; label: string; hint: string }[] = [
  { id: "safe", label: "Safe", hint: "Chat and reasoning. LM Studio keeps the model loaded." },
  { id: "power", label: "Power", hint: "Coding and engineering, still served by LM Studio." },
  { id: "vision", label: "Vision", hint: "Image tasks. Load a vision model in LM Studio first." },
];

export function defaultIntegration(): ChatIntegration {
  return {
    provider: "lmstudio",
    baseUrl: LM_STUDIO_BASE,
    model: "",
    foxProfile: "safe",
    temperature: 0.4,
    maxTokens: 1024,
    tools: [],
    advisorOnly: true,
  };
}

export const TOOL_CATALOG: { id: ToolServer; label: string; hint: string; approval: boolean }[] = [
  { id: "filesystem", label: "Filesystem", hint: "List, read, and write files inside the workspace.", approval: false },
  { id: "workspace_info", label: "Workspace info", hint: "Path, file count, and size of the workspace.", approval: false },
  { id: "git", label: "Git", hint: "status, log, or diff inside the workspace.", approval: false },
  { id: "bash", label: "Windows shell", hint: "Runs the command in cmd.exe inside the workspace after you approve it.", approval: true },
  { id: "sqlite", label: "SQLite query", hint: "One read-only SELECT against a .sqlite file in the workspace.", approval: false },
  { id: "memory", label: "Agent memory", hint: "Recent notes stored for this mesh.", approval: false },
  { id: "web_search", label: "Web search", hint: "DuckDuckGo instant answer. Needs network.", approval: false },
  { id: "fetch_url", label: "HTTP fetch", hint: "GET an http or https URL after you approve it.", approval: true },
  { id: "mcp_bridge", label: "MCP bridge", hint: "POST a tool call to an http(s) bridge URL you set.", approval: true },
];

function nid(prefix: string): string {
  return `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
}

export function createNode(kind: NodeKind, position: { x: number; y: number }): GraphNode {
  if (kind === "agent") {
    const data: AgentData = {
      kind: "agent",
      name: "Agent",
      role: "Collaborator",
      systemPrompt: "Imported into Murmuration. Review the prompt before relying on it.",
      broadcast: false,
      humanGate: false,
      remember: true,
    };
    return { id: nid("agent"), type: "agent", position, data };
  }
  if (kind === "llm") {
    const data: LlmData = {
      kind: "llm",
      model: "",
      gpuLayers: 100,
      nCtx: 8192,
      threads: 8,
      temperature: 0.7,
      topK: 40,
      topP: 0.95,
      repeatPenalty: 1.1,
    };
    return { id: nid("llm"), type: "llm", position, data };
  }
  if (kind === "tool") return createToolNode("filesystem", position);
  if (kind === "chat_in") {
    const data: ChatInData = { kind: "chat_in", name: "Chat input", placeholder: "Message FOX", text: "", integration: defaultIntegration() };
    return { id: nid("chat-in"), type: "chat_in", position, data };
  }
  if (kind === "chat_out") {
    const data: ChatOutData = { kind: "chat_out", name: "Chat output", text: "", integration: defaultIntegration() };
    return { id: nid("chat-out"), type: "chat_out", position, data };
  }
  const data: RouterData = { kind: "router", name: "Route", routes: [{ id: "write", label: "Write", match: "write" }], fallback: "Continue" };
  return { id: nid("router"), type: "router", position, data };
}

export function createToolNode(server: ToolServer, position: { x: number; y: number }): GraphNode {
  const preset = TOOL_CATALOG.find((item) => item.id === server);
  const data: ToolData = { kind: "tool", server, label: preset?.label ?? server, approval: preset?.approval ?? false };
  return { id: nid("tool"), type: "tool", position, data };
}
