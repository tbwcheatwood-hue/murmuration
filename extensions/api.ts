/** Murmuration SwarmPlugin contract. Builtin plugins are bundled. WASM plugins export name_ptr and name_len. */
export interface SwarmPluginManifest {
  id: string;
  name: string;
  version: string;
  description: string;
  permissions: Array<"telemetry" | "prompts" | "filesystem" | "network">;
  entry: string;
}

export interface WasmPluginAbi {
  memory: WebAssembly.Memory;
  name_ptr(): number;
  name_len(): number;
}
