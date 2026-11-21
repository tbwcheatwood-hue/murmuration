import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { fetchFlowWorkflow, inTauri, listFlowCatalog, runCanvasTool } from "../lib/actions";
import { bundledFlows, bundledN8n, type BundledFlow } from "../lib/tim/bundled";
import { convertWorkflow, parseWorkflowText } from "../lib/tim/convert";
import { TOOL_CATALOG, createNode } from "../lib/tim/graph";
import type { CatalogHit, GraphNode, ImportConversionResult, ToolServer } from "../lib/tim/types";
import { ChatNodes } from "./ChatNodes";
import { useSwarm } from "../store";

type Selection =
  | { kind: "bundled"; flow: BundledFlow }
  | { kind: "remote"; hit: CatalogHit; text?: string; error?: string };

type StudioState = {
  remote: CatalogHit[];
  selection: Selection | null;
  notice: string;
};

let studio: StudioState = { remote: [], selection: null, notice: "" };
const listeners = new Set<() => void>();

function publish(next: StudioState) {
  studio = next;
  listeners.forEach((listener) => listener());
}

function useStudio(): StudioState {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => studio,
  );
}

export function FlowCatalog() {
  const { remote, selection, notice } = useStudio();
  const [query, setQuery] = useState("");
  const [platform, setPlatform] = useState<"all" | "langflow" | "flowise" | "n8n">("all");
  const [busy, setBusy] = useState(false);
  const bundled = useMemo(() => bundledFlows(), []);
  const localN8n = useMemo(() => bundledN8n(), []);
  const needle = query.trim().toLowerCase();
  const visibleBundled = bundled.filter((flow) => (platform === "all" || flow.platform === platform) && flow.name.toLowerCase().includes(needle));
  const remoteMatches = [...localN8n, ...remote.filter((hit) => !localN8n.some((item) => item.rawUrl === hit.rawUrl))].filter(
    (hit) => (platform === "all" || hit.platform === platform) && hit.name.toLowerCase().includes(needle),
  );
  const visibleRemote = remoteMatches.slice(0, 80);

  async function refresh() {
    if (!inTauri()) {
      publish({ ...studio, notice: "The desktop app can refresh these lists from GitHub. Bundled Langflow and Flowise files are already on disk." });
      return;
    }
    setBusy(true);
    const next: CatalogHit[] = [];
    const problems: string[] = [];
    for (const name of ["langflow", "flowise", "n8n"] as const) {
      try {
        next.push(...(await listFlowCatalog(name)));
      } catch (err) {
        problems.push(`${name}: ${err instanceof Error ? err.message : String(err)}`);
      }
    }
    publish({ ...studio, remote: next, notice: problems.length ? problems.join(" ") : `Pulled ${next.length} catalog entries.` });
    setBusy(false);
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex gap-1 border-b border-line p-2">
        {(["all", "langflow", "flowise", "n8n"] as const).map((item) => (
          <button key={item} className={`px-2 py-1 text-[10px] uppercase ${platform === item ? "bg-panel-2 text-ink" : "text-mute"}`} onClick={() => setPlatform(item)}>
            {item}
          </button>
        ))}
      </div>
      <input
        className="mx-2 mt-2 border border-line bg-canvas px-2 py-1 text-xs outline-none"
        placeholder="Search flows"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
      />
        <button className="mx-2 mt-2 border border-line px-2 py-1 text-left text-[11px] text-mute hover:text-ink" onClick={() => void refresh()} disabled={busy}>
        {busy ? "Pulling catalogs…" : "Pull Langflow, Flowise, and n8n"}
      </button>
      <p className="px-3 pt-2 text-[11px] text-faint">Chat input and chat output talk to FOX. LM Studio on port 1234 manages the model.</p>
      {notice && <p className="px-3 py-2 text-[11px] text-mute">{notice}</p>}
      <div className="min-h-0 flex-1 overflow-auto py-1">
        {visibleBundled.map((flow) => (
          <button
            key={flow.key}
            className={`block w-full truncate px-3 py-1 text-left text-xs hover:bg-panel-2 ${selection?.kind === "bundled" && selection.flow.key === flow.key ? "text-ink" : "text-mute"}`}
            onClick={() => publish({ ...studio, selection: { kind: "bundled", flow } })}
          >
            <span className="mr-2 font-mono text-[10px] uppercase text-faint">{flow.platform}</span>
            {flow.name}
          </button>
        ))}
        {visibleRemote.map((hit) => (
          <button
            key={hit.rawUrl}
            className={`block w-full truncate px-3 py-1 text-left text-xs hover:bg-panel-2 ${selection?.kind === "remote" && selection.hit.rawUrl === hit.rawUrl ? "text-ink" : "text-mute"}`}
            onClick={() => publish({ ...studio, selection: { kind: "remote", hit } })}
          >
            <span className="mr-2 font-mono text-[10px] uppercase text-faint">{hit.platform}</span>
            {hit.name}
          </button>
        ))}
        {visibleBundled.length === 0 && visibleRemote.length === 0 && <p className="px-3 py-2 text-xs text-mute">No catalog entries yet.</p>}
        {remoteMatches.length > visibleRemote.length && (
          <p className="px-3 py-2 text-[11px] text-faint">
            Showing {visibleRemote.length} of {remoteMatches.length}. Search to narrow the n8n list.
          </p>
        )}
      </div>
    </div>
  );
}

