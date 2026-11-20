export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

export function gb(value: number): string {
  return value.toFixed(1);
}

export function vramOf(node: { gpus: { vramTotalGb: number; vramAllocatedGb: number }[] }): { total: number; allocated: number } {
  return {
    total: node.gpus.reduce((sum, gpu) => sum + gpu.vramTotalGb, 0),
    allocated: node.gpus.reduce((sum, gpu) => sum + gpu.vramAllocatedGb, 0),
  };
}

export function poolOf(nodes: { gpus: { vramTotalGb: number; vramAllocatedGb: number }[] }[]): { total: number; allocated: number; free: number } {
  const total = nodes.reduce((sum, node) => sum + vramOf(node).total, 0);
  const allocated = nodes.reduce((sum, node) => sum + vramOf(node).allocated, 0);
  return { total, allocated, free: Math.max(0, total - allocated) };
}

export function hostName(nodes: { id: string; hostname: string }[], id: string): string {
  return nodes.find((node) => node.id === id)?.hostname ?? id;
}

export function nowHms(): string {
  const date = new Date();
  const pad = (value: number, size = 2) => String(value).padStart(size, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`;
}

export function nowIso(): string {
  return new Date().toISOString();
}

export function clip(value: string, max: number): string {
  return value.length > max ? `${value.slice(0, max)}…` : value;
}

let seq = 1;
export function uid(prefix: string): string {
  seq += 1;
  return `${prefix}-${seq}`;
}
