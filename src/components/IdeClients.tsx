import { useEffect, useState } from "react";
import { applyIdeClient, ideClientStatus, type IdeClient } from "../lib/actions";
import { useSwarm } from "../store";

export function IdeClients() {
  const port = useSwarm((state) => state.snapshot.gatewayPort);
  const [clients, setClients] = useState<IdeClient[]>([]);
  const [busy, setBusy] = useState("");

  useEffect(() => {
    void ideClientStatus()
      .then(setClients)
      .catch((err: unknown) => useSwarm.getState().pushToast(err instanceof Error ? err.message : String(err), "warn"));
  }, [port]);

  async function apply(id: string) {
    setBusy(id);
    try {
      const next = await applyIdeClient(id);
      setClients((current) => current.map((item) => (item.id === next.id ? next : item)));
      useSwarm.getState().pushToast(next.message);
    } catch (err) {
      useSwarm.getState().pushToast(err instanceof Error ? err.message : String(err), "warn");
    } finally {
      setBusy("");
    }
  }

  const listed = clients.length
    ? clients
    : [
        { id: "cursor", name: "Cursor", configured: false, baseUrl: `http://127.0.0.1:${port}/v1`, model: "murmuration", path: "", message: "Checking Cursor…" },
        { id: "zcode", name: "Zcode", configured: false, baseUrl: `http://127.0.0.1:${port}/v1`, model: "murmuration", path: "", message: "Checking Zcode…" },
      ];

  return (
    <div className="flex flex-col gap-3 p-3">
      <p className="text-[12px] leading-5 text-mute">
        Cursor and Zcode call the Murmuration gateway. LM Studio still loads and serves the model. The model id is murmuration.
      </p>
      {listed.map((client) => (
        <section key={client.id} className="border border-line bg-canvas p-3">
          <div className="flex items-center gap-2">
            <h2 className="text-sm">{client.name}</h2>
            <span className={`ml-auto font-mono text-[10px] ${client.configured ? "text-mint" : "text-faint"}`}>{client.configured ? "pointed" : "open"}</span>
          </div>
          <p className="mt-2 font-mono text-[10px] text-faint">{client.baseUrl}</p>
          <p className="mt-2 text-[12px] leading-5 text-mute">{client.message}</p>
          {client.id === "zcode" && <p className="mt-2 text-[11px] leading-4 text-faint">If Zcode is already open, restart it so the model list picks up Murmuration.</p>}
          <button className="mt-3 border border-line px-2 py-1 text-[12px] text-ink" disabled={busy === client.id} onClick={() => void apply(client.id)}>
            {busy === client.id ? "Writing…" : `Point ${client.name} here`}
          </button>
        </section>
      ))}
    </div>
  );
}
