import { useEffect, useRef, useState } from "react";

/** The same review client is served independently and embedded in Workshop.
 * Orchestration and credentials stay in the local service, never in the renderer.
 */
export function EnvironmentQaPage({ onBack, serviceOrigin = "http://127.0.0.1:7338" }: { onBack: () => void; serviceOrigin?: string }) {
  const [connected, setConnected] = useState(false);
  const [pending, setPending] = useState(0);
  const [generation, setGeneration] = useState(0);
  const frame = useRef<HTMLIFrameElement>(null);
  const lastSeen = useRef(0);
  const origin = serviceOrigin;
  const source = `${origin}/?embed=workshop&mode=hitl&parentOrigin=${encodeURIComponent(window.location.origin)}`;
  useEffect(() => {
    const receive = (event: MessageEvent) => {
      // The frame proves its authenticated API works. A successful opaque fetch
      // or iframe load alone cannot establish that the QA service is connected.
      if (event.origin !== origin || event.source !== frame.current?.contentWindow) return;
      const value = event.data;
      if (value?.type !== "workshop-qa:status" || value.version !== 1 ||
          !Number.isSafeInteger(value.pending) || value.pending < 0) return;
      lastSeen.current = Date.now();
      setConnected(true);
      setPending(value.pending);
    };
    window.addEventListener("message", receive);
    const timer = setInterval(() => {
      if (Date.now() - lastSeen.current > 15000) setConnected(false);
    }, 3000);
    return () => { window.removeEventListener("message", receive); clearInterval(timer); };
  }, [origin]);
  return <section className="environment-qa-page" style={{ display: "flex", flexDirection: "column", flex: 1, minWidth: 0, minHeight: 0 }}>
    <div style={{ padding: "12px 20px", display: "flex", gap: 16, alignItems: "center", flexWrap: "wrap" }}>
      <button type="button" onClick={onBack}>Back</button>
      <strong>Task QA · human review</strong>
      <span role="status">{connected ? `${pending} pending decision${pending === 1 ? "" : "s"}` : "QA service not connected"}</span>
      <button type="button" onClick={() => { lastSeen.current = 0; setConnected(false); setGeneration(n => n + 1); }}>Reconnect</button>
      <a href={`${origin}/?mode=hitl`} target="_blank" rel="noreferrer">Standalone review</a>
    </div>
    {!connected && <div style={{ padding: 24 }}><h2>Connect the QA service</h2><p>Waiting for the review client. If the service stopped, restart it and choose Reconnect. Saved decisions and draft reasons are retained.</p>
        <p>Start it from the Workshop checkout with the task directories you want to review:</p>
        <pre style={{ whiteSpace: "pre-wrap" }}>PYTHONPATH=services/environment-qa python3 -m environment_qa --store .qa-local serve --task-root /absolute/path/to/tasks</pre>
        <p>This starts the review service only; it does not authorize paid AI execution.</p>
      </div>}
    <iframe ref={frame} key={generation} title="Environment QA review" src={source}
      style={{ border: 0, flex: 1, width: "100%", minHeight: 550, display: connected ? "block" : "none" }} />
  </section>;
}
