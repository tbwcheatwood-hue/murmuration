import { useSyncExternalStore } from "react";
import { foxChat } from "../actions";
import { useSwarm } from "../../store";
import { createNode } from "./graph";
import type { ChatInData, ChatOutData, GraphNode, ImportConversionResult } from "./types";

export const FOX_CHAT_IN = "fox-chat-in";
export const FOX_CHAT_OUT = "fox-chat-out";

export type PlacedFlow = {
  id: string;
  name: string;
  platform: string;
  steps: string[];
  nodeCount: number;
  edgeCount: number;
  pending: boolean;
  error: string;
};

type BoardState = {
  chatNodes: GraphNode[];
  placed: PlacedFlow[];
  activeFlowId: string | null;
  running: boolean;
  runError: string;
};

function foxPair(): GraphNode[] {
  const input = createNode("chat_in", { x: 40, y: 520 });
  const output = createNode("chat_out", { x: 720, y: 520 });
  input.id = FOX_CHAT_IN;
  output.id = FOX_CHAT_OUT;
  return [input, output];
}

let board: BoardState = {
  chatNodes: foxPair(),
  placed: [],
  activeFlowId: null,
  running: false,
  runError: "",
};

const listeners = new Set<() => void>();

function publish(next: BoardState) {
  board = next;
  listeners.forEach((listener) => listener());
}

export function useBoard(): BoardState {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => board,
  );
}

export function flowNodeId(key: string): string {
  const slug = key.replace(/[^a-zA-Z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 72);
  return `flow-${slug || "pasted"}`;
}

function stepLines(result: ImportConversionResult): string[] {
  if (!result.ok || !result.graph) return [result.error || "This export did not convert."];
  const names = result.graph.nodes
    .filter((node) => node.type !== "chat_in" && node.type !== "chat_out")
    .map((node) => {
      const data = node.data;
      if (data.kind === "agent") return data.name;
      if (data.kind === "tool") return data.label;
      if (data.kind === "router") return data.name;
      if (data.kind === "llm") return data.model || "LM Studio model";
      return "";
    })
    .filter((name) => name.length > 0);
  if (names.length === 0) return result.summary.slice(0, 6).map((line) => line.message);
  return names.slice(0, 8).map((name, index) => `${index + 1}. ${name}`);
}

export function stageFlow(key: string, name: string, platform: string) {
  const id = flowNodeId(key);
  const current = board.placed.find((item) => item.id === id);
  const next: PlacedFlow = current ?? {
    id,
    name,
    platform,
    steps: ["Opening this flow on the topology."],
    nodeCount: 0,
    edgeCount: 0,
    pending: true,
    error: "",
  };
  const placed = current ? board.placed.map((item) => (item.id === id ? { ...next, name, platform } : item)) : [...board.placed, next];
  publish({ ...board, placed, activeFlowId: id, runError: "" });
  useSwarm.getState().setTab("topology");
}

export function commitFlow(key: string, platform: string, result: ImportConversionResult) {
  const id = flowNodeId(key);
  const next: PlacedFlow = {
    id,
    name: result.name || key,
    platform,
    steps: stepLines(result),
    nodeCount: result.graph?.nodes.length ?? 0,
    edgeCount: result.graph?.edges.length ?? 0,
    pending: false,
    error: result.ok ? "" : result.error || "This export did not convert.",
  };
  const placed = board.placed.some((item) => item.id === id) ? board.placed.map((item) => (item.id === id ? next : item)) : [...board.placed, next];
  publish({ ...board, placed, activeFlowId: id });
  useSwarm.getState().setTab("topology");
}

export function setActiveFlow(id: string) {
  publish({ ...board, activeFlowId: id });
}

export function setChatNodes(nodes: GraphNode[]) {
  publish({ ...board, chatNodes: nodes });
}

export function patchFoxChat(kind: "chat_in" | "chat_out", text: string) {
  publish({
    ...board,
    chatNodes: board.chatNodes.map((node) => (node.type === kind ? { ...node, data: { ...node.data, text } } : node)),
  });
}

export function foxChatNodes(): { input: ChatInData | null; output: ChatOutData | null } {
  const input = board.chatNodes.find((node) => node.type === "chat_in");
  const output = board.chatNodes.find((node) => node.type === "chat_out");
  return {
    input: input?.data.kind === "chat_in" ? input.data : null,
    output: output?.data.kind === "chat_out" ? output.data : null,
  };
}

export async function sendLinkedChat(): Promise<void> {
  if (board.running) return;
  const { input } = foxChatNodes();
  if (!input) return;
  const flow = board.placed.find((item) => item.id === board.activeFlowId);
  const prompt =
    flow && !flow.pending && !flow.error
      ? `Run the imported flow "${flow.name}" (${flow.platform}, ${flow.nodeCount} nodes). LM Studio is the model. Follow these steps, then answer the user.\n${flow.steps.map((step) => `- ${step}`).join("\n")}\n\nUser:\n${input.text}`
      : input.text;
  publish({ ...board, running: true, runError: "" });
  try {
    const reply = await foxChat(prompt, input.integration.model, input.integration.temperature, input.integration.maxTokens, input.integration.foxProfile, input.integration.tools);
    publish({
      ...board,
      running: false,
      chatNodes: board.chatNodes.map((node) => (node.type === "chat_out" ? { ...node, data: { ...node.data, text: reply } } : node)),
    });
  } catch (err) {
    publish({ ...board, running: false, runError: err instanceof Error ? err.message : String(err) });
  }
}
