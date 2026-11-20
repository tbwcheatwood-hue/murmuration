import { create } from "zustand";
import type { Activity, FitResult, Snapshot, TaskPlan, WorkspaceTab } from "./types";
import { currentMesh } from "./lib/mesh";

export interface Toast {
  id: string;
  text: string;
  tone: "info" | "warn" | "danger";
}

interface SwarmUI {
  snapshot: Snapshot;
  tab: WorkspaceTab;
  activity: Activity;
  selectedNodeId: string | null;
  selectedModelId: string | null;
  sidebarOpen: boolean;
  inspectorOpen: boolean;
  bottomOpen: boolean;
  paletteOpen: boolean;
  logNode: string;
  logSource: string;
  objective: string;
  plan: TaskPlan | null;
  running: boolean;
  artifact: string;
  fit: FitResult | null;
  enabledExtensions: Record<string, boolean>;
  toasts: Toast[];
  applySnapshot: (snapshot: Snapshot) => void;
  setTab: (tab: WorkspaceTab) => void;
  setActivity: (activity: Activity) => void;
  selectNode: (id: string | null) => void;
  selectModel: (id: string | null) => void;
  toggleSidebar: () => void;
  toggleInspector: () => void;
  toggleBottom: () => void;
  setPalette: (open: boolean) => void;
  setLogNode: (id: string) => void;
  setLogSource: (source: string) => void;
  setObjective: (objective: string) => void;
  setPlan: (plan: TaskPlan | null) => void;
  setRunning: (running: boolean) => void;
  setArtifact: (artifact: string) => void;
  setFit: (fit: FitResult | null) => void;
  toggleExtension: (id: string) => void;
  pushToast: (text: string, tone?: Toast["tone"]) => void;
  dismissToast: (id: string) => void;
}

export const useSwarm = create<SwarmUI>((set) => ({
  snapshot: currentMesh(),
  tab: "topology",
  activity: "nodes",
  selectedNodeId: null,
  selectedModelId: null,
  sidebarOpen: true,
  inspectorOpen: true,
  bottomOpen: true,
  paletteOpen: false,
  logNode: "all",
  logSource: "all",
  objective: "Refactor the auth middleware to use short-lived tokens and keep the session store local.",
  plan: null,
  running: false,
  artifact: "",
  fit: null,
  enabledExtensions: {
    "prometheus-exporter": true,
    "context-compressor": true,
    "git-worktree": true,
  },
  toasts: [],
  applySnapshot: (snapshot) => set({ snapshot }),
  setTab: (tab) => set({ tab }),
  setActivity: (activity) => set({ activity }),
  selectNode: (selectedNodeId) => set({ selectedNodeId }),
  selectModel: (selectedModelId) => set({ selectedModelId }),
  toggleSidebar: () => set((state) => ({ sidebarOpen: !state.sidebarOpen })),
  toggleInspector: () => set((state) => ({ inspectorOpen: !state.inspectorOpen })),
  toggleBottom: () => set((state) => ({ bottomOpen: !state.bottomOpen })),
  setPalette: (paletteOpen) => set({ paletteOpen }),
  setLogNode: (logNode) => set({ logNode }),
  setLogSource: (logSource) => set({ logSource }),
  setObjective: (objective) => set({ objective }),
  setPlan: (plan) => set({ plan }),
  setRunning: (running) => set({ running }),
  setArtifact: (artifact) => set({ artifact }),
  setFit: (fit) => set({ fit }),
  toggleExtension: (id) =>
    set((state) => ({
      enabledExtensions: { ...state.enabledExtensions, [id]: !state.enabledExtensions[id] },
    })),
  pushToast: (text, tone = "info") =>
    set((state) => ({
      toasts: [...state.toasts, { id: `${Date.now()}-${Math.random().toString(36).slice(2, 6)}`, text, tone }].slice(-4),
    })),
  dismissToast: (id) => set((state) => ({ toasts: state.toasts.filter((toast) => toast.id !== id) })),
}));
