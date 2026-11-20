import { useEffect, useState } from "react";
import { builtins } from "../extensions/registry";
import { loadProbeWasm } from "../extensions/wasmHost";
import { useSwarm } from "../store";
import { cx } from "../lib/format";

export function ExtensionManager() {
  const enabled = useSwarm((state) => state.enabledExtensions);
  const toggle = useSwarm((state) => state.toggleExtension);
  const [wasmName, setWasmName] = useState("loading wasm probe…");
  useEffect(() => {
    void loadProbeWasm().then(setWasmName).catch(() => setWasmName("wasm probe failed"));
  }, []);
  return (
    <div className="flex flex-col gap-3 p-3">
      <p className="text-[12px] leading-relaxed text-mute">
        SwarmPlugin extensions run in the studio. The host can also instantiate a WASM module that exports <span className="font-mono text-ink">name_ptr</span> and <span className="font-mono text-ink">name_len</span>.
      </p>
      <div className="rounded border border-line bg-canvas px-2 py-1.5 font-mono text-[10px] text-mint">WASM probe · {wasmName}</div>
      {builtins.map((plugin) => {
        const on = enabled[plugin.id] !== false;
        const Panel = plugin.Panel;
        return (
          <section key={plugin.id} className="rounded-md border border-line bg-panel-2">
            <div className="flex items-start gap-2 px-2.5 py-2">
              <div className="min-w-0">
                <div className="text-[13px]">{plugin.name}</div>
                <div className="font-mono text-[10px] text-faint">
                  {plugin.version} · {plugin.permissions.join(", ")}
                </div>
                <p className="mt-1 text-[12px] leading-snug text-mute">{plugin.description}</p>
              </div>
              <button className={cx("ml-auto shrink-0 rounded border px-2 py-0.5 text-[11px]", on ? "border-mint/40 text-mint" : "border-line text-faint")} onClick={() => toggle(plugin.id)}>
                {on ? "On" : "Off"}
              </button>
            </div>
            {on && Panel && (
              <div className="border-t border-line px-2.5 py-2">
                <Panel />
              </div>
            )}
          </section>
        );
      })}
    </div>
  );
}
