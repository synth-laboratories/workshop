import { useState } from "react";
import { bridges } from "../../runtime/desktopBridge";

/** Commands stay on the public service; no local checkpoint registry. */
export function ContainerExperimentControls({ runId }: { runId: string }) {
  const [checkpoint, setCheckpoint] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<unknown>(null);
  const act = async (action: "recover" | "start" | "verify_checkpoint") => {
    setBusy(true);
    try { setResult(await bridges.optimizers?.containerExperimentAction(runId, action, checkpoint)); }
    catch (error) { setResult({ error: String(error) }); }
    finally { setBusy(false); }
  };
  return <section aria-label="Container experiment recovery">
    <h3>Experiment recovery and checkpoint verification</h3>
    <p>Recovery reconciles a completed phase receipt. It never repeats uncertain training or a partially observed panel. Continue admits only remaining phases under the original cap.</p>
    <button disabled={busy} onClick={() => void act("recover")}>Reconcile completed phase</button>
    <button disabled={busy} onClick={() => void act("start")}>Continue remaining phases</button>
    <label>Immutable checkpoint ID<input value={checkpoint} onChange={event => setCheckpoint(event.target.value)} /></label>
    <button disabled={busy || !checkpoint} onClick={() => void act("verify_checkpoint")}>Verify provider artifacts</button>
    {result != null && <details open><summary>Backend result</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>}
  </section>;
}
