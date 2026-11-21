import { useEffect, useMemo, useState } from "react";
import { activateBlueprint, injectLoad, rebalance, refreshDiscovery, setMode } from "../lib/actions";
import { runObjective } from "../lib/run";
import { useSwarm } from "../store";
import { cx } from "../lib/format";

interface Item {
  id: string;
  title: string;
  hint: string;
  group: string;
  run: () => void;
}

export function CommandPalette() {
  const open = useSwarm((state) => state.paletteOpen);
  const snapshot = useSwarm((state) => state.snapshot);
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);

  const items = useMemo<Item[]>(() => {
    const apply = (work: () => Promise<unknown>) => () => {
      void work()
        .then((result) => {
          if (result && typeof result === "object" && "nodes" in result) useSwarm.getState().applySnapshot(result as typeof snapshot);
        })
        .catch((error: Error) => useSwarm.getState().pushToast(error.message, "warn"));
      useSwarm.getState().setPalette(false);
    };
    return [
      { id: "topology", title: "Show topology", hint: "Ctrl+1", group: "View", run: () => useSwarm.getState().setTab("topology") },
      { id: "workbench", title: "Show workbench", hint: "Ctrl+2", group: "View", run: () => useSwarm.getState().setTab("workbench") },
      { id: "traffic", title: "Show traffic inspector", hint: "Ctrl+3", group: "View", run: () => useSwarm.getState().setTab("traffic") },
      { id: "fox", title: "Show FOX chat", hint: "Ctrl+4", group: "View", run: () => useSwarm.getState().setTab("chat") },
      { id: "run", title: "Run the current objective", hint: "Ctrl+Enter", group: "Swarm", run: () => void runObjective() },
      { id: "rebalance", title: "Rebalance roles by VRAM", hint: "orchestrator", group: "Swarm", run: apply(() => rebalance()) },
      { id: "pipeline", title: "Switch to pipeline routing", hint: "mode", group: "Swarm", run: apply(() => setMode("pipeline")) },
      { id: "shard", title: "Switch to layer sharding", hint: "mode", group: "Swarm", run: apply(() => setMode("shard")) },
      { id: "spec", title: "Switch to speculative decode", hint: "mode", group: "Swarm", run: apply(() => setMode("speculative")) },
      { id: "stress", title: "Inject thermal stress", hint: "telemetry", group: "Mesh", run: apply(() => injectLoad()) },
      { id: "refresh", title: "Refresh LM Link devices", hint: "lms link status", group: "Mesh", run: apply(() => refreshDiscovery()) },
      ...snapshot.blueprints.map((blueprint) => ({
        id: blueprint.id,
        title: `Activate ${blueprint.name}`,
        hint: blueprint.mode,
        group: "Blueprint",
        run: apply(() => activateBlueprint(blueprint.id)),
      })),
    ];
  }, [snapshot]);

  const filtered = items.filter((item) => `${item.title} ${item.hint} ${item.group}`.toLowerCase().includes(query.toLowerCase()));
  const active = filtered[Math.min(cursor, Math.max(0, filtered.length - 1))];

  useEffect(() => {
    if (!open) {
      setQuery("");
      setCursor(0);
    }
  }, [open]);

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/55 pt-[12vh]" onClick={() => useSwarm.getState().setPalette(false)}>
      <div className="w-[560px] overflow-hidden rounded-xl border border-line bg-panel shadow-2xl" onClick={(event) => event.stopPropagation()}>
        <input
          autoFocus
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setCursor(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape") useSwarm.getState().setPalette(false);
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setCursor((value) => Math.min(filtered.length - 1, value + 1));
            }
            if (event.key === "ArrowUp") {
              event.preventDefault();
              setCursor((value) => Math.max(0, value - 1));
            }
            if (event.key === "Enter" && active) {
              active.run();
              useSwarm.getState().setPalette(false);
            }
          }}
          placeholder="Mount, rebalance, switch blueprint…"
          className="w-full border-b border-line bg-transparent px-4 py-3 outline-none"
        />
        <ul className="max-h-80 overflow-auto py-1">
          {filtered.map((item, index) => (
            <li key={item.id}>
              <button className={cx("flex w-full items-center gap-3 px-4 py-2 text-left", index === cursor ? "bg-white/5" : "")} onMouseEnter={() => setCursor(index)} onClick={() => { item.run(); useSwarm.getState().setPalette(false); }}>
                <span className="w-20 font-mono text-[10px] text-faint">{item.group}</span>
                <span className="flex-1">{item.title}</span>
                <span className="font-mono text-[10px] text-faint">{item.hint}</span>
              </button>
            </li>
          ))}
          {filtered.length === 0 && <li className="px-4 py-3 text-mute">No matching command.</li>}
        </ul>
      </div>
    </div>
  );
}
