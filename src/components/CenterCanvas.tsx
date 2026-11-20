import { useSwarm } from "../store";
import { cx } from "../lib/format";
import { TopologyView } from "./TopologyView";
import { TrafficView } from "./TrafficView";
import { Workbench } from "./Workbench";
import type { WorkspaceTab } from "../types";

const tabs: { id: WorkspaceTab; label: string; key: string }[] = [
  { id: "topology", label: "Topology", key: "1" },
  { id: "workbench", label: "Workbench", key: "2" },
  { id: "traffic", label: "Traffic", key: "3" },
];

export function CenterCanvas() {
  const tab = useSwarm((state) => state.tab);
  return (
    <section className="flex min-w-0 flex-1 flex-col bg-canvas">
      <div className="flex h-9 shrink-0 items-end gap-1 border-b border-line px-2">
        {tabs.map((item) => (
          <button
            key={item.id}
            className={cx("mb-[-1px] border-b px-3 py-1.5 text-[12px]", tab === item.id ? "border-accent text-ink" : "border-transparent text-mute hover:text-ink")}
            onClick={() => useSwarm.getState().setTab(item.id)}
          >
            {item.label}
            <span className="ml-2 font-mono text-[10px] text-faint">{item.key}</span>
          </button>
        ))}
      </div>
      <div className="relative min-h-0 flex-1">
        <div className={cx("absolute inset-0", tab === "topology" ? "" : "hidden")}>
          <TopologyView />
        </div>
        <div className={cx("absolute inset-0", tab === "workbench" ? "" : "hidden")}>
          <Workbench />
        </div>
        <div className={cx("absolute inset-0", tab === "traffic" ? "" : "hidden")}>
          <TrafficView />
        </div>
      </div>
    </section>
  );
}
