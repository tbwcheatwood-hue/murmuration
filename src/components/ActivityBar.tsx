import { Boxes, Cable, Network, PanelBottom, PanelRight, Puzzle, Spline, Workflow } from "lucide-react";
import { useSwarm } from "../store";
import type { Activity } from "../types";
import { cx } from "../lib/format";

const items: { id: Activity; icon: typeof Network; label: string }[] = [
  { id: "nodes", icon: Network, label: "LM Link" },
  { id: "models", icon: Boxes, label: "Model registry" },
  { id: "blueprints", icon: Workflow, label: "Blueprints" },
  { id: "extensions", icon: Puzzle, label: "Extensions" },
  { id: "flows", icon: Spline, label: "Flows" },
  { id: "clients", icon: Cable, label: "IDE" },
];

export function ActivityBar() {
  const activity = useSwarm((state) => state.activity);
  const setActivity = useSwarm((state) => state.setActivity);
  const sidebarOpen = useSwarm((state) => state.sidebarOpen);
  return (
    <nav className="flex w-12 shrink-0 flex-col items-center gap-1 border-r border-line bg-panel py-2">
      {items.map((item) => {
        const Icon = item.icon;
        const active = sidebarOpen && activity === item.id;
        return (
          <button
            key={item.id}
            title={item.label}
            className={cx("flex h-9 w-9 items-center justify-center rounded-md", active ? "bg-panel-2 text-ink" : "text-mute hover:text-ink")}
            onClick={() => {
              if (activity === item.id && sidebarOpen) useSwarm.getState().toggleSidebar();
              else {
                setActivity(item.id);
                if (item.id === "flows") useSwarm.getState().setTab("chat");
                if (!sidebarOpen) useSwarm.getState().toggleSidebar();
              }
            }}
          >
            <Icon size={16} strokeWidth={1.75} />
          </button>
        );
      })}
      <div className="mt-auto flex flex-col gap-1">
        <button title="Toggle inspector" className="flex h-9 w-9 items-center justify-center text-mute hover:text-ink" onClick={() => useSwarm.getState().toggleInspector()}>
          <PanelRight size={16} strokeWidth={1.75} />
        </button>
        <button title="Toggle logs" className="flex h-9 w-9 items-center justify-center text-mute hover:text-ink" onClick={() => useSwarm.getState().toggleBottom()}>
          <PanelBottom size={16} strokeWidth={1.75} />
        </button>
      </div>
    </nav>
  );
}
