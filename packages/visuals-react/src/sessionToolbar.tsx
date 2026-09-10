import { useEffect, useState } from "react";
import type { SessionCheckpoint } from "@synth/visuals-protocol";
import { useVisualSessionClient, useVisualSessionSnapshot } from "./session.ts";

export function VisualSessionToolbar() {
  const client = useVisualSessionClient(); const session = useVisualSessionSnapshot();
  const [saved, setSaved] = useState<Array<Pick<SessionCheckpoint,"id"|"capturedAt"|"digest">>>([]);
  const [recordings, setRecordings] = useState<Array<{ id: string; createdAt: string;eventCount:number }>>([]);
  const replayState=session?.state.replay;
  const replay=replayState && "recordingId" in replayState ? {id:replayState.recordingId,eventCount:replayState.eventCount ?? 0} : undefined;
  const cursor=replayState && "sequence" in replayState ? replayState.sequence : 0;
  const playing=Boolean(replayState && "playing" in replayState && replayState.playing);
  const intervalMs=replayState && "recordingId" in replayState ? replayState.intervalMs??300 : 300;
  const [busy, setBusy] = useState(false); const [error, setError] = useState<string>();
  const [pixelPath,setPixelPath]=useState<string>();
  useEffect(() => {
    if (!playing || !client || !replay || busy) return;
    const timer = setTimeout(() => {
      setBusy(true);
      void client.tickRecording().catch((reason) => { setError(String(reason)); }).finally(() => setBusy(false));
    }, intervalMs);
    return () => clearTimeout(timer);
  }, [playing,client,replay?.id,cursor,busy,intervalMs]);
  if (!client || !session) return null;
  const run = async (work: () => Promise<unknown>) => {
    setBusy(true); setError(undefined);
    try { await work(); } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  };
  return <div className="visual-session-tools" role="group" aria-label="Visual session" style={{ display:"flex",gap:8,alignItems:"center",flexWrap:"wrap",padding:"6px 10px",fontSize:12 }}>
    <span>{session.ready ? `State ${session.state.stateVersion}` : "Restoring view…"}</span>
    {session.state.replay && <span role="status">{playing ? "Playing recorded presentation" : "Restored playback is paused; edit a control to leave replay."}</span>}
    <button disabled={!session.ready || busy} onClick={() => void run(async () => { await client.capture(); setSaved(await client.checkpoints()); })}>Snapshot</button>
    {client.supportsPixelCapture && <button disabled={!session.ready || busy} onClick={()=>void run(async()=>{
      const capture=await client.capturePixels();setPixelPath(String(capture.path));setSaved(await client.checkpoints());
    })}>Capture pixels</button>}
    {pixelPath && <span role="status">PNG and state receipt saved: {pixelPath}</span>}
    <button disabled={!session.ready || busy} aria-pressed={Boolean(session.recordingId)} onClick={() => void run(() => session.recordingId ? client.stopRecording() : client.startRecording())}>{session.recordingId ? "Stop recording" : "Record"}</button>
    <button disabled={!session.ready || busy} onClick={() => void run(async () => { setSaved(await client.checkpoints()); setRecordings(await client.recordings()); })}>Saved views</button>
    <button disabled={!session.ready || busy} onClick={() => void run(async () => {
      const snapshot=await client.capture();
      const url=URL.createObjectURL(new Blob([JSON.stringify(snapshot,null,2)],{type:"application/json"}));
      const link=document.createElement("a");link.href=url;link.download=`visual-${snapshot.id}.json`;link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
    })}>Export state</button>
    <label>Import state<input aria-label="Import visual checkpoint" type="file" accept="application/json,.json" disabled={!session.ready || busy} style={{maxWidth:180}} onChange={(event)=>{
      const file=event.target.files?.[0];event.target.value="";if(!file)return;
      void run(async()=>{if(file.size>2_000_000)throw new Error("Bundle exceeds 2 MB import limit");const bundle=JSON.parse(await file.text());if(bundle.schemaVersion==="synth.visual-session-recording.v1"){await client.importRecording(bundle);setRecordings(await client.recordings());}else{await client.importCheckpoint(bundle);setSaved(await client.checkpoints());}});
    }}/></label>
    {saved.length > 0 && <select aria-label="Restore snapshot" value="" disabled={busy || Boolean(session.recordingId)} onChange={(event) => void run(() => client.restore(event.target.value))}>
      <option value="" disabled>Restore snapshot…</option>{saved.map((item) => <option key={item.id} value={item.id}>{new Date(item.capturedAt).toLocaleString()}</option>)}
    </select>}
    {recordings.length > 0 && <select aria-label="Replay recording" value={replay?.id ?? ""} disabled={busy || Boolean(session.recordingId)} onChange={(event) => void run(async () => { const recording=recordings.find(item=>item.id===event.target.value);if(!recording)return; await client.seekRecording(recording.id,0); })}>
      <option value="" disabled>Replay recording…</option>{recordings.map((item) => <option key={item.id} value={item.id}>{new Date(item.createdAt).toLocaleString()}</option>)}
    </select>}
    {replay && <>
      <button disabled={busy} onClick={()=>void run(async()=>{
        const recording=await client.recording(replay.id);const text=JSON.stringify(recording);
        if(text.length>2_000_000)throw new Error("Recording exceeds portable bundle limit; use paginated record.read");
        const url=URL.createObjectURL(new Blob([text],{type:"application/json"}));const link=document.createElement("a");link.href=url;link.download=`recording-${replay.id}.json`;link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
      })}>Export recording</button>
      <button disabled={busy || Boolean(session.recordingId)} onClick={() => void run(()=>client.playRecording(!playing))}>{playing ? "Pause" : "Play"}</button>
      <label>Speed <select aria-label="Recording speed" value={intervalMs} disabled={busy || Boolean(session.recordingId)} onChange={(event)=>void run(()=>client.playRecording(playing,Number(event.target.value)))}>
        {[1200,600,300,150,75].map(interval=><option key={interval} value={interval}>{300/interval}×</option>)}
        {![1200,600,300,150,75].includes(intervalMs) && <option value={intervalMs}>{(300/intervalMs).toFixed(2)}×</option>}
      </select></label>
      <button aria-label="Previous recording event" disabled={busy || Boolean(session.recordingId) || cursor===0} onClick={()=>void run(()=>client.seekRecording(replay.id,cursor-1))}>Previous event</button>
      <button aria-label="Next recording event" disabled={busy || Boolean(session.recordingId) || cursor>=replay.eventCount} onClick={()=>void run(()=>client.seekRecording(replay.id,cursor+1))}>Next event</button>
      <input aria-label="Recording event" type="range" min={0} max={replay.eventCount} value={cursor} disabled={busy || Boolean(session.recordingId)} onChange={(event) => { const next=Number(event.target.value); void run(() => client.seekRecording(replay.id,next)); }} />
      <span>{cursor}/{replay.eventCount}</span>
    </>}
    {(error || session.error) && <span role="alert">{error || session.error}</span>}
  </div>;
}
