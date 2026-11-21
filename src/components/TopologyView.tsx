import { useEffect } from "react";
import {
  Background,
  BackgroundVariant,
  BaseEdge,
  Controls,
  Handle,
  Position,
  ReactFlow,
  useReactFlow,
  getBezierPath,
  useEdgesState,
  useNodesState,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useSwarm } from "../store";
import { FOX_CHAT_IN, FOX_CHAT_OUT, patchFoxChat, sendLinkedChat, setActiveFlow, useBoard, type PlacedFlow } from "../lib/tim/board";
import { gb, vramOf } from "../lib/format";
import type { SwarmNode } from "../types";

type MachineData = { node?: SwarmNode; gateway?: boolean };
type MachineFlowNode = Node<MachineData, "machine">;
type FlowCanvasNode = Node<{ flowId: string }, "flow">;
type ChatCanvasNode = Node<Record<string, never>, "chatIn" | "chatOut">;
type CanvasNode = MachineFlowNode | FlowCanvasNode | ChatCanvasNode;

function MachineCard({ data }: NodeProps<MachineFlowNode>) {
  if (data.gateway) {
    return (
      <div className="relative w-[210px] rounded-md border border-accent/40 bg-panel px-3 py-2 shadow-lg">
        <Handle type="target" position={Position.Left} />
        <Handle type="source" position={Position.Right} />
        <div className="text-[11px] uppercase tracking-[0.14em] text-faint">Gateway</div>
        <div className="mt-1 font-medium">Murmuration</div>
        <div className="font-mono text-[10px] text-mute">OpenAI · :11435/v1</div>
      </div>
    );
  }
  const node = data.node;
  if (!node) return null;
  const { total, allocated } = vramOf(node);
  const ratio = total === 0 ? 0 : allocated / total;
  return (
    <button className="relative w-[220px] rounded-md border border-line bg-panel px-3 py-2 text-left shadow-lg" onClick={() => useSwarm.getState().selectNode(node.id)}>
      <Handle type="target" position={Position.Left} />
      <Handle type="source" position={Position.Right} />
      <div className="flex items-center gap-2">
        <span className="h-1.5 w-1.5 rounded-full" style={{ background: node.color }} />
        <span className="truncate font-medium">{node.hostname}</span>
        <span className="ml-auto font-mono text-[10px] text-mute">{node.pingMs} ms</span>
      </div>
      <div className="mt-1 font-mono text-[10px] text-faint">
        {node.gpus[0]?.name ?? "no GPU probe"} · {node.relay}
        {node.derpRegion ? `:${node.derpRegion}` : ""}
      </div>
      <div className="mt-2 h-1.5 overflow-hidden rounded bg-canvas">
        <div className="h-full" style={{ width: `${Math.min(100, ratio * 100)}%`, background: node.color }} />
      </div>
      <div className="mt-1 flex font-mono text-[10px] text-mute">
        <span>{total === 0 ? "VRAM unknown" : `${gb(allocated)} / ${gb(total)} GB`}</span>
        <span className="ml-auto">{node.tokensPerSec.toFixed(0)} tok/s</span>
      </div>
      <div className="mt-1 text-[10px] uppercase tracking-[0.12em] text-faint">{node.role ?? "idle"} · {node.status}</div>
    </button>
  );
}

function PacketEdge(props: EdgeProps) {
  const [path, labelX, labelY] = getBezierPath(props);
  const active = Boolean(props.data && (props.data as { active?: boolean }).active);
  return (
    <>
      <BaseEdge id={props.id} path={path} style={{ stroke: "#2A3142", strokeWidth: 1.4 }} />
      {active && (
        <circle r="3.2" fill="#7EE0C6">
          <animateMotion dur="1.8s" repeatCount="indefinite" path={path} />
        </circle>
      )}
      <text x={labelX} y={labelY} fill="#8B95A8" fontSize="10" fontFamily="JetBrains Mono, monospace" textAnchor="middle">
        {props.label}
      </text>
    </>
  );
}

