import { useEffect, useState } from "react";
import { foxStatus, type FoxStatus } from "../lib/actions";
import { sendLinkedChat, useBoard } from "../lib/tim/board";
import { FOX_PROFILES, LM_STUDIO_BASE, TOOL_CATALOG, createNode } from "../lib/tim/graph";
import type { ChatInData, ChatIntegration, ChatOutData, GraphNode, ToolServer } from "../lib/tim/types";

function isChat(node: GraphNode): node is GraphNode & { data: ChatInData | ChatOutData } {
  return node.type === "chat_in" || node.type === "chat_out";
}

function ensureChatNodes(nodes: GraphNode[]): GraphNode[] {
  const next = [...nodes];
  if (!next.some((node) => node.type === "chat_in")) next.unshift(createNode("chat_in", { x: 40, y: 40 }));
  if (!next.some((node) => node.type === "chat_out")) next.push(createNode("chat_out", { x: 40, y: 220 }));
  return next;
}

export function ChatNodes({
  nodes,
  modelNames,
  onChange,
}: {
  nodes: GraphNode[];
  modelNames: string[];
  onChange: (nodes: GraphNode[]) => void;
}) {
  const board = ensureChatNodes(nodes);
  const input = board.find((node) => node.type === "chat_in");
  const output = board.find((node) => node.type === "chat_out");
  const inputData = input?.data as ChatInData | undefined;
  const outputData = output?.data as ChatOutData | undefined;
  const integration = inputData?.integration;
  const [status, setStatus] = useState<FoxStatus | null>(null);
  const [error, setError] = useState("");
  const running = useBoard().running;
  const runError = useBoard().runError;

  useEffect(() => {
    void foxStatus().then(setStatus).catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
  }, []);

  function patchIntegration(patch: Partial<ChatIntegration>) {
    onChange(
      board.map((node) => {
        if (!isChat(node)) return node;
        return { ...node, data: { ...node.data, integration: { ...node.data.integration, ...patch, provider: "lmstudio", baseUrl: LM_STUDIO_BASE } } };
      }),
    );
  }

  function patchText(kind: "chat_in" | "chat_out", text: string) {
    onChange(board.map((node) => (node.type === kind ? { ...node, data: { ...node.data, text } } : node)));
  }

  function toggleTool(server: ToolServer) {
    const current = integration?.tools ?? [];
    const tools = current.includes(server) ? current.filter((item) => item !== server) : [...current, server];
    patchIntegration({ tools });
  }

  async function send() {
    if (!integration || !inputData) return;
    setError("");
    await sendLinkedChat();
  }

  const listed = status?.models.length ? status.models : modelNames;

  return (
    <div className="border-b border-line p-3">
      <div className="flex items-baseline justify-between gap-3">
        <p className="text-[11px] uppercase tracking-[0.14em] text-faint">FOX chat</p>
        <p className="truncate text-[11px] text-mute">{status?.message ?? "Checking LM Studio…"}</p>
      </div>
      <div className="mt-3 grid grid-cols-2 gap-3">
        <label className="flex flex-col gap-1 text-[11px] text-mute">
          {inputData?.name ?? "Chat input"}
          <textarea
            className="h-24 border border-line bg-panel p-2 text-xs text-ink outline-none"
            placeholder={inputData?.placeholder ?? "Message FOX"}
            value={inputData?.text ?? ""}
            onChange={(event) => patchText("chat_in", event.target.value)}
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-mute">
          {outputData?.name ?? "Chat output"}
          <textarea className="h-24 border border-line bg-panel p-2 text-xs text-ink outline-none" readOnly value={outputData?.text ?? ""} />
        </label>
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <select
          className="border border-line bg-panel px-2 py-1 text-xs"
          value={integration?.foxProfile ?? "safe"}
          onChange={(event) => patchIntegration({ foxProfile: event.target.value as ChatIntegration["foxProfile"] })}
        >
          {FOX_PROFILES.map((profile) => (
            <option key={profile.id} value={profile.id}>
              {profile.label}
            </option>
          ))}
        </select>
        <select className="max-w-64 border border-line bg-panel px-2 py-1 text-xs" value={integration?.model ?? ""} onChange={(event) => patchIntegration({ model: event.target.value })}>
          <option value="">Loaded LM Studio model</option>
          {listed.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>
        <label className="flex items-center gap-1 text-[11px] text-mute">
          temp
          <input
            className="w-16 border border-line bg-panel px-2 py-1 text-xs"
            type="number"
            min={0}
            max={2}
            step={0.1}
            value={integration?.temperature ?? 0.4}
            onChange={(event) => patchIntegration({ temperature: Number(event.target.value) })}
          />
        </label>
        <label className="flex items-center gap-1 text-[11px] text-mute">
          tokens
          <input
            className="w-20 border border-line bg-panel px-2 py-1 text-xs"
            type="number"
            min={1}
            max={4096}
            value={integration?.maxTokens ?? 1024}
            onChange={(event) => patchIntegration({ maxTokens: Number(event.target.value) })}
          />
        </label>
        <button className="border border-line px-3 py-1 text-xs text-ink" onClick={() => void send()} disabled={running}>
          {running ? "Asking LM Studio…" : "Send"}
        </button>
      </div>
      <p className="mt-2 text-[11px] text-faint">{FOX_PROFILES.find((profile) => profile.id === integration?.foxProfile)?.hint} Endpoint {LM_STUDIO_BASE}.</p>
      <div className="mt-2 flex flex-wrap gap-1">
        {TOOL_CATALOG.map((tool) => {
          const on = integration?.tools.includes(tool.id) ?? false;
          return (
            <button key={tool.id} className={`border px-2 py-1 text-[10px] ${on ? "border-accent text-ink" : "border-line text-mute"}`} onClick={() => toggleTool(tool.id)}>
              {tool.label}
            </button>
          );
        })}
      </div>
      {(runError || error) && <p className="mt-2 text-xs text-danger">{runError || error}</p>}
    </div>
  );
}
