import { useEffect, useMemo, useRef, useState } from "react";
import type { VisualRecord } from "@synth/runtime-protocol";
import { publicError } from "../runtime/publicError";
import { swarmSdkCall, watchSwarm, type SwarmEventEnvelope } from "../runtime/cloudSwarms";
import { bridges } from "../runtime/desktopBridge";

type Props = { onBack: () => void; onOpenVisual: (visual: VisualRecord) => void };
const SWARM_VISUAL_TEMPLATE = "live.synth_swarm.v1";
const SWARM_AGENT_MODEL = "gpt-5.6-luna";
// Cloud swarms use the SDK-native harness so the control plane can inject the
// selected provider/model without depending on a developer's Codex login in
// the actor container.
const SWARM_AGENT_HARNESS = "opencode_sdk";

function swarmIdOf(value: Record<string, unknown> | null): string | null {
	const candidate = value?.swarm_id ?? value?.run_id;
	return typeof candidate === "string" ? candidate : null;
}

function visualBindings(
	swarm: Record<string, unknown>,
	events: SwarmEventEnvelope[],
	streamState: string,
	objective: string,
	maxSpendUsd: number,
	maxConcurrentActors: number
) {
	return {
		schemaVersion: "synth.visual-bindings.v1",
		inputs: [
			{
				input: "stream",
				kind: "inline",
				schema: "synth.visual.live_eval_events.v1",
				data: { events: events.map((event, index) => ({ ...event, logical_time: index + 1 })) }
			},
			{
				input: "swarm",
				kind: "inline",
				schema: "synth.research-swarm-summary.v1",
				data: {
					schemaVersion: "synth.research-swarm-summary.v1",
					swarmId: swarmIdOf(swarm),
					objective,
					status: swarm.status ?? swarm.state ?? "starting",
					projectId: swarm.project_id ?? swarm.projectId ?? null,
					maxSpendUsd,
					maxConcurrentActors,
					model: SWARM_AGENT_MODEL,
					harness: SWARM_AGENT_HARNESS,
					provider: "auto",
					streamState
				}
			}
		]
	};
}