function FoxChatInCard() {
  const { running, runError } = useBoard();
  const input = useBoard().chatNodes.find((node) => node.type === "chat_in");
  const data = input?.data.kind === "chat_in" ? input.data : null;
  return (
    <div className="w-[220px] rounded-md border border-accent/40 bg-panel px-3 py-2 shadow-lg">
      <Handle type="source" position={Position.Right} />
      <div className="text-[11px] uppercase tracking-[0.14em] text-faint">FOX · Chat input</div>
      <textarea
        className="nodrag nowheel mt-2 h-16 w-full resize-none border border-line bg-canvas p-1 text-xs text-ink outline-none"
        placeholder={data?.placeholder ?? "Message FOX"}
        value={data?.text ?? ""}
        onChange={(event) => patchFoxChat("chat_in", event.target.value)}
      />
      <button className="nodrag mt-2 border border-line px-2 py-1 text-[11px] text-ink" disabled={running} onClick={() => void sendLinkedChat()}>
        {running ? "Asking LM Studio…" : "Send"}
      </button>
      {runError && <p className="mt-1 text-[10px] text-danger">{runError}</p>}
    </div>
  );
}

function FoxChatOutCard() {
  const output = useBoard().chatNodes.find((node) => node.type === "chat_out");
  const text = output?.data.kind === "chat_out" ? output.data.text : "";
  return (
    <div className="w-[220px] rounded-md border border-accent/40 bg-panel px-3 py-2 shadow-lg">
      <Handle type="target" position={Position.Left} />
      <div className="text-[11px] uppercase tracking-[0.14em] text-faint">FOX · Chat output</div>
      <pre className="nowheel mt-2 max-h-28 overflow-auto whitespace-pre-wrap font-sans text-xs text-ink">{text || "The FOX reply lands here."}</pre>
    </div>
  );
}

function FlowCard({ data }: NodeProps<FlowCanvasNode>) {
  const { placed, activeFlowId, running } = useBoard();
  const flow = placed.find((item) => item.id === data.flowId);
  if (!flow) return null;
  const active = flow.id === activeFlowId;
  return (
    <div
      className={`w-[230px] rounded-md border bg-panel px-3 py-2 text-left shadow-lg ${active ? "border-accent" : "border-line"}`}
      onClick={() => setActiveFlow(flow.id)}
    >
      <Handle type="target" position={Position.Left} />
      <Handle type="source" position={Position.Right} />
      <div className="font-mono text-[10px] uppercase text-faint">{flow.platform}</div>
      <div className="mt-1 truncate font-medium">{flow.name}</div>
      <div className="mt-1 text-[10px] text-mute">{flow.pending ? "Opening…" : `${flow.nodeCount} nodes · ${flow.edgeCount} edges`}</div>
      {flow.steps[0] && <p className="mt-1 line-clamp-3 text-[10px] text-faint">{flow.steps[0]}</p>}
      {flow.error && <p className="mt-1 text-[10px] text-danger">{flow.error}</p>}
      <button
        className="nodrag mt-2 border border-line px-2 py-1 text-[11px] text-ink"
        disabled={running || flow.pending || Boolean(flow.error)}
        onClick={(event) => {
          event.stopPropagation();
          setActiveFlow(flow.id);
          void sendLinkedChat();
        }}
      >
        {running && active ? "Running…" : "Run through FOX"}
      </button>
    </div>
  );
}

const nodeTypes = { machine: MachineCard, chatIn: FoxChatInCard, chatOut: FoxChatOutCard, flow: FlowCard };
const edgeTypes = { packet: PacketEdge };

function ringPosition(index: number, count: number): { x: number; y: number } {
  const angle = (Math.PI * 2 * index) / Math.max(1, count) - Math.PI / 2;
  return { x: 470 + Math.cos(angle) * 280, y: 230 + Math.sin(angle) * 190 };
}

function FitPlaced({ count }: { count: number }) {
  const { fitView } = useReactFlow();
  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      void fitView({ padding: 0.2, duration: 180 });
    });
    return () => cancelAnimationFrame(frame);
  }, [count, fitView]);
  return null;
}

function flowEdges(placed: PlacedFlow[], activeFlowId: string | null, running: boolean): Edge[] {
  if (placed.length === 0) {
    return [{ id: "fox-link", source: FOX_CHAT_IN, target: FOX_CHAT_OUT, type: "packet", label: "FOX", data: { active: running } }];
  }
  return placed.flatMap((flow) => {
    const active = running && flow.id === activeFlowId;
    return [
      { id: `in-${flow.id}`, source: FOX_CHAT_IN, target: flow.id, type: "packet", label: "in", data: { active } },
      { id: `out-${flow.id}`, source: flow.id, target: FOX_CHAT_OUT, type: "packet", label: "out", data: { active } },
    ];
  });
}

