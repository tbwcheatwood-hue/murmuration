import { convertFlowise, detectFlowise } from "./flowise";
import { convertLangflow, detectLangflow } from "./langflow";
import { convertN8n, detectN8n } from "./n8n";
import type { ConversionLine, ImportConversionResult, WorkflowPlatform } from "./types";

export type ConverterId = "langflow" | "flowise" | "n8n";

const converters: { id: ConverterId; detect: (raw: unknown) => boolean; convert: (raw: unknown) => ImportConversionResult }[] = [
  { id: "langflow", detect: detectLangflow, convert: convertLangflow },
  { id: "flowise", detect: detectFlowise, convert: convertFlowise },
  { id: "n8n", detect: detectN8n, convert: convertN8n },
];

export function detectWorkflowSource(raw: unknown): WorkflowPlatform {
  for (const item of converters) if (item.detect(raw)) return item.id;
  return "unknown";
}

export function convertWorkflow(raw: unknown, source: "auto" | ConverterId = "auto"): ImportConversionResult {
  if (source !== "auto") {
    const pick = converters.find((item) => item.id === source);
    if (!pick) return { ok: false, error: `Unknown source ${source}`, name: "", summary: [] as ConversionLine[], detectedSource: "unknown" };
    return pick.convert(raw);
  }
  for (const item of converters) if (item.detect(raw)) return item.convert(raw);
  return { ok: false, error: "Unrecognized workflow JSON. Supported: Langflow, Flowise, n8n exports.", name: "", summary: [] as ConversionLine[], detectedSource: "unknown" };
}

const MAX_BYTES = 2_000_000;

export function parseWorkflowText(text: string, source: "auto" | ConverterId = "auto"): ImportConversionResult {
  const trimmed = text.trim();
  if (!trimmed) return { ok: false, error: "Empty file.", name: "", summary: [] as ConversionLine[], detectedSource: "unknown" };
  if (new TextEncoder().encode(trimmed).length > MAX_BYTES) return { ok: false, error: "File exceeds 2 MB limit.", name: "", summary: [] as ConversionLine[], detectedSource: "unknown" };
  let json: unknown;
  try {
    json = JSON.parse(trimmed) as unknown;
  } catch {
    return { ok: false, error: "Invalid JSON. Export a .json flow from Langflow, Flowise, or n8n.", name: "", summary: [] as ConversionLine[], detectedSource: "unknown" };
  }
  if (!json || typeof json !== "object") return { ok: false, error: "JSON root must be an object.", name: "", summary: [] as ConversionLine[], detectedSource: "unknown" };
  return convertWorkflow(json, source);
}
