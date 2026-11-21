import { useState } from "react";
import { useSwarm } from "../store";
import { commitShard, mountModel, pullModel, activateBlueprint, refreshDiscovery, inTauri } from "../lib/actions";
import { cx, gb, hostName, vramOf } from "../lib/format";
import { ExtensionManager } from "./ExtensionManager";
import { FlowCatalog } from "./FlowStudio";
import type { SwarmNode } from "../types";

export function Sidebar() {
  const activity = useSwarm((state) => state.activity);
  const title =
    activity === "nodes" ? "LM Link" : activity === "models" ? "Model registry" : activity === "blueprints" ? "Blueprints" : activity === "flows" ? "Flows" : "Extensions";
  return (
    <aside className="flex w-[300px] shrink-0 flex-col border-r border-line bg-panel">
      <div className="flex h-9 items-center border-b border-line px-3 text-[11px] uppercase tracking-[0.14em] text-faint">{title}</div>
      <div className="min-h-0 flex-1 overflow-auto">
        {activity === "nodes" && <NodeTree />}
        {activity === "models" && <ModelRegistry />}
        {activity === "blueprints" && <BlueprintList />}
        {activity === "extensions" && <ExtensionManager />}
        {activity === "flows" && <FlowCatalog />}
      </div>
    </aside>
  );
}

function nodeHaystack(node: SwarmNode): string {
  const { total, allocated } = vramOf(node);
  return [
    node.hostname,
    node.status,
    node.role ?? "",
    node.cpuName,
    node.gpuName,
    node.vramSource,
    node.ramGb > 0 ? `${gb(node.ramGb)} ${node.ramGb}` : "",
    node.configuredVramGb > 0 ? `${gb(node.configuredVramGb)} ${node.configuredVramGb}` : "",
    node.effectiveVramGb > 0 ? `${gb(node.effectiveVramGb)} ${node.effectiveVramGb}` : "",
    total > 0 ? `${gb(total)} ${gb(allocated)} ${total} ${allocated}` : "",
    ...node.gpus.map((gpu) => gpu.name),
  ]
    .join(" ")
    .toLowerCase();
}

function matchesHardware(node: SwarmNode, query: string): boolean {
  const parts = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return true;
  const haystack = nodeHaystack(node);
  return parts.every((part) => haystack.includes(part));
}

function NodeTree() {
  const snapshot = useSwarm((state) => state.snapshot);
  const selected = useSwarm((state) => state.selectedNodeId);
  const [query, setQuery] = useState("");
  const nodes = snapshot.nodes.filter((node) => !node.simulated);
  const visible = nodes.filter((node) => matchesHardware(node, query));
  const waiting = snapshot.linkStatus === "pending" && nodes.length === 0;
  const empty = !waiting && nodes.length === 0;
  const unmatched = !waiting && !empty && visible.length === 0;
  return (
    <div className="flex min-h-full flex-col">
      <div className="flex items-start gap-2 px-3 py-2">
        <p className="min-w-0 flex-1 text-[11px] leading-4 text-mute">{snapshot.linkMessage}</p>
        <button
          className="shrink-0 rounded border border-line px-2 py-0.5 text-[11px] text-mute hover:text-ink"
          onClick={() => {
            if (!inTauri()) {
              useSwarm.getState().pushToast("LM Link status is read by the desktop app.", "warn");
              return;
            }
            void refreshDiscovery()
              .then((next) => useSwarm.getState().applySnapshot(next))
              .catch((error: Error) => useSwarm.getState().pushToast(error.message, "warn"));
          }}
        >
          Refresh
        </button>
      </div>
      {nodes.length > 0 && (
        <label className="block px-3 pb-2">
          <span className="sr-only">Search devices</span>
          <input
            className="w-full rounded border border-line bg-canvas px-2 py-1 text-[12px]"
            placeholder="Search VRAM, RAM, CPU, GPU"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
      )}
      {waiting && <p className="px-3 py-6 text-sm text-mute">Checking LM Link…</p>}
      {empty && <p className="px-3 py-6 text-sm text-mute">No devices on this LM Link.</p>}
      {unmatched && <p className="px-3 py-6 text-sm text-mute">No devices match.</p>}
      <ul className="pb-2">
        {visible.map((node) => (
          <NodeRow key={node.id} node={node} selected={selected === node.id} />
        ))}
      </ul>
    </div>
  );
}

