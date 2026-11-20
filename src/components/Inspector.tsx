import { useEffect } from "react";
import { applyExitNode, calculateFit, commitShard, setNodeStatus, setToolPolicy, updateParams } from "../lib/actions";
import { gb, vramOf } from "../lib/format";
import { useSwarm } from "../store";
import type { InferenceParams } from "../types";

export function Inspector() {
  const snapshot = useSwarm((state) => state.snapshot);
  const selectedNodeId = useSwarm((state) => state.selectedNodeId);
  const selectedModelId = useSwarm((state) => state.selectedModelId);
  const node = snapshot.nodes.find((item) => item.id === selectedNodeId);
  const model = snapshot.models.find((item) => item.id === selectedModelId);
  const fit = useSwarm((state) => state.fit);
  const params = snapshot.params;

  useEffect(() => {
    if (!model) {
      useSwarm.getState().setFit(null);
      return;
    }
    let cancel = false;
    void calculateFit(model.id, params)
      .then((result) => {
        if (!cancel) useSwarm.getState().setFit(result);
      })
      .catch(() => {
        if (!cancel) useSwarm.getState().setFit(null);
      });
    return () => {
      cancel = true;
    };
  }, [model, params]);

  const patch = (partial: Partial<InferenceParams>) => {
    const next = { ...params, ...partial };
    void updateParams(next).then((snap) => useSwarm.getState().applySnapshot(snap));
  };

  return (
    <aside className="flex w-[320px] shrink-0 flex-col border-l border-line bg-panel">
      <div className="flex h-9 items-center border-b border-line px-3 text-[11px] uppercase tracking-[0.14em] text-faint">Inspector</div>
      <div className="min-h-0 flex-1 overflow-auto px-3 py-3">
        {node && (
          <section className="mb-4">
            <h2 className="text-[14px] font-medium">{node.hostname}</h2>
            <p className="mt-1 font-mono text-[11px] text-mute">
              {node.tailscaleIp} · {node.os} · {node.relay}
              {node.derpRegion ? ` ${node.derpRegion}` : ""} · {node.thermal}
            </p>
            <dl className="mt-2 grid grid-cols-2 gap-y-1 font-mono text-[11px] text-mute">
              <dt>status</dt>
              <dd className="text-right text-ink">{node.status}</dd>
              <dt>role</dt>
              <dd className="text-right text-ink">{node.role ?? "idle"}</dd>
              <dt>tokens</dt>
              <dd className="text-right text-ink">{node.tokensPerSec.toFixed(1)} /s</dd>
              <dt>link</dt>
              <dd className="text-right text-ink">{node.lmLinkBound ? "LM Link" : "unbound"}</dd>
            </dl>
            {node.gpus.map((gpu) => (
              <p key={gpu.name} className="mt-2 font-mono text-[11px] text-mute">
                {gpu.name} · {gb(gpu.vramAllocatedGb)} / {gb(gpu.vramTotalGb)} GB · {gpu.utilization.toFixed(0)}%
                {node.unifiedMemoryGb ? ` · unified ${gb(node.unifiedMemoryGb)} GB` : ""}
              </p>
            ))}
            {node.gpus.length === 0 && <p className="mt-2 text-[12px] text-faint">No GPU telemetry on this node yet.</p>}
            <div className="mt-3 flex flex-wrap gap-1">
              <Mini
                label="Drop"
                onClick={() => void setNodeStatus(node.id, "offline").then((snap) => useSwarm.getState().applySnapshot(snap))}
              />
              <Mini
                label="Sleep"
                onClick={() => void setNodeStatus(node.id, "sleeping").then((snap) => useSwarm.getState().applySnapshot(snap))}
              />
              <Mini
                label="Restore"
                onClick={() => void setNodeStatus(node.id, "online").then((snap) => useSwarm.getState().applySnapshot(snap))}
              />
              <Mini
                label="Exit node"
                onClick={() =>
                  void applyExitNode(node.id, true)
                    .then((snap) => useSwarm.getState().applySnapshot(snap))
                    .catch((error: Error) => useSwarm.getState().pushToast(error.message, "warn"))
                }
              />
            </div>
          </section>
        )}

        {model && fit && (
          <section className="mb-4 border-t border-line pt-3">
            <h2 className="text-[13px]">{model.name}</h2>
            <p className="mt-1 font-mono text-[11px] text-mute">
              weights {gb(fit.weightsGb)} · KV {gb(fit.kvGb)} · scratch {gb(fit.scratchGb)} · pool {gb(fit.pooledGb)}
            </p>
            <div className="mt-2 flex h-3 overflow-hidden rounded bg-canvas">
              {fit.slices.map((slice) => (
                <div
                  key={`${slice.nodeId}-${slice.kind}`}
                  title={`${slice.hostname} ${slice.layers} layers ${gb(slice.vramGb)} GB`}
                  className="h-full"
                  style={{
                    width: `${Math.max(2, (slice.vramGb / Math.max(0.1, fit.totalGb)) * 100)}%`,
                    background: slice.kind === "unplaced" ? "#F07178" : slice.kind === "cpu" ? "#5C6678" : snapshot.nodes.find((item) => item.id === slice.nodeId)?.color ?? "#8AB4FF",
                  }}
                />
              ))}
            </div>
            <ul className="mt-2 space-y-1 font-mono text-[10px] text-mute">
              {fit.slices.map((slice) => (
                <li key={`${slice.kind}-${slice.nodeId}`}>
                  {slice.hostname} · {slice.kind} · {slice.layers}L · {gb(slice.vramGb)} GB
                </li>
              ))}
            </ul>
            {fit.warnings.map((warning) => (
              <p key={warning} className="mt-1 text-[12px] text-warn">
                {warning}
              </p>
            ))}
            <p className={`mt-2 text-[12px] ${fit.fits ? "text-mint" : "text-danger"}`}>{fit.fits ? "Fits the current pool." : "Does not fit."}</p>
            <Mini
              label="Commit shard plan"
              onClick={() =>
                void commitShard(model.id)
                  .then((snap) => useSwarm.getState().applySnapshot(snap))
                  .catch((error: Error) => useSwarm.getState().pushToast(error.message, "warn"))
              }
            />
          </section>
        )}

        <section className="border-t border-line pt-3">
          <h2 className="text-[11px] uppercase tracking-[0.14em] text-faint">Decode</h2>
          <label className="mt-2 block text-[12px] text-mute">
            Context {params.contextLength}
            <input className="mt-1 w-full accent-[#8AB4FF]" type="range" min={2048} max={131072} step={1024} value={params.contextLength} onChange={(event) => patch({ contextLength: Number(event.target.value) })} />
          </label>
          <label className="mt-2 block text-[12px] text-mute">
            KV cache
            <select className="mt-1 w-full rounded border border-line bg-canvas px-2 py-1" value={params.kvQuant} onChange={(event) => patch({ kvQuant: event.target.value })}>
              <option value="fp16">fp16</option>
              <option value="q8">q8</option>
              <option value="q4">q4</option>
            </select>
          </label>
          <label className="mt-2 flex items-center gap-2 text-[12px] text-mute">
            <input type="checkbox" checked={params.flashAttention} onChange={(event) => patch({ flashAttention: event.target.checked })} />
            Flash attention
          </label>
          <label className="mt-2 block text-[12px] text-mute">
            GPU offload layers {params.gpuOffloadLayers}
            <input className="mt-1 w-full accent-[#8AB4FF]" type="range" min={0} max={256} value={params.gpuOffloadLayers} onChange={(event) => patch({ gpuOffloadLayers: Number(event.target.value) })} />
          </label>
          <label className="mt-2 flex items-center gap-2 text-[12px] text-mute">
            <input type="checkbox" checked={params.speculative} onChange={(event) => patch({ speculative: event.target.checked })} />
            Speculative decode over the mesh
          </label>
          <label className="mt-2 flex items-center gap-2 text-[12px] text-mute">
            <input type="checkbox" checked={params.evictForFit} onChange={(event) => patch({ evictForFit: event.target.checked })} />
            Plan against total VRAM
          </label>
          <label className="mt-2 block text-[12px] text-mute">
            Draft node
            <select className="mt-1 w-full rounded border border-line bg-canvas px-2 py-1" value={params.draftNodeId ?? ""} onChange={(event) => patch({ draftNodeId: event.target.value || null })}>
              <option value="">none</option>
              {snapshot.nodes.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.hostname}
                </option>
              ))}
            </select>
          </label>
        </section>

        <section className="mt-4 border-t border-line pt-3">
          <h2 className="text-[11px] uppercase tracking-[0.14em] text-faint">MCP tools</h2>
          <p className="mt-1 text-[12px] leading-snug text-mute">Shell and file tools stay disarmed until you set a project root. The bearer token is only shown here.</p>
          <label className="mt-2 block text-[12px] text-mute">
            Project root
            <input
              className="mt-1 w-full rounded border border-line bg-canvas px-2 py-1 font-mono text-[11px]"
              value={snapshot.toolRoot}
              onChange={(event) => void setToolPolicy(event.target.value, snapshot.shellArmed, snapshot.shardBackend).then((snap) => useSwarm.getState().applySnapshot(snap))}
            />
          </label>
          <label className="mt-2 block text-[12px] text-mute">
            Shard backend URL
            <input
              className="mt-1 w-full rounded border border-line bg-canvas px-2 py-1 font-mono text-[11px]"
              placeholder="http://100.x.x.x:8080"
              value={snapshot.shardBackend}
              onChange={(event) =>
                void setToolPolicy(snapshot.toolRoot, snapshot.shellArmed, event.target.value)
                  .then((snap) => useSwarm.getState().applySnapshot(snap))
                  .catch((error: Error) => useSwarm.getState().pushToast(error.message, "warn"))
              }
            />
          </label>
          <label className="mt-2 flex items-center gap-2 text-[12px] text-mute">
            <input
              type="checkbox"
              checked={snapshot.shellArmed}
              onChange={(event) => void setToolPolicy(snapshot.toolRoot, event.target.checked, snapshot.shardBackend).then((snap) => useSwarm.getState().applySnapshot(snap))}
            />
            Arm local shell
          </label>
          <p className="mt-2 break-all font-mono text-[10px] text-faint">Bearer {snapshot.gatewayToken}</p>
          <p className="mt-2 font-mono text-[10px] text-faint">tools: swarm_status, files_read, files_write, shell_exec, vector_query, web_search</p>
        </section>
        {node && node.gpus.length > 0 && <p className="mt-3 font-mono text-[10px] text-faint">free {gb(Math.max(0, vramOf(node).total - vramOf(node).allocated))} GB on this node</p>}
      </div>
    </aside>
  );
}

function Mini({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button className="mr-1 mt-2 rounded border border-line px-2 py-0.5 text-[11px] text-mute hover:text-ink" onClick={onClick}>
      {label}
    </button>
  );
}
