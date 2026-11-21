import { createNode, createToolNode } from "./graph";
import type { AgentData, ChatInData, ChatOutData, ConversionLine, GraphEdge, GraphNode, GraphWithMeta, ImportConversionResult, RouterData, ToolData } from "./types";
import { pushSecretWarnings } from "./util";

type N8nNode = { id?: string; name: string; type: string; position?: [number, number]; parameters?: Record<string, unknown> };
type N8nPayload = { name?: string; nodes?: N8nNode[]; connections?: Record<string, { main?: { node: string; type: string; index: number }[][] }> };

export function detectN8n(raw: unknown): boolean {
  if (!raw || typeof raw !== "object") return false;
  const nodes = (raw as N8nPayload).nodes;
  return Array.isArray(nodes) && nodes.some((node) => typeof node.type === "string" && node.type.startsWith("n8n-nodes"));
}

export function convertN8n(raw: unknown): ImportConversionResult {
  const summary: ConversionLine[] = [];
  if (!detectN8n(raw)) {
    return { ok: false, error: "Not an n8n workflow export (expected nodes with n8n-nodes-base.* types).", name: "", summary, detectedSource: "n8n" };
  }
  const record = raw as N8nPayload;
  const srcNodes = record.nodes ?? [];
  pushSecretWarnings(record, "n8n", summary);
  const idByName = new Map<string, string>();
  const nodes: GraphNode[] = [];
  for (const src of srcNodes) {
    const kind = src.type.toLowerCase();
    const pos = { x: Number(src.position?.[0] ?? 0), y: Number(src.position?.[1] ?? 0) };
    let node: GraphNode;
    if (kind.includes("chattrigger") || (kind.includes("chat") && kind.includes("trigger"))) {
      node = createNode("chat_in", pos);
      (node.data as ChatInData).name = src.name;
    } else if (kind.includes("chatoutput") || src.name.toLowerCase().includes("chat output")) {
      node = createNode("chat_out", pos);
      (node.data as ChatOutData).name = src.name;
    } else if (kind.includes("lmchat") || kind.includes("openai") || kind.includes("ollama") || kind.includes("anthropic")) node = createNode("llm", pos);
    else if (kind.includes("httprequest") || kind.includes("webhook")) {
      node = createToolNode("fetch_url", pos);
      const data = node.data as ToolData;
      data.label = src.name;
      data.approval = true;
    } else if (kind.includes("executecommand") || kind.includes("code")) {
      node = createToolNode("bash", pos);
      const data = node.data as ToolData;
      data.label = src.name;
      data.approval = true;
    } else if (kind.includes("if") || kind.includes("switch")) {
      node = createNode("router", pos);
      (node.data as RouterData).name = src.name;
    } else {
      node = createNode("agent", pos);
      const data = node.data as AgentData;
      data.name = src.name;
      data.role = src.type.replace(/^n8n-nodes-base\./, "");
      data.systemPrompt = `Placeholder for n8n node "${src.name}" (${src.type}). Schedules and credentials do not transfer.`;
    }
    idByName.set(src.name, node.id);
    nodes.push(node);
    summary.push({
      severity: kind.includes("stickynote") || kind.includes("noop") ? "unsupported" : "warning",
      message: `n8n ${src.type} → ${node.type} (behavior not equivalent)`,
      sourceRef: src.name,
    });
  }
  const edges: GraphEdge[] = [];
  for (const [fromName, blocks] of Object.entries(record.connections ?? {})) {
    const source = idByName.get(fromName);
    if (!source) continue;
    for (const outputs of blocks.main ?? []) {
      for (const link of outputs ?? []) {
        const target = idByName.get(link.node);
        if (!target) continue;
        edges.push({ id: `e-${crypto.randomUUID().slice(0, 8)}`, source, target, sourceHandle: "flow-out", targetHandle: "flow-in", type: "mesh", data: { relation: "flow" } });
      }
    }
  }
  summary.push({ severity: "warning", message: "n8n triggers, credentials, and per-item execution are not represented." });
  const graph: GraphWithMeta = {
    nodes,
    edges,
    meta: {
      source: "n8n",
      originalName: record.name,
      importedAt: new Date().toISOString(),
      sourceNodeCount: srcNodes.length,
      sourceEdgeCount: edges.length,
      notSemanticEquivalent: true,
    },
  };
  return { ok: true, name: String(record.name || "Imported n8n"), graph, summary, detectedSource: "n8n" };
}
