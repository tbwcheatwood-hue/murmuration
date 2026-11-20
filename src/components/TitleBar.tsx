import { useSwarm } from "../store";
import { setMode } from "../lib/actions";
import type { SwarmMode } from "../types";

const modes: SwarmMode[] = ["pipeline", "shard", "speculative"];

export function TitleBar() {
  const snapshot = useSwarm((state) => state.snapshot);
  const setPalette = useSwarm((state) => state.setPalette);
  return (
    <header className="flex h-11 shrink-0 items-center gap-3 border-b border-line bg-panel px-3">
      <Flock />
      <div className="leading-tight">
        <div className="text-[13px] font-semibold tracking-tight">Murmuration</div>
        <div className="font-mono text-[10px] text-faint">TensorMesh · SwarmLink</div>
      </div>
      <div className="ml-4 flex items-center rounded-md border border-line bg-canvas p-0.5">
        {modes.map((mode) => (
          <button
            key={mode}
            className={`rounded px-2.5 py-1 font-mono text-[11px] ${snapshot.mode === mode ? "bg-panel-2 text-ink" : "text-mute hover:text-ink"}`}
            onClick={() => {
              void setMode(mode).then((next) => useSwarm.getState().applySnapshot(next));
            }}
          >
            {mode}
          </button>
        ))}
      </div>
      {snapshot.failoverNote && <div className="truncate font-mono text-[11px] text-warn">{snapshot.failoverNote}</div>}
      <button className="ml-auto flex items-center gap-2 rounded border border-line bg-canvas px-2 py-1 text-mute hover:text-ink" onClick={() => setPalette(true)}>
        <span className="text-xs">Command</span>
        <kbd className="font-mono text-[10px] text-faint">Ctrl K</kbd>
      </button>
      <div className="font-mono text-[11px] text-mute">
        <span className="mr-2 text-mint">●</span>
        127.0.0.1:{snapshot.gatewayPort}/v1
      </div>
    </header>
  );
}

function Flock() {
  return (
    <svg width="22" height="16" viewBox="0 0 22 16" aria-hidden="true">
      <circle cx="3" cy="11" r="1.4" fill="#8AB4FF" />
      <circle cx="8" cy="7" r="1.5" fill="#7EE0C6" />
      <circle cx="13" cy="4" r="1.6" fill="#D2A8FF" />
      <circle cx="18" cy="8" r="1.4" fill="#E6B35A" />
      <path d="M3 11 C 8 10, 12 6, 18 8" fill="none" stroke="#1E2330" strokeWidth="1" />
    </svg>
  );
}
