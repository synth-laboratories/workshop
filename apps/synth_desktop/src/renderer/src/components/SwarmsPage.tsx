import { useEffect, useMemo, useRef, useState } from "react";
import type { VisualRecord } from "@synth/runtime-protocol";
import { publicError } from "../runtime/publicError";
import { isTerminalSwarmState, mergeSwarmEvent, swarmEventState, swarmSdkCall, watchSwarm, type SwarmEventEnvelope } from "../runtime/cloudSwarms";
import { bridges } from "../runtime/desktopBridge";

type Props = { onBack: () => void; onOpenVisual: (visual: VisualRecord) => void };
const SWARM_VISUAL_TEMPLATE = "live.synth_swarm.v1";
const SWARM_AGENT_MODEL = "gpt-5.6-luna";
const SWARM_AGENT_HARNESS = "codex";

function swarmIdOf(value: Record<string, unknown> | null): string | null {
	const candidate = value?.swarm_id ?? value?.run_id ?? value?.swarmId;
	return typeof candidate === "string" ? candidate : null;
}

const record = (value: unknown): Record<string, unknown> | null =>
	value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
const text = (value: unknown): string | null => typeof value === "string" && value.trim() ? value.trim() : null;

function configurationFacts(swarm: Record<string, unknown>) {
	const snapshot = record(record(swarm.configuration)?.snapshot);
	const resolved = record(snapshot?.resolved_actor_profile);
	return {
		model: text(snapshot?.agent_model) ?? text(resolved?.model) ?? SWARM_AGENT_MODEL,
		harness: text(snapshot?.agent_harness) ?? text(snapshot?.agent_kind) ?? text(resolved?.agent_harness) ?? SWARM_AGENT_HARNESS
	};
}

function visualInputs(visual: VisualRecord): Record<string, Record<string, unknown>> {
	const bindings = record(visual.bindings);
	const inputs = Array.isArray(bindings?.inputs) ? bindings.inputs : [];
	return Object.fromEntries(inputs.flatMap((input) => {
		const row = record(input);
		const name = text(row?.input);
		const data = record(row?.data);
		return name && data ? [[name, data]] : [];
	}));
}