export function TopologyView() {
  const snapshot = useSwarm((state) => state.snapshot);
  const placedFlows = useBoard().placed;
  const activeFlowId = useBoard().activeFlowId;
  const flowRunning = useBoard().running;
  const [nodes, setNodes, onNodesChange] = useNodesState<CanvasNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);

  const linkNodes = snapshot.nodes.filter((node) => !node.simulated);

  useEffect(() => {
    setNodes((current) => {
      const placed = new Map(current.map((node) => [node.id, node.position]));
      const next: CanvasNode[] = [];
      if (linkNodes.length > 0) {
        next.push({
          id: "gateway",
          type: "machine",
          position: placed.get("gateway") ?? { x: 430, y: 250 },
          data: { gateway: true },
        });
      }
      linkNodes.forEach((node, index) => {
        next.push({
          id: node.id,
          type: "machine",
          position: placed.get(node.id) ?? ringPosition(index, linkNodes.length),
          data: { node },
        });
      });
      next.push({ id: FOX_CHAT_IN, type: "chatIn", position: placed.get(FOX_CHAT_IN) ?? { x: 24, y: 80 }, data: {} });
      placedFlows.forEach((flow, index) => {
        next.push({
          id: flow.id,
          type: "flow",
          position: placed.get(flow.id) ?? { x: 280, y: 48 + index * 150 },
          data: { flowId: flow.id },
        });
      });
      next.push({ id: FOX_CHAT_OUT, type: "chatOut", position: placed.get(FOX_CHAT_OUT) ?? { x: 548, y: 80 }, data: {} });
      return next;
    });
    const nextEdges: Edge[] = flowEdges(placedFlows, activeFlowId, flowRunning);
    for (const node of linkNodes) {
      if (node.status === "offline" || node.status === "sleeping") continue;
      nextEdges.push({
        id: `gw-${node.id}`,
        source: "gateway",
        target: node.id,
        type: "packet",
        label: `${node.throughputMbps.toFixed(0)} MB/s`,
        data: { active: snapshot.inferenceActive },
      });
    }
    const ordered = linkNodes.filter((node) => node.role && node.role !== "standby" && node.role !== "this device" && node.status !== "offline");
    if (snapshot.mode === "pipeline" || snapshot.mode === "speculative") {
      for (let index = 0; index < ordered.length - 1; index += 1) {
        const from = ordered[index];
        const to = ordered[index + 1];
        if (!from || !to) continue;
        nextEdges.push({
          id: `role-${from.id}-${to.id}`,
          source: from.id,
          target: to.id,
          type: "packet",
          label: snapshot.mode === "speculative" ? "draft" : (from.role ?? ""),
          data: { active: snapshot.inferenceActive },
        });
      }
    }
    if (snapshot.mode === "shard") {
      const sharders = linkNodes.filter((node) => node.role === "shard");
      for (let index = 0; index < sharders.length; index += 1) {
        const from = sharders[index];
        const to = sharders[(index + 1) % sharders.length];
        if (!from || !to || from.id === to.id) continue;
        nextEdges.push({
          id: `shard-${from.id}-${to.id}`,
          source: from.id,
          target: to.id,
          type: "packet",
          label: "layers",
          data: { active: snapshot.inferenceActive },
        });
      }
    }
    setEdges(nextEdges);
  }, [snapshot, placedFlows, activeFlowId, flowRunning, setEdges, setNodes]);

  return (
    <div className="relative h-full w-full">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        fitView
        colorMode="dark"
        minZoom={0.25}
        maxZoom={1.5}
        nodesConnectable={false}
      >
        <Background variant={BackgroundVariant.Lines} gap={22} color="#1E2330" />
        <Controls showInteractive={false} />
        <FitPlaced count={placedFlows.length} />
      </ReactFlow>
      {linkNodes.length === 0 && <div className="pointer-events-none absolute left-3 top-3 max-w-sm text-xs text-mute">{snapshot.linkMessage}</div>}
    </div>
  );
}
