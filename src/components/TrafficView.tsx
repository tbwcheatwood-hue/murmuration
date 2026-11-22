import { useState } from "react";
import { useSwarm } from "../store";
import { cx } from "../lib/format";

function upstreamLabel(value: string) {
  if (value === "live") return "Live model";
  if (value === "brief") return "Plan";
  return "Upstream error";
}

function upstreamSentence(value: string) {
  if (value === "live") return "Live model reply";
  if (value === "brief") return "Orchestrator plan, not a model reply";
  return "Upstream error, not a model reply";
}

export function TrafficView() {
  const traffic = useSwarm((state) => state.snapshot.traffic);
  const port = useSwarm((state) => state.snapshot.gatewayPort);
  const [selected, setSelected] = useState<string | null>(null);
  const newest = [...traffic].reverse();
  const current = newest.find((entry) => entry.id === selected) ?? newest[0];
  return (
    <div className="grid h-full grid-cols-[minmax(320px,0.9fr)_minmax(0,1.1fr)]">
      <div className="min-h-0 overflow-auto border-r border-line">
        <div className="sticky top-0 border-b border-line bg-panel px-3 py-2 font-mono text-[10px] text-mute">http://127.0.0.1:{port}/v1</div>
        {newest.length === 0 && <p className="px-3 py-4 text-[12px] text-faint">No requests yet. Run a prompt, or point Cursor at the gateway.</p>}
        {newest.map((entry) => (
          <button
            key={entry.id}
            className={cx("flex w-full items-center gap-2 border-b border-line px-3 py-2 text-left font-mono text-[11px]", current?.id === entry.id ? "bg-white/5" : "hover:bg-white/[0.03]")}
            onClick={() => setSelected(entry.id)}
          >
            <span className="text-accent">{entry.method}</span>
            <span className="truncate text-mute">{entry.path}</span>
            <span className="ml-auto text-faint">{upstreamLabel(entry.upstream)}</span>
            <span className={cx(entry.status < 400 ? "text-mint" : "text-danger")}>{entry.status}</span>
            <span className="w-14 text-right text-faint">{entry.ttftMs} ms</span>
          </button>
        ))}
      </div>
      {current ? (
        <div className="min-h-0 overflow-auto px-4 py-3">
          <div className="font-mono text-[11px] text-mute">
            {current.ts} · {current.routedTo} · {upstreamSentence(current.upstream)} · TTFT {current.ttftMs} ms
          </div>
          <h3 className="mt-3 text-[11px] uppercase tracking-[0.14em] text-faint">Decision</h3>
          <p className="mt-1 text-[13px] leading-relaxed">{current.decision}</p>
          <h3 className="mt-4 text-[11px] uppercase tracking-[0.14em] text-faint">Request</h3>
          <pre className="mt-1 whitespace-pre-wrap font-mono text-[11px] text-mute">{current.requestPreview}</pre>
          <h3 className="mt-4 text-[11px] uppercase tracking-[0.14em] text-faint">Response</h3>
          <pre className="mt-1 whitespace-pre-wrap font-mono text-[11px] text-mute">{current.responsePreview}</pre>
        </div>
      ) : (
        <div />
      )}
    </div>
  );
}
