import { useEffect, useState } from "react";
import { setInferenceActive } from "../lib/actions";
import { runObjective } from "../lib/run";
import { useSwarm } from "../store";
import { cx } from "../lib/format";

export function Workbench() {
  const objective = useSwarm((state) => state.objective);
  const plan = useSwarm((state) => state.plan);
  const running = useSwarm((state) => state.running);
  const artifact = useSwarm((state) => state.artifact);
  const [shown, setShown] = useState<Record<string, string>>({});

  useEffect(() => {
    if (!plan || !running) return;
    const cursors = new Map<string, number>();
    const timer = window.setInterval(() => {
      let done = true;
      const next: Record<string, string> = {};
      for (const step of plan.steps) {
        const cursor = Math.min(step.output.length, (cursors.get(step.id) ?? 0) + 4);
        cursors.set(step.id, cursor);
        next[step.id] = step.output.slice(0, cursor);
        if (cursor < step.output.length) done = false;
      }
      setShown(next);
      if (done) {
        window.clearInterval(timer);
        useSwarm.getState().setArtifact(plan.artifact);
        useSwarm.getState().setRunning(false);
        void setInferenceActive(false).then((snapshot) => useSwarm.getState().applySnapshot(snapshot));
      }
    }, 24);
    return () => window.clearInterval(timer);
  }, [plan, running]);

  return (
    <div className="grid h-full min-h-0 grid-rows-[minmax(0,1fr)_190px]">
      <div className="grid min-h-0 grid-cols-[minmax(260px,0.9fr)_minmax(0,1.2fr)]">
        <div className="flex min-h-0 flex-col border-r border-line">
          <div className="flex items-center justify-between border-b border-line px-3 py-2 text-[11px] uppercase tracking-[0.14em] text-faint">
            Objective
            <button className="rounded border border-line px-2 py-0.5 font-sans text-[11px] normal-case tracking-normal text-ink" onClick={() => void runObjective()} disabled={running}>
              {running ? "Running" : "Run swarm"}
            </button>
          </div>
          <textarea
            className="min-h-28 flex-1 resize-none bg-transparent px-3 py-2 outline-none"
            value={objective}
            onChange={(event) => useSwarm.getState().setObjective(event.target.value)}
          />
          <div className="border-t border-line px-3 py-2">
            <div className="text-[11px] uppercase tracking-[0.14em] text-faint">Decomposition</div>
            <ul className="mt-2 flex flex-col gap-1">
              {(plan?.steps ?? []).map((step) => {
                const text = shown[step.id] ?? "";
                const state = text.length === 0 ? "queued" : text.length < step.output.length ? "running" : "done";
                return (
                  <li key={step.id} className="font-mono text-[11px] text-mute">
                    <span className={cx(state === "done" ? "text-mint" : state === "running" ? "text-warn" : "text-faint")}>{state}</span>
                    {"  "}
                    {step.role} · {step.hostname} · {step.model}
                    {step.live ? "" : " · brief"}
                  </li>
                );
              })}
              {!plan && <li className="text-[12px] text-faint">Run the objective to fan it across the assigned roles.</li>}
            </ul>
            {plan?.warnings.map((warning) => (
              <p key={warning} className="mt-2 text-[12px] text-warn">
                {warning}
              </p>
            ))}
          </div>
        </div>
        <div className="grid min-h-0 auto-rows-fr grid-cols-1 overflow-auto xl:grid-cols-3">
          {(plan?.steps ?? []).map((step) => (
            <article key={step.id} className="flex min-h-0 flex-col border-b border-line xl:border-r xl:border-b-0">
              <header className="border-b border-line px-3 py-2">
                <div className="text-[12px]">{step.role}</div>
                <div className="font-mono text-[10px] text-faint">
                  {step.hostname} · {step.model}
                  {step.live ? " · live model" : " · orchestrator brief"}
                </div>
              </header>
              <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap px-3 py-2 font-mono text-[11px] leading-relaxed text-ink">{shown[step.id] ?? ""}</pre>
            </article>
          ))}
        </div>
      </div>
      <div className="flex min-h-0 flex-col border-t border-line">
        <div className="flex h-8 items-center border-b border-line px-3 text-[11px] uppercase tracking-[0.14em] text-faint">Artifact</div>
        <textarea
          className="min-h-0 flex-1 resize-none bg-transparent px-3 py-2 font-mono text-[11px] leading-relaxed outline-none"
          value={artifact}
          onChange={(event) => useSwarm.getState().setArtifact(event.target.value)}
          placeholder="The swarm writes the artifact here when the workers finish."
        />
      </div>
    </div>
  );
}