export function SwarmsPage({ onBack, onOpenVisual }: Props) {
	const [objective, setObjective] = useState("Investigate the selected task and return evidence-backed results.");
	const [projectId, setProjectId] = useState("");
	const [maxSpend, setMaxSpend] = useState("5");
	const [concurrency, setConcurrency] = useState("4");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [swarm, setSwarm] = useState<Record<string, unknown> | null>(null);
	const [events, setEvents] = useState<SwarmEventEnvelope[]>([]);
	const [streamState, setStreamState] = useState<"idle" | "connected" | "reconnecting" | "closed" | "error">("idle");
	const [visualId, setVisualId] = useState<string | null>(null);
	const visualUpdateSequence = useRef(0);
	const swarmId = swarmIdOf(swarm);
	const latest = events.at(-1);

	useEffect(() => {
		if (!swarmId) return;
		let dispose: (() => void) | undefined;
		void watchSwarm(
			swarmId,
			(event) => setEvents((current) => [...current.slice(-499), event]),
			(state, detail) => {
				setStreamState(state);
				if (detail) setError(detail);
			}
		).then((close) => { dispose = close; }).catch((reason) => {
			setStreamState("error");
			setError(publicError(reason));
		});
		return () => dispose?.();
	}, [swarmId]);

	useEffect(() => {
		if (!visualId || !swarm || !bridges.visuals) return;
		const sequence = ++visualUpdateSequence.current;
		const timer = window.setTimeout(() => {
			if (sequence !== visualUpdateSequence.current) return;
			void bridges.visuals?.update(visualId, {
				bindings: visualBindings(swarm, events, streamState, objective.trim(), Number(maxSpend), Number(concurrency)),
				status: streamState === "error" ? "failed" : streamState === "closed" ? "saved" : "live",
				bumpRevision: false,
				metadata: { swarmId, streamState, eventCount: events.length, transport: "sdk_sse" }
			}).catch((reason) => setError(publicError(reason)));
		}, 200);
		return () => window.clearTimeout(timer);
	}, [concurrency, events, maxSpend, objective, streamState, swarm, swarmId, visualId]);

	const spec = useMemo(() => ({
		objective: objective.trim(),
		runbook: "lite",
		timeboxSeconds: 900,
		agentModel: SWARM_AGENT_MODEL,
		agentHarness: SWARM_AGENT_HARNESS,
		provider: "auto",
		idempotencyKey: crypto.randomUUID(),
		limit: {
			maxSpendUsd: Number(maxSpend),
			maxConcurrentActors: Number(concurrency),
			maxWallclockSeconds: 900
		},
		executionTarget: { kind: "platform_resolved" }
	}), [objective, maxSpend, concurrency]);

	async function launch() {
		setBusy(true); setError(null); setEvents([]);
		try {
			const payload = { projectId: projectId.trim() || undefined, spec };
			const preflight = await swarmSdkCall("preflight", payload);
			if (preflight.clear_to_trigger !== true) {
				const blockers = Array.isArray(preflight.blockers) ? preflight.blockers.join(" · ") : "Swarm preflight was refused";
				throw new Error(blockers);
			}
			const createdSwarm = await swarmSdkCall("create", payload);
			setSwarm(createdSwarm);
			if (bridges.visuals) {
				const title = objective.trim().length > 72 ? `${objective.trim().slice(0, 69)}…` : objective.trim();
				const visual = await bridges.visuals.create({
					templateId: SWARM_VISUAL_TEMPLATE,
					title,
					status: "live",
					bindings: visualBindings(createdSwarm, [], "idle", objective.trim(), Number(maxSpend), Number(concurrency)),
					metadata: { swarmId: swarmIdOf(createdSwarm), transport: "sdk_sse", source: "synth-cloud-swarms-sdk" }
				});
				setVisualId(visual.id);
				const shown = await bridges.visuals.show(visual.id).catch(() => visual);
				onOpenVisual(shown);
			}
		} catch (reason) { setError(publicError(reason)); }
		finally { setBusy(false); }
	}

	async function control(operation: "pause" | "resume" | "cancel") {
		if (!swarmId) return;
		setBusy(true); setError(null);
		try { setSwarm(await swarmSdkCall(operation, { swarmId })); }
		catch (reason) { setError(publicError(reason)); }
		finally { setBusy(false); }
	}

	return <main className="settings-page" data-testid="swarms-page">
		<header className="settings-header">
			<button type="button" className="back-button" onClick={onBack}>Back</button>
			<div><h1>Swarms</h1><p>Launch and supervise bounded Synth Cloud swarms through the pinned SDK.</p></div>
		</header>
		<section className="settings-section">
			<h2>New swarm</h2>
			<label className="settings-field"><span>Objective</span><textarea value={objective} onChange={(event) => setObjective(event.target.value)} rows={4} /></label>
			<label className="settings-field"><span>Project ID <small>(optional)</small></span><input value={projectId} onChange={(event) => setProjectId(event.target.value)} placeholder="One-off when empty" /></label>
			<div style={{display:"grid", gridTemplateColumns:"1fr 1fr", gap:12}}>
				<label className="settings-field"><span>Maximum spend (USD)</span><input type="number" min="0" step="0.25" value={maxSpend} onChange={(event) => setMaxSpend(event.target.value)} /></label>
				<label className="settings-field"><span>Maximum concurrent actors</span><input type="number" min="1" max="64" value={concurrency} onChange={(event) => setConcurrency(event.target.value)} /></label>
			</div>
			<button type="button" className="primary-button" disabled={busy || !objective.trim()} onClick={() => void launch()}>{busy ? "Working…" : "Preflight and launch"}</button>
			{error ? <p role="alert" className="settings-error">{error}</p> : null}
		</section>
		{swarmId ? <section className="settings-section" aria-live="polite">
			<div style={{display:"flex", justifyContent:"space-between", gap:12, alignItems:"center"}}>
				<div><h2>Live swarm</h2><p><code>{swarmId}</code> · {streamState} · {events.length} events</p></div>
				<div style={{display:"flex", gap:8}}><button type="button" onClick={() => void control("pause")} disabled={busy}>Pause</button><button type="button" onClick={() => void control("resume")} disabled={busy}>Resume</button><button type="button" onClick={() => void control("cancel")} disabled={busy}>Cancel</button></div>
			</div>
			{visualId ? <p>The live swarm visual is open in the right panel and follows the SDK event stream without polling.</p> : null}
			{latest ? <div className="settings-card"><strong>{latest.kind}</strong><p>Sequence {latest.sequence} · state {latest.state_version}</p><pre style={{whiteSpace:"pre-wrap", maxHeight:320, overflow:"auto"}}>{JSON.stringify(latest.payload, null, 2)}</pre></div> : <p>Waiting for the first SDK event…</p>}
		</section> : null}
	</main>;
}