function NodeRow({ node, selected }: { node: SwarmNode; selected: boolean }) {
  const { total, allocated } = vramOf(node);
  return (
    <li>
      <button
        className={cx("flex w-full flex-col gap-0.5 border-l-2 px-3 py-2 text-left", selected ? "border-accent bg-white/5" : "border-transparent hover:bg-white/[0.03]")}
        onClick={() => useSwarm.getState().selectNode(node.id)}
      >
        <div className="flex items-center gap-2">
          <Led status={node.status} />
          <span className="truncate font-medium">{node.hostname}</span>
          {node.pingMs > 0 && <span className="ml-auto font-mono text-[10px] text-mute">{node.pingMs} ms</span>}
        </div>
        <div className="truncate pl-4 font-mono text-[10px] text-faint">
          {node.status}
          {node.role ? ` · ${node.role}` : ""}
          {total > 0 ? ` · ${gb(allocated)} / ${gb(total)} GB` : node.effectiveVramGb > 0 ? ` · ${node.vramSource} ${gb(node.effectiveVramGb)} GB` : ""}
        </div>
        {(node.cpuName || node.gpuName || node.ramGb > 0) && (
          <div className="truncate pl-4 font-mono text-[10px] text-faint">
            {[node.cpuName, node.gpuName, node.ramGb > 0 ? `${gb(node.ramGb)} GB RAM` : ""].filter(Boolean).join(" · ")}
          </div>
        )}
      </button>
    </li>
  );
}

function Led({ status }: { status: string }) {
  const color = status === "online" ? "text-mint" : status === "degraded" || status === "throttled" ? "text-warn" : "text-danger";
  return <span className={cx("led", color)} style={{ background: "currentColor" }} />;
}

function ModelRegistry() {
  const snapshot = useSwarm((state) => state.snapshot);
  const selected = useSwarm((state) => state.selectedModelId);
  const nodes = snapshot.nodes.filter((node) => node.gpus.length > 0 || node.id === "local");
  return (
    <ul className="py-1">
      {snapshot.models.map((model) => (
        <li key={model.id} className={cx("border-l-2 px-3 py-2", selected === model.id ? "border-accent bg-white/5" : "border-transparent")}>
          <button className="w-full text-left" onClick={() => useSwarm.getState().selectModel(model.id)}>
            <div className="flex items-baseline gap-2">
              <span className="truncate">{model.name}</span>
              <span className="ml-auto font-mono text-[10px] text-faint">{model.format}</span>
            </div>
            <div className="mt-1 font-mono text-[10px] text-mute">
              {model.quant} · {gb(model.sizeGb)} GB · {model.paramsB}B · {model.sharded ? `sharded ×${model.shardNodes.length}` : hostName(snapshot.nodes, model.hostNodeId)}
              {model.loaded ? " · resident" : ""}
            </div>
          </button>
          <div className="mt-2 flex gap-1">
            <NodePick
              label="Pull"
              nodes={nodes}
              onPick={(nodeId) => {
                void pullModel(model.id, nodeId)
                  .then((next) => useSwarm.getState().applySnapshot(next))
                  .catch((error: Error) => useSwarm.getState().pushToast(error.message, "danger"));
              }}
            />
            <button
              className="rounded border border-line px-2 py-0.5 text-[11px] text-mute hover:text-ink"
              onClick={() => {
                void commitShard(model.id)
                  .then((next) => {
                    useSwarm.getState().applySnapshot(next);
                    useSwarm.getState().pushToast(`Shard plan committed for ${model.name}.`, "info");
                  })
                  .catch((error: Error) => useSwarm.getState().pushToast(error.message, "warn"));
              }}
            >
              Shard
            </button>
            <NodePick
              label="Mount"
              nodes={nodes}
              onPick={(nodeId) => {
                void mountModel(model.id, nodeId)
                  .then((next) => useSwarm.getState().applySnapshot(next))
                  .catch((error: Error) => useSwarm.getState().pushToast(error.message, "warn"));
              }}
            />
          </div>
        </li>
      ))}
    </ul>
  );
}

function NodePick({ label, nodes, onPick }: { label: string; nodes: SwarmNode[]; onPick: (id: string) => void }) {
  return (
    <select
      className="rounded border border-line bg-canvas px-1 py-0.5 text-[11px] text-mute"
      value=""
      onChange={(event) => {
        if (event.target.value) onPick(event.target.value);
        event.target.value = "";
      }}
    >
      <option value="">{label}</option>
      {nodes.map((node) => (
        <option key={node.id} value={node.id}>
          {label} → {node.hostname}
        </option>
      ))}
    </select>
  );
}

function BlueprintList() {
  const snapshot = useSwarm((state) => state.snapshot);
  return (
    <ul className="py-1">
      {snapshot.blueprints.map((blueprint) => {
        const active = snapshot.activeBlueprintId === blueprint.id;
        return (
          <li key={blueprint.id}>
            <button
              className={cx("w-full border-l-2 px-3 py-2 text-left", active ? "border-accent bg-white/5" : "border-transparent hover:bg-white/[0.03]")}
              onClick={() => {
                void activateBlueprint(blueprint.id).then((next) => useSwarm.getState().applySnapshot(next));
              }}
            >
              <div className="flex items-center gap-2">
                <span>{blueprint.name}</span>
                {active && <span className="ml-auto font-mono text-[10px] text-mint">active</span>}
              </div>
              <p className="mt-1 text-[12px] leading-snug text-mute">{blueprint.description}</p>
              <div className="mt-1 font-mono text-[10px] text-faint">{blueprint.mode}</div>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
