import { planObjective, setInferenceActive } from "./actions";
import { useSwarm } from "../store";

export async function runObjective(): Promise<void> {
  const ui = useSwarm.getState();
  const objective = ui.objective.trim();
  if (!objective) {
    ui.pushToast("Write an objective first.", "warn");
    return;
  }
  ui.setTab("workbench");
  ui.setRunning(true);
  ui.setArtifact("");
  try {
    ui.applySnapshot(await setInferenceActive(true));
    const plan = await planObjective(objective);
    useSwarm.getState().setPlan(plan);
    const port = useSwarm.getState().snapshot.gatewayPort;
    void fetch(`http://127.0.0.1:${port}/v1/chat/completions`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        model: "murmuration",
        stream: true,
        messages: [{ role: "user", content: objective }],
      }),
    }).catch(() => undefined);
  } catch (error) {
    useSwarm.getState().pushToast(error instanceof Error ? error.message : "The swarm run failed.", "danger");
    useSwarm.getState().setRunning(false);
    const snap = await setInferenceActive(false).catch(() => null);
    if (snap) useSwarm.getState().applySnapshot(snap);
  }
}