function visualBindings(
	swarm: Record<string, unknown>,
	events: SwarmEventEnvelope[],
	streamState: string,
	objective: string,
	maxSpendUsd: number,
	maxConcurrentActors: number
) {
	const configuration = configurationFacts(swarm);
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
					model: configuration.model,
					harness: configuration.harness,
					provider: "auto",
					streamState,
					usage: swarm.usage ?? null
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
	const [deployment, setDeployment] = useState<string | null>(null);
	const visualUpdateSequence = useRef(0);
	const swarmId = swarmIdOf(swarm);
	const latest = events.at(-1);

	useEffect(() => {
		if (!bridges.visuals) return;
		let disposed = false;
		void Promise.all([
			swarmSdkCall("capabilities", {}),
			bridges.visuals.list({ templateId: SWARM_VISUAL_TEMPLATE, status: "live", limit: 100 })
		]).then(async ([capabilities, visuals]) => {
			const currentDeployment = text(capabilities.deployment) ?? "unknown";
			setDeployment(currentDeployment);
			let resumable: { visual: VisualRecord; summary: Record<string, unknown>; events: SwarmEventEnvelope[] } | null = null;
			for (const visual of visuals) {
				const metadata = record(visual.metadata);
				const visualDeployment = text(metadata?.deployment);
				const inputs = visualInputs(visual);
				const priorEvents = Array.isArray(inputs.stream?.events) ? inputs.stream.events as SwarmEventEnvelope[] : [];
				const terminalState = priorEvents.length ? swarmEventState(priorEvents.at(-1)!) : "";
				if (!visualDeployment || visualDeployment !== currentDeployment) {
					await bridges.visuals?.update(visual.id, {
						status: "saved",
						bumpRevision: true,
						metadata: { ...metadata, streamState: "closed", recoveryReason: "legacy_or_other_deployment", recoveredAt: new Date().toISOString() }
					});
				} else if (isTerminalSwarmState(terminalState)) {
					await bridges.visuals?.update(visual.id, {
						status: terminalState === "failed" ? "failed" : "saved",
						bumpRevision: true,
						metadata: { ...metadata, streamState: "closed", recoveredAt: new Date().toISOString() }
					});
				} else if (!resumable && inputs.swarm && swarmIdOf(inputs.swarm)) {
					resumable = { visual, summary: inputs.swarm, events: priorEvents };
				}
			}
			if (!disposed && resumable) {
				const { visual, summary, events: priorEvents } = resumable;
				setVisualId(visual.id);
				setSwarm({ ...summary, swarm_id: swarmIdOf(summary) });
				setEvents(priorEvents.slice(-500));
				setObjective(text(summary.objective) ?? objective);
				if (typeof summary.maxSpendUsd === "number") setMaxSpend(String(summary.maxSpendUsd));
				if (typeof summary.maxConcurrentActors === "number") setConcurrency(String(summary.maxConcurrentActors));
				setStreamState("reconnecting");
			}
		}).catch((reason) => { if (!disposed) setError(publicError(reason)); });
		return () => { disposed = true; };
	// Reconcile once at mount. The recovered objective is intentionally not a dependency.
	// eslint-disable-next-line react-hooks/exhaustive-deps
	}, []);

	useEffect(() => {
		if (!swarmId) return;
		let dispose: (() => void) | undefined;
		void watchSwarm(
			swarmId,
			(event) => setEvents((current) => mergeSwarmEvent(current, event)),
			(state, detail) => {
				setStreamState(state);
				if (detail) setError(detail);
				if (state === "closed") {
					void swarmSdkCall("usage", { swarmId }).then((usage) => {
						setSwarm((current) => current ? { ...current, usage } : current);
					}).catch((reason) => setError(publicError(reason)));
				}
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
				// The visual host reconciles updates by revision. Persist each debounced
				// SSE frame as a revision so the open pane advances live and the Visuals
				// registry can replay the same state after navigation or restart.
				bumpRevision: true,
				metadata: { swarmId, streamState, eventCount: events.length, transport: "sdk_sse" }
			}).catch((reason) => setError(publicError(reason)));
		}, 1_000);
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
			const capabilities = await swarmSdkCall("capabilities", {});
			const launchDeployment = text(capabilities.deployment) ?? deployment ?? "unknown";
			setDeployment(launchDeployment);
			const preflight = await swarmSdkCall("preflight", payload);
			if (preflight.clear_to_trigger !== true) {
				const blockers = Array.isArray(preflight.blockers) ? preflight.blockers.join(" · ") : "Swarm preflight was refused";
				throw new Error(blockers);
			}
			const createdSwarm = await swarmSdkCall("create", payload);
			const createdSwarmId = swarmIdOf(createdSwarm);
			const configuration = createdSwarmId
				? await swarmSdkCall("configuration", { swarmId: createdSwarmId }).catch(() => null)
				: null;
			const hydratedSwarm = configuration ? { ...createdSwarm, configuration } : createdSwarm;
			setSwarm(hydratedSwarm);
			if (bridges.visuals) {
				const title = objective.trim().length > 72 ? `${objective.trim().slice(0, 69)}…` : objective.trim();
				const visual = await bridges.visuals.create({
					templateId: SWARM_VISUAL_TEMPLATE,
					title,
					status: "live",
					bindings: visualBindings(hydratedSwarm, [], "idle", objective.trim(), Number(maxSpend), Number(concurrency)),
					metadata: { swarmId: swarmIdOf(createdSwarm), transport: "sdk_sse", source: "synth-cloud-swarms-sdk", deployment: launchDeployment }
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
		try {
			const next = await swarmSdkCall(operation, { swarmId });
			setSwarm((current) => ({ ...next, configuration: current?.configuration }));
		}
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
