import { createNode, createToolNode } from "./graph";
import type { AgentData, ChatInData, ChatOutData, ConversionLine, GraphEdge, GraphNode, GraphWithMeta, ImportConversionResult, LlmData, RouterData, ToolData } from "./types";
import { componentLabel, nodeTypeName, pushSecretWarnings } from "./util";

type LangflowPayload = {
  name?: string;
  description?: string;
  data?: { nodes?: unknown[]; edges?: unknown[] };
  nodes?: unknown[];
  edges?: unknown[];
};

function mapKind(typeName: string): { kind: "agent" | "llm" | "router" | "tool-web" | "chat_in" | "chat_out"; note?: string } {
  const name = typeName.toLowerCase();
  if (/chatinput|textinput/.test(name)) return { kind: "chat_in", note: "kept as a chat input. LM Studio answers it." };
  if (/chatoutput|textoutput/.test(name)) return { kind: "chat_out", note: "kept as a chat output." };
  if (/prompt|^input$|output/.test(name)) return { kind: "agent", note: "mapped as agent (Langflow input semantics differ)" };
  if (/openai|anthropic|ollama|lmstudio|huggingface|groq|mistral|llm|model|chatgpt/.test(name)) return { kind: "llm" };
  if (/router|conditional|ifelse|route/.test(name)) return { kind: "router" };
  if (/agent|crew|autogen|supervisor/.test(name)) return { kind: "agent" };
  if (/wikipedia|duckduckgo|search|serp|tavily/.test(name)) return { kind: "tool-web", note: "mapped to web_search" };
  if (/bash|python|terminal|shell|code/.test(name)) return { kind: "tool-web", note: "mapped to the Windows shell tool" };
  if (/file|directory|read|write|document/.test(name)) return { kind: "tool-web", note: "mapped to filesystem" };
  return { kind: "agent", note: "represented as a generic agent" };
}

function extractModelName(node: Record<string, unknown>): string {
  const data = node.data as Record<string, unknown> | undefined;
  const nest = data?.node as Record<string, unknown> | undefined;
  const template = nest?.template as Record<string, unknown> | undefined;
  if (!template) return "";
  for (const key of ["model_name", "model", "model_id", "openai_api_base"]) {
    const field = template[key] as { value?: unknown } | undefined;
    if (typeof field?.value === "string") return field.value;
  }
  return "";
}

export function detectLangflow(raw: unknown): boolean {
  if (!raw || typeof raw !== "object") return false;
  const record = raw as LangflowPayload;
  const nodes = record.data?.nodes ?? record.nodes;
  const edges = record.data?.edges ?? record.edges;
  if (!Array.isArray(nodes) || !Array.isArray(edges)) return false;
  return nodes.some((item) => {
    if (!item || typeof item !== "object") return false;
    const node = item as Record<string, unknown>;
    const data = node.data as Record<string, unknown> | undefined;
    return Boolean(data?.node ?? data?.type ?? (typeof node.type === "string" && node.type.includes("Chat")));
  });
}

export function convertLangflow(raw: unknown): ImportConversionResult {
  const summary: ConversionLine[] = [];
  if (!detectLangflow(raw)) {
    return { ok: false, error: "Not a Langflow export (expected data.nodes and data.edges).", name: "", summary, detectedSource: "langflow" };
  }
  const record = raw as LangflowPayload;
  const srcNodes = (record.data?.nodes ?? record.nodes) as Record<string, unknown>[];
  const srcEdges = (record.data?.edges ?? record.edges) as Record<string, unknown>[];
  pushSecretWarnings(record, "langflow", summary);
  const idMap = new Map<string, string>();
  const nodes: GraphNode[] = [];
  for (const src of srcNodes) {
    const typeName = nodeTypeName(src);
    const map = mapKind(typeName);
    const position = (src.position as { x?: number; y?: number }) ?? { x: 0, y: 0 };
    const pos = { x: Number(position.x ?? 0), y: Number(position.y ?? 0) };
    let node: GraphNode;
    if (map.kind === "tool-web") {
      const server = /bash|python|shell|code/.test(typeName.toLowerCase()) ? "bash" : /search|wikipedia|duck/.test(typeName.toLowerCase()) ? "web_search" : "filesystem";
      node = createToolNode(server, pos);
      (node.data as ToolData).label = componentLabel(src);
    } else if (map.kind === "llm") {
      node = createNode("llm", pos);
      const model = extractModelName(src);
      if (model) (node.data as LlmData).model = model;
    } else if (map.kind === "router") {
      node = createNode("router", pos);
      (node.data as RouterData).name = componentLabel(src);
    } else if (map.kind === "chat_in") {
      node = createNode("chat_in", pos);
      (node.data as ChatInData).name = componentLabel(src);
    } else if (map.kind === "chat_out") {
      node = createNode("chat_out", pos);
      (node.data as ChatOutData).name = componentLabel(src);
    } else {
      node = createNode("agent", pos);
      const data = node.data as AgentData;
      data.name = componentLabel(src);
      data.role = typeName;
      data.systemPrompt = `Imported from Langflow component "${typeName}". Behavior is not equivalent. Review the prompt.`;
    }
    idMap.set(String(src.id), node.id);
    nodes.push(node);
    summary.push(
      map.note
        ? { severity: "warning", message: `${componentLabel(src)}: ${map.note}`, sourceRef: String(src.id) }
        : { severity: "ok", message: `Converted ${typeName} → ${node.type}`, sourceRef: String(src.id) },
    );
  }
  const edges: GraphEdge[] = [];
  for (const src of srcEdges) {
    const source = idMap.get(String(src.source));
    const target = idMap.get(String(src.target));
    if (!source || !target) {
      summary.push({ severity: "warning", message: `Dropped edge — missing endpoint (source=${String(src.source)}, target=${String(src.target)})` });
      continue;
    }
    edges.push({ id: `e-${crypto.randomUUID().slice(0, 8)}`, source, target, sourceHandle: "flow-out", targetHandle: "flow-in", type: "mesh", data: { relation: "flow" } });
  }
  summary.push({ severity: "warning", message: "Langflow ports become flow edges. Brain and tool edges are not inferred." });
  const graph: GraphWithMeta = {
    nodes,
    edges,
    meta: {
      source: "langflow",
      originalName: record.name,
      description: record.description,
      importedAt: new Date().toISOString(),
      sourceNodeCount: srcNodes.length,
      sourceEdgeCount: srcEdges.length,
      notSemanticEquivalent: true,
    },
  };
  return { ok: true, name: String(record.name || "Imported Langflow"), graph, summary, detectedSource: "langflow" };
}
