import type { ReactNode } from "react";
import { compressContext, metricsText } from "../lib/metrics";
import { gitWorktreeApply } from "../lib/actions";
import { useSwarm } from "../store";
import { vramOf } from "../lib/format";

export interface BuiltinPlugin {
  id: string;
  name: string;
  version: string;
  description: string;
  permissions: string[];
  Panel?: () => ReactNode;
}

function PrometheusPanel() {
  const snapshot = useSwarm((state) => state.snapshot);
  const text = metricsText(snapshot);
  return (
    <div className="flex flex-col gap-2">
      <pre className="max-h-40 overflow-auto font-mono text-[10px] leading-relaxed text-mute">{text}</pre>
      <button
        className="self-start rounded border border-line px-2 py-0.5 text-[11px] text-mute hover:text-ink"
        onClick={() => void navigator.clipboard.writeText(text)}
      >
        Copy exposition
      </button>
      <p className="font-mono text-[10px] text-faint">scrape 127.0.0.1:{snapshot.gatewayPort}/metrics when the desktop gateway is up</p>
    </div>
  );
}

function CompressorPanel() {
  const snapshot = useSwarm((state) => state.snapshot);
  const objective = useSwarm((state) => state.objective);
  const smallest = [...snapshot.nodes].filter((node) => node.status === "online" && vramOf(node).total > 0).sort((a, b) => vramOf(a).total - vramOf(b).total)[0];
  const result = compressContext(objective, 48);
  return (
    <div className="flex flex-col gap-1 font-mono text-[10px] text-mute">
      <div>node {smallest?.hostname ?? "none"}</div>
      <div>
        {result.before} → {result.after} tokens
      </div>
      <p className="whitespace-pre-wrap text-ink">{result.summary || "Nothing to compress."}</p>
    </div>
  );
}

function GitPanel() {
  const artifact = useSwarm((state) => state.artifact);
  const root = useSwarm((state) => state.snapshot.toolRoot);
  return (
    <div className="flex flex-col gap-2">
      <p className="text-[11px] text-mute">Applies the workbench artifact as ARTIFACT.md on an isolated branch. Set the project root in the inspector.</p>
      <button
        className="self-start rounded border border-line px-2 py-0.5 text-[11px] text-mute hover:text-ink"
        onClick={() => {
          const repo = root || ".";
          void gitWorktreeApply(repo, "murmuration/swarm-artifact", [{ path: "ARTIFACT.md", contents: artifact || "# empty\n" }], "murmuration: apply swarm artifact", true)
            .then((text) => useSwarm.getState().pushToast(text.split("\n")[0] ?? "Worktree ready.", "info"))
            .catch((error: Error) => useSwarm.getState().pushToast(error.message, "warn"));
        }}
      >
        Apply to worktree
      </button>
    </div>
  );
}

export const builtins: BuiltinPlugin[] = [
  {
    id: "prometheus-exporter",
    name: "Prometheus Telemetry Exporter",
    version: "0.1.0",
    description: "Renders the VRAM pool as a Prometheus text scrape.",
    permissions: ["telemetry", "network"],
    Panel: PrometheusPanel,
  },
  {
    id: "context-compressor",
    name: "Smart Context Compressor",
    version: "0.1.0",
    description: "Extractive summary on the smallest warm node before the architect sees the objective.",
    permissions: ["prompts"],
    Panel: CompressorPanel,
  },
  {
    id: "git-worktree",
    name: "Git Worktree Auto-Committer",
    version: "0.1.0",
    description: "Commits the swarm artifact on an isolated branch after you confirm.",
    permissions: ["filesystem"],
    Panel: GitPanel,
  },
];
