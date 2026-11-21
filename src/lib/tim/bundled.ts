import n8nIndex from "../../../catalogs/n8n/index.json";
import type { CatalogHit } from "./types";

export type BundledFlow = {
  key: string;
  platform: "langflow" | "flowise";
  name: string;
  body: unknown;
};

function titleFromPath(path: string): string {
  const file = path.split("/").pop() ?? path;
  return file.replace(/\.json$/i, "").replace(/[-_]/g, " ");
}

function bundled(platform: "langflow" | "flowise", files: Record<string, unknown>): BundledFlow[] {
  return Object.entries(files)
    .map(([path, body]) => ({ key: `${platform}:${path}`, platform, name: titleFromPath(path), body }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

export function bundledFlows(): BundledFlow[] {
  const langflow = import.meta.glob("../../../catalogs/langflow/*.json", { eager: true, import: "default" });
  const flowise = import.meta.glob("../../../catalogs/flowise/*.json", { eager: true, import: "default" });
  return [...bundled("langflow", langflow), ...bundled("flowise", flowise)];
}

export function bundledN8n(): CatalogHit[] {
  return (n8nIndex as CatalogHit[]).map((hit) => ({ ...hit, platform: "n8n" }));
}
