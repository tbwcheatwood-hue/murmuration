import type { Snapshot } from "../types";
import { vramOf } from "./format";

function esc(value: string): string {
  return value.replaceAll("\\", "\\\\").replaceAll('"', '\\"');
}

export function metricsText(snapshot: Snapshot): string {
  const lines = [
    "# HELP murmuration_node_up Node can receive work",
    "# TYPE murmuration_node_up gauge",
    "# HELP murmuration_vram_allocated_bytes Allocated VRAM",
    "# TYPE murmuration_vram_allocated_bytes gauge",
    "# HELP murmuration_vram_total_bytes Total VRAM",
    "# TYPE murmuration_vram_total_bytes gauge",
    "# HELP murmuration_gpu_utilization_ratio GPU utilization",
    "# TYPE murmuration_gpu_utilization_ratio gauge",
    "# HELP murmuration_tokens_per_second Decode throughput",
    "# TYPE murmuration_tokens_per_second gauge",
    "# HELP murmuration_ping_milliseconds Tailscale ping",
    "# TYPE murmuration_ping_milliseconds gauge",
  ];
  for (const node of snapshot.nodes) {
    const up = node.status === "online" || node.status === "degraded" ? 1 : 0;
    const { total, allocated } = vramOf(node);
    const name = esc(node.hostname);
    const util = (node.gpus[0]?.utilization ?? 0) / 100;
    lines.push(`murmuration_node_up{node="${name}",relay="${esc(node.relay)}"} ${up}`);
    lines.push(`murmuration_vram_total_bytes{node="${name}"} ${Math.round(total * 1e9)}`);
    lines.push(`murmuration_vram_allocated_bytes{node="${name}"} ${Math.round(allocated * 1e9)}`);
    lines.push(`murmuration_gpu_utilization_ratio{node="${name}"} ${util.toFixed(4)}`);
    lines.push(`murmuration_tokens_per_second{node="${name}"} ${node.tokensPerSec.toFixed(3)}`);
    lines.push(`murmuration_ping_milliseconds{node="${name}"} ${node.pingMs}`);
  }
  return lines.join("\n");
}

export function compressContext(text: string, budgetTokens: number): { summary: string; before: number; after: number } {
  const before = Math.ceil(text.length / 4);
  const sentences = text
    .split(/(?<=[.!?])\s+/)
    .map((sentence) => sentence.trim())
    .filter(Boolean);
  if (sentences.length === 0) {
    return { summary: "", before, after: 0 };
  }
  const counts = new Map<string, number>();
  for (const sentence of sentences) {
    for (const word of sentence.toLowerCase().split(/\W+/)) {
      if (word.length > 3) counts.set(word, (counts.get(word) ?? 0) + 1);
    }
  }
  const ranked = sentences
    .map((sentence, index) => {
      const words = sentence.toLowerCase().split(/\W+/).filter((word) => word.length > 3);
      const rare = words.reduce((sum, word) => sum + 1 / (counts.get(word) ?? 1), 0);
      const recency = index / sentences.length;
      return { sentence, score: rare + recency * 2 };
    })
    .sort((a, b) => b.score - a.score);
  const kept: string[] = [];
  let used = 0;
  for (const item of ranked) {
    const cost = Math.ceil(item.sentence.length / 4);
    if (used + cost > budgetTokens && kept.length > 0) continue;
    kept.push(item.sentence);
    used += cost;
    if (used >= budgetTokens) break;
  }
  const ordered = sentences.filter((sentence) => kept.includes(sentence));
  const summary = ordered.join(" ");
  return { summary, before, after: Math.ceil(summary.length / 4) };
}
