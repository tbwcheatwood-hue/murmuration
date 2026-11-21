import { useEffect } from "react";
import { ActivityBar } from "./components/ActivityBar";
import { BottomDock } from "./components/BottomDock";
import { CenterCanvas } from "./components/CenterCanvas";
import { CommandPalette } from "./components/CommandPalette";
import { Inspector } from "./components/Inspector";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { getSnapshot, inTauri, setInferenceActive } from "./lib/actions";
import { startBrowserMesh } from "./lib/mesh";
import { runObjective } from "./lib/run";
import { useSwarm } from "./store";
import type { Snapshot } from "./types";

export default function App() {
  const sidebarOpen = useSwarm((state) => state.sidebarOpen);
  const inspectorOpen = useSwarm((state) => state.inspectorOpen);
  const bottomOpen = useSwarm((state) => state.bottomOpen);
  const toasts = useSwarm((state) => state.toasts);

  useEffect(() => {
    let stop = () => {};
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    if (inTauri()) {
      void getSnapshot().then((snapshot) => {
        if (!cancelled) useSwarm.getState().applySnapshot(snapshot);
      });
      void import("@tauri-apps/api/event").then(async ({ listen }) => {
        const fn = await listen<Snapshot>("swarm-tick", (event) => {
          useSwarm.getState().applySnapshot(event.payload);
        });
        if (cancelled) fn();
        else unlisten = fn;
      });
    } else {
      stop = startBrowserMesh((snapshot) => useSwarm.getState().applySnapshot(snapshot));
    }
    return () => {
      cancelled = true;
      stop();
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod) return;
      const key = event.key.toLowerCase();
      const ui = useSwarm.getState();
      if (key === "k") {
        event.preventDefault();
        ui.setPalette(!ui.paletteOpen);
      } else if (key === "b") {
        event.preventDefault();
        ui.toggleSidebar();
      } else if (key === "j") {
        event.preventDefault();
        ui.toggleBottom();
      } else if (key === "1") {
        ui.setTab("topology");
      } else if (key === "2") {
        ui.setTab("workbench");
      } else if (key === "3") {
        ui.setTab("traffic");
      } else if (key === "4") {
        ui.setTab("chat");
      } else if (key === "enter" && ui.tab === "workbench") {
        event.preventDefault();
        void runObjective();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    const timers = toasts.map((toast) => window.setTimeout(() => useSwarm.getState().dismissToast(toast.id), 4200));
    return () => timers.forEach((timer) => window.clearTimeout(timer));
  }, [toasts]);

  return (
    <div className="flex h-screen flex-col bg-canvas text-ink">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <ActivityBar />
        {sidebarOpen && <Sidebar />}
        <CenterCanvas />
        {inspectorOpen && <Inspector />}
      </div>
      <BottomDock collapsed={!bottomOpen} />
      <CommandPalette />
      <div className="pointer-events-none fixed bottom-16 right-4 z-40 flex w-80 flex-col gap-2">
        {toasts.map((toast) => (
          <button
            key={toast.id}
            className={`pointer-events-auto border px-3 py-2 text-left text-xs ${
              toast.tone === "danger" ? "border-danger/50 text-danger" : toast.tone === "warn" ? "border-warn/50 text-warn" : "border-line text-ink"
            } bg-panel`}
            onClick={() => {
              useSwarm.getState().dismissToast(toast.id);
              if (toast.text.includes("inference")) void setInferenceActive(false);
            }}
          >
            {toast.text}
          </button>
        ))}
      </div>
    </div>
  );
}