function useConverted(selection: Selection | null, pasted: string): ImportConversionResult | null {
  return useMemo(() => {
    if (pasted.trim()) return parseWorkflowText(pasted);
    if (!selection) return null;
    if (selection.kind === "bundled") return convertWorkflow(selection.flow.body);
    if (selection.text) return parseWorkflowText(selection.text);
    return null;
  }, [selection, pasted]);
}

export function FlowStudio() {
  const { selection } = useStudio();
  const toolRoot = useSwarm((state) => state.snapshot.toolRoot);
  const [pasted, setPasted] = useState("");
  const [server, setServer] = useState<ToolServer>("bash");
  const [command, setCommand] = useState("ver");
  const [path, setPath] = useState(".");
  const [content, setContent] = useState("");
  const [fileOp, setFileOp] = useState("list");
  const [gitOp, setGitOp] = useState("status");
  const [sql, setSql] = useState("select 1");
  const [query, setQuery] = useState("");
  const [url, setUrl] = useState("https://example.com");
  const [bridge, setBridge] = useState("http://127.0.0.1:1234/mcp");
  const [toolName, setToolName] = useState("");
  const [note, setNote] = useState("");
  const [approved, setApproved] = useState(false);
  const [output, setOutput] = useState("");
  const [running, setRunning] = useState(false);
  const converted = useConverted(selection, pasted);
  const modelNames = useSwarm((state) => state.snapshot.models.map((model) => model.name));
  const [graphNodes, setGraphNodes] = useState<GraphNode[]>(() => [createNode("chat_in", { x: 40, y: 40 }), createNode("chat_out", { x: 40, y: 220 })]);
  const preset = TOOL_CATALOG.find((item) => item.id === server);

  useEffect(() => {
    if (!converted?.ok || !converted.graph) return;
    const imported = converted.graph.nodes;
    const chatIn = imported.find((node) => node.type === "chat_in") ?? createNode("chat_in", { x: 40, y: 40 });
    const chatOut = imported.find((node) => node.type === "chat_out") ?? createNode("chat_out", { x: 40, y: 220 });
    setGraphNodes([chatIn, chatOut, ...imported.filter((node) => node.type !== "chat_in" && node.type !== "chat_out")]);
  }, [converted]);

  useEffect(() => {
    if (!selection || selection.kind !== "remote" || selection.text || selection.error) return;
    let cancelled = false;
    void fetchFlowWorkflow(selection.hit.rawUrl)
      .then((text) => {
        if (!cancelled) publish({ ...studio, selection: { kind: "remote", hit: selection.hit, text } });
      })
      .catch((err) => {
        if (!cancelled) publish({ ...studio, selection: { kind: "remote", hit: selection.hit, error: err instanceof Error ? err.message : String(err) } });
      });
    return () => {
      cancelled = true;
    };
  }, [selection]);

  async function runTool() {
    const needsApproval = Boolean(preset?.approval);
    if (needsApproval && !approved) {
      setOutput(server === "bash" ? "Windows shell needs approval. Confirm the command, then run it again." : "This tool needs approval.");
      return;
    }
    const args: Record<string, unknown> =
      server === "filesystem"
        ? { op: fileOp, path, content }
        : server === "bash"
          ? { command }
          : server === "git"
            ? { op: gitOp }
            : server === "sqlite"
              ? { sql }
              : server === "memory"
                ? { note, limit: 12 }
                : server === "web_search"
                  ? { q: query }
                  : server === "fetch_url"
                    ? { url }
                    : server === "mcp_bridge"
                      ? { tool: toolName, bridge }
                      : {};
    setRunning(true);
    try {
      setOutput(await runCanvasTool(server, args, toolRoot, approved || !needsApproval));
    } catch (err) {
      setOutput(err instanceof Error ? err.message : String(err));
    } finally {
      setRunning(false);
    }
  }

  return (
    <section className="flex min-w-0 flex-1 flex-col bg-canvas">
      <div className="flex h-9 items-center border-b border-line px-3 text-[12px] text-mute">FOX chat, flow import, and canvas tools. LM Studio manages the model.</div>
      <ChatNodes nodes={graphNodes} modelNames={modelNames} onChange={setGraphNodes} />
      <div className="grid min-h-0 flex-1 grid-cols-2">
        <div className="min-h-0 overflow-auto border-r border-line p-3">
          <p className="text-[11px] uppercase tracking-[0.14em] text-faint">Convert</p>
          <p className="mt-2 text-xs text-mute">Langflow, Flowise, and n8n exports become a structural graph. Credentials are dropped. Brain and tool edges are not inferred.</p>
          <textarea
            className="mt-3 h-28 w-full border border-line bg-panel p-2 font-mono text-[11px] outline-none"
            placeholder="Paste a workflow JSON export"
            value={pasted}
            onChange={(event) => setPasted(event.target.value)}
          />
          {selection?.kind === "remote" && !selection.text && !selection.error && <p className="mt-2 text-xs text-mute">Downloading {selection.hit.name}…</p>}
          {selection?.kind === "remote" && selection.error && <p className="mt-2 text-xs text-danger">{selection.error}</p>}
          {converted && (
            <div className="mt-3">
              <p className="text-xs text-ink">
                {converted.ok ? converted.name : converted.error} · {converted.detectedSource}
                {converted.graph ? ` · ${converted.graph.nodes.length} nodes · ${converted.graph.edges.length} edges` : ""}
              </p>
              <ul className="mt-2 max-h-64 space-y-1 overflow-auto">
                {converted.summary.slice(0, 40).map((line, index) => (
                  <li key={`${line.message}-${index}`} className={`text-[11px] ${line.severity === "ok" ? "text-mute" : "text-warn"}`}>
                    {line.message}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
        <div className="min-h-0 overflow-auto p-3">
          <p className="text-[11px] uppercase tracking-[0.14em] text-faint">Nine tools</p>
          <p className="mt-2 text-xs text-mute">Workspace {toolRoot || "is the app tool root"}. The shell tool runs cmd.exe there, with no console window, and stops after 20 seconds.</p>
          <div className="mt-3 flex flex-wrap gap-1">
            {TOOL_CATALOG.map((item) => (
              <button key={item.id} className={`border px-2 py-1 text-[11px] ${server === item.id ? "border-accent text-ink" : "border-line text-mute"}`} onClick={() => setServer(item.id)}>
                {item.label}
              </button>
            ))}
          </div>
          {preset && <p className="mt-2 text-[11px] text-mute">{preset.hint}</p>}
          <div className="mt-3 flex flex-col gap-2">
            {server === "filesystem" && (
              <>
                <select className="border border-line bg-panel px-2 py-1 text-xs" value={fileOp} onChange={(event) => setFileOp(event.target.value)}>
                  <option value="list">list</option>
                  <option value="read">read</option>
                  <option value="write">write</option>
                </select>
                <input className="border border-line bg-panel px-2 py-1 text-xs" value={path} onChange={(event) => setPath(event.target.value)} placeholder="path" />
                {fileOp === "write" && <textarea className="h-20 border border-line bg-panel p-2 text-xs" value={content} onChange={(event) => setContent(event.target.value)} placeholder="content" />}
              </>
            )}
            {server === "bash" && <input className="border border-line bg-panel px-2 py-1 font-mono text-xs" value={command} onChange={(event) => setCommand(event.target.value)} />}
            {server === "git" && (
              <select className="border border-line bg-panel px-2 py-1 text-xs" value={gitOp} onChange={(event) => setGitOp(event.target.value)}>
                <option value="status">status</option>
                <option value="log">log</option>
                <option value="diff">diff</option>
              </select>
            )}
            {server === "sqlite" && <input className="border border-line bg-panel px-2 py-1 font-mono text-xs" value={sql} onChange={(event) => setSql(event.target.value)} />}
            {server === "memory" && <input className="border border-line bg-panel px-2 py-1 text-xs" value={note} onChange={(event) => setNote(event.target.value)} placeholder="note to remember" />}
            {server === "web_search" && <input className="border border-line bg-panel px-2 py-1 text-xs" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="query" />}
            {server === "fetch_url" && <input className="border border-line bg-panel px-2 py-1 text-xs" value={url} onChange={(event) => setUrl(event.target.value)} />}
            {server === "mcp_bridge" && (
              <>
                <input className="border border-line bg-panel px-2 py-1 text-xs" value={bridge} onChange={(event) => setBridge(event.target.value)} placeholder="bridge URL" />
                <input className="border border-line bg-panel px-2 py-1 text-xs" value={toolName} onChange={(event) => setToolName(event.target.value)} placeholder="tool name" />
              </>
            )}
            {preset?.approval && (
              <label className="flex items-center gap-2 text-xs text-mute">
                <input type="checkbox" checked={approved} onChange={(event) => setApproved(event.target.checked)} />
                Approve this run
              </label>
            )}
            <button className="w-fit border border-line px-3 py-1 text-xs text-ink" onClick={() => void runTool()} disabled={running}>
              {running ? "Running…" : "Run"}
            </button>
            {output && <pre className="max-h-64 overflow-auto whitespace-pre-wrap border border-line bg-panel p-2 font-mono text-[11px] text-mute">{output}</pre>}
          </div>
        </div>
      </div>
    </section>
  );
}
