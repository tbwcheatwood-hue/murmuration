import { useSwarm } from "../store";
import { cx, gb, poolOf } from "../lib/format";

export function BottomDock({ collapsed }: { collapsed: boolean }) {
  const snapshot = useSwarm((state) => state.snapshot);
  const logNode = useSwarm((state) => state.logNode);
  const logSource = useSwarm((state) => state.logSource);
  const nodes = snapshot.nodes.filter((node) => !node.simulated);
  const pool = poolOf(nodes);
  const sources = ["all", "lms", "llmster", "tailscale", "gateway", "orchestrator", "telemetry", "mcp"];
  const lines = snapshot.logs.filter((line) => (logNode === "all" || line.nodeId === logNode) && (logSource === "all" || line.source === logSource));
  return (
    <footer className="shrink-0 border-t border-line bg-panel">
      <VramBar total={pool.total} allocated={pool.allocated} free={pool.free} segments={nodes.map((node) => ({ id: node.id, color: node.color, value: node.gpus.reduce((sum, gpu) => sum + gpu.vramAllocatedGb, 0), label: node.hostname }))} />
      {!collapsed && (
        <>
          <div className="flex h-8 items-center gap-1 overflow-auto border-b border-line px-2">
            <Filter label="all nodes" active={logNode === "all"} onClick={() => useSwarm.getState().setLogNode("all")} />
            {nodes.map((node) => (
              <Filter key={node.id} label={node.hostname} active={logNode === node.id} onClick={() => useSwarm.getState().setLogNode(node.id)} />
            ))}
            <span className="mx-1 h-4 w-px bg-line" />
            {sources.map((source) => (
              <Filter key={source} label={source} active={logSource === source} onClick={() => useSwarm.getState().setLogSource(source)} />
            ))}
          </div>
          <div className="h-36 overflow-auto px-3 py-1.5 font-mono text-[11px] leading-5">
            {lines.map((line) => (
              <div key={line.id} className={cx(line.level === "warn" ? "text-warn" : line.level === "error" ? "text-danger" : "text-mute")}>
                <span className="text-faint">{line.ts}</span> {line.nodeId.padEnd(8, " ")} {line.source.padEnd(12, " ")} {line.message}
              </div>
            ))}
          </div>
        </>
      )}
      <div className="flex h-7 items-center gap-3 border-t border-line px-3 font-mono text-[10px] text-faint">
        <span>{nodes.filter((node) => node.status === "online").length} online</span>
        <span>{snapshot.meshSource}</span>
        <span>{snapshot.mode}</span>
        <span className="ml-auto">Ctrl+B sidebar · Ctrl+J logs · Ctrl+1/2/3 views</span>
      </div>
    </footer>
  );
}

function VramBar({
  total,
  allocated,
  free,
  segments,
}: {
  total: number;
  allocated: number;
  free: number;
  segments: { id: string; color: string; value: number; label: string }[];
}) {
  return (
    <div className="px-3 py-2">
      <div className="mb-1 flex items-baseline gap-3 font-mono text-[11px]">
        <span className="text-ink">Swarm VRAM {gb(total)} GB</span>
        <span className="text-mute">Allocated {gb(allocated)}</span>
        <span className="text-faint">Free {gb(free)}</span>
      </div>
      <div className="flex h-2 overflow-hidden rounded bg-canvas">
        {segments.map((segment) => (
          <div key={segment.id} title={`${segment.label} ${gb(segment.value)} GB`} style={{ width: total === 0 ? 0 : `${(segment.value / total) * 100}%`, background: segment.color }} />
        ))}
      </div>
    </div>
  );
}

function Filter({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button className={cx("rounded px-1.5 py-0.5 font-mono text-[10px]", active ? "bg-panel-2 text-ink" : "text-faint hover:text-mute")} onClick={onClick}>
      {label}
    </button>
  );
}
