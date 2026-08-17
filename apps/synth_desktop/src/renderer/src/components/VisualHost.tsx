import { Component, useEffect, useMemo, useRef, useState, type ComponentType, type ErrorInfo, type ReactNode } from "react";
import type { ArtifactRef } from "../types/landing";
import type { VisualRecord } from "@synth/runtime-protocol";
import {
	bindTemplateSlots,
	createReplayClient,
	isVisualBindings,
	propsFromBindings,
	consumeInjectedRendererCrash,
	rememberLastKnownGood,
	replayStreamsFromBindings,
	resolveTemplate,
	resolveVisualBindings,
	selectRenderedProjection
} from "@synth/visuals";
import { publicError, toPublicError, type PublicError } from "../runtime/publicError";
import type { VisualAnnotation, VisualSeal, VisualSealBundle, VisualUpload } from "../bridge";
import { loadVisualShell } from "../runtime/visualsLoader";
import { bridges } from "../runtime/desktopBridge";
import { subscribeToRun } from "../runtime/runProgress/subscription";
import { progressAgreement, projectRunProgress, splitSnapshotEvents } from "../runtime/runProgress/project";
import type { ProgressAgreement } from "../runtime/runProgress/project";
import { DIAGNOSTIC_CODES, reportDiagnostic } from "../runtime/diagnostics";
import { MermaidVisual } from "./MermaidVisual";
import { SystemsMapVisual } from "./SystemsMapVisual";
import { SystemsDynamicVisual } from "./SystemsDynamicVisual";
import type { SubagentState } from "../runtime/sessionView";
import { bindingAuthorityKey } from "../runtime/visualRevisionState";

type ShellProps = {
	title?: string;
	lede?: string;
	bindings?: Record<string, unknown>;
	[key: string]: unknown;
};

export function artifactFromVisualRecord(visual: VisualRecord): ArtifactRef {
	return {
		id: visual.id,
		kind: "report",
		title: visual.title,
		templateId: visual.templateId,
		visualId: visual.id,
		revision: visual.currentRevision,
		rendererKind: visual.rendererKind,
		bindings: visual.bindings,
		metadata: visual.metadata,
		summary: typeof visual.metadata?.summary === "string" ? visual.metadata.summary : undefined,
		preview: {
			variant:
				visual.templateId.includes("scrub") || visual.templateId.includes("rollout")
					? "craftax_frame"
					: visual.templateId.includes("craftax") || visual.templateId.includes("eval_matrix")
						? "craftax_pareto"
						: "generic"
		}
	};
}

function elapsedLabel(value: string, now: number): string {
	const seconds = Math.max(0, Math.floor((now - Date.parse(value)) / 1000));
	if (seconds < 60) return `${seconds}s`;
	if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
	return `${Math.floor(seconds / 3600)}h`;
}

function subagentStatusLabel(status: SubagentState["status"]): string {
	return ({
		starting: "Starting",
		working: "Working",
		completed: "Completed",
		interrupted: "Interrupted",
		failed: "Failed",
		stopped: "Stopped",
		unavailable: "Unavailable"
	})[status];
}

function subagentMarker(id: string): string {
	let value = 0;
	for (let index = 0; index < id.length; index += 1) value = (value + id.charCodeAt(index)) % 2;
	return value ? "✣" : "✺";
}

function SubagentsVisual({ artifact }: { artifact: ArtifactRef }) {
	const resolved = propsFromBindings(artifact.bindings);
	const agents = Array.isArray(resolved.props.agents) ? resolved.props.agents as SubagentState[] : [];
	const sessionId = typeof resolved.props.sessionId === "string" ? resolved.props.sessionId : undefined;
	const [now, setNow] = useState(Date.now());
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const [detail, setDetail] = useState<unknown>(null);
	const [detailError, setDetailError] = useState<string | null>(null);
	useEffect(() => {
		const timer = window.setInterval(() => setNow(Date.now()), 1_000);
		return () => window.clearInterval(timer);
	}, []);
	useEffect(() => {
		if (!selectedId || !sessionId || !bridges.codex?.readThread) {
			setDetail(null);
			setDetailError(null);
			return;
		}
		let cancelled = false;
		void bridges.codex.readThread(sessionId, selectedId, true).then(
			(payload) => {
				if (!cancelled) {
					setDetail(payload);
					setDetailError(null);
				}
			},
			(reason) => {
				if (!cancelled) {
					setDetail(null);
					setDetailError(publicError(reason));
				}
			}
		);
		return () => {
			cancelled = true;
		};
	}, [selectedId, sessionId]);
	const groups = [
		{ label: "Working", agents: agents.filter((agent) => agent.status === "starting" || agent.status === "working") },
		{ label: "Needs attention", agents: agents.filter((agent) => agent.status === "interrupted" || agent.status === "failed" || agent.status === "stopped" || agent.status === "unavailable") },
		{ label: "Completed", agents: agents.filter((agent) => agent.status === "completed") }
	];
	const selected = agents.find((agent) => agent.id === selectedId) ?? null;
	const working = groups[0].agents.length;
	const attention = groups[1].agents.length;
	const completed = groups[2].agents.length;
	if (selected) {
		return (
			<div className="subagents-visual" data-testid="visual-subagents">
				<button type="button" className="subagents-back" data-testid="subagents-back" onClick={() => setSelectedId(null)}>
					← {selected.title}
				</button>
				<p className="subagents-workspace-summary" data-testid="subagents-workspace-summary">
					{subagentStatusLabel(selected.status)} · {selected.status === "starting" || selected.status === "working" ? elapsedLabel(selected.startedAt, now) : elapsedLabel(selected.updatedAt, now)}
				</p>
				<div className="subagents-detail" data-testid="subagents-detail">
					{selected.summary ? <p>{selected.summary}</p> : <p>No result yet</p>}
					{detailError ? <p className="subagents-empty">{detailError}</p> : null}
					{detail ? <pre>{JSON.stringify(detail, null, 2)}</pre> : null}
				</div>
			</div>
		);
	}
	return (
		<div className="subagents-visual" data-testid="visual-subagents">
			<p className="subagents-workspace-summary" data-testid="subagents-workspace-summary">
				{working} working · {attention} need attention · {completed} completed
			</p>
			{groups.map((group) => (
				<section key={group.label} className="subagents-group">
					<h3>{group.label} · {group.agents.length}</h3>
					{group.agents.length === 0 ? <p className="subagents-empty">No {group.label.toLowerCase()} subagents</p> : null}
					{group.agents.map((agent) => (
						<button
							type="button"
							className="subagent-row"
							key={agent.id}
							data-status={agent.status}
							data-testid={`subagent-row-${agent.id}`}
							onClick={() => setSelectedId(agent.id)}
						>
							<span className={`subagent-mark mark-${agent.status}`} aria-hidden>{subagentMarker(agent.id)}</span>
							<div className="subagent-copy">
								<div className="subagent-title-row"><strong>{agent.title}</strong><span className={`subagent-state state-${agent.status}`}>{subagentStatusLabel(agent.status)}</span></div>
								{agent.summary ? <p>{agent.summary}</p> : null}
							</div>
							<time dateTime={agent.updatedAt}>{agent.status === "starting" || agent.status === "working" ? elapsedLabel(agent.startedAt, now) : elapsedLabel(agent.updatedAt, now) + " ago"}</time>
						</button>
					))}
				</section>
			))}
		</div>
	);
}

function CraftaxEvalVisual({ artifact }: { artifact: ArtifactRef }) {
	const models = [
		{ name: "Laguna XS", ach: 11.4, cost: 0.12, accent: true },
		{ name: "Luna", ach: 10.1, cost: 0.09 },
		{ name: "Terra", ach: 9.4, cost: 0.15 },
		{ name: "Flash Lite", ach: 7.2, cost: 0.04 },
		{ name: "Kimi K3", ach: 8.8, cost: 0.11 }
	];
	const achievements = [
		["drink", "food", "sapling", "wood", "cow", "zombie"],
		["pickaxe", "sword", "plant", "table", "coal", "stone"],
		["skeleton", "iron", "furnace", "ladder", "bow", "arrow"]
	];
	return (
		<div className="craftax-visual" data-testid="visual-craftax-pareto">
			<div className="craftax-visual-hero">
				<p className="visual-kicker">Open-ended agents · Craftax</p>
				<h2>{artifact.title}</h2>
				{artifact.summary ? <p className="visual-lede">{artifact.summary}</p> : null}
			</div>
			<section className="craftax-section">
				<div className="craftax-section-head">
					<h3>Cost vs performance</h3>
					<span>ACH ↑ · $ / rollout →</span>
				</div>
				<div className="pareto-plot" role="img" aria-label="Pareto chart of achievements vs cost">
					<svg viewBox="0 0 320 200" className="pareto-svg">
						{[40, 80, 120, 160].map((y) => (
							<line key={y} x1="36" y1={y} x2="300" y2={y} stroke="#e8eaee" strokeWidth="1" />
						))}
						{models.map((m, i) => {
							const x = 70 + i * 45;
							const y = 155 - m.ach * 9;
							return <circle key={m.name} cx={x} cy={y} r={m.accent ? 6 : 4} fill={m.accent ? "#f05f22" : "#9aa3b2"} />;
						})}
					</svg>
				</div>
			</section>
			<section className="craftax-section">
				<h3>Achievement matrix</h3>
				<div className="achievement-matrix">
					{achievements.map((row, rowIndex) => (
						<div key={rowIndex} className="achievement-row">
							{row.map((cell) => <span key={cell}>{cell}</span>)}
						</div>
					))}
				</div>
			</section>
		</div>
	);
}

function CraftaxFrameVisual({ artifact }: { artifact: ArtifactRef }) {
	return (
		<div className="craftax-visual" data-testid="visual-craftax-frame">
			<div className="craftax-visual-hero">
				<p className="visual-kicker">Environment frame</p>
				<h2>{artifact.title}</h2>
				{artifact.summary ? <p className="visual-lede">{artifact.summary}</p> : null}
			</div>
			<div className="env-frame" role="img" aria-label="Craftax frame">
				<div className="env-grid">
					{Array.from({ length: 96 }, (_, i) => (
						<span key={i} className={`env-tile t-${(i * 7) % 5}`} />
					))}
				</div>
			</div>
		</div>
	);
}

function MockFallback({ artifact }: { artifact: ArtifactRef }) {
	const variant = artifact.preview?.variant ?? "generic";
	if (variant === "craftax_pareto") return <CraftaxEvalVisual artifact={artifact} />;
	if (variant === "craftax_frame") return <CraftaxFrameVisual artifact={artifact} />;
	return (
		<div className="visual-generic" data-testid="visual-fallback">
			<h2>{artifact.title}</h2>
			{artifact.summary ? <p>{artifact.summary}</p> : null}
			{artifact.templateId ? <p className="visual-template-id">template · {artifact.templateId}</p> : null}
		</div>
	);
}

function TemplateVisualHost({ artifact }: { artifact: ArtifactRef }) {
	const [Shell, setShell] = useState<ComponentType<ShellProps> | null>(null);
	const [failed, setFailed] = useState(false);
	const [optimizerPayload, setOptimizerPayload] = useState<Record<string, unknown> | null>(null);
	const [optimizerLoadError, setOptimizerLoadError] = useState<string | null>(null);
	const [comparisonPayload, setComparisonPayload] = useState<Record<string, unknown> | null>(null);
	const [progressView, setProgressView] = useState<ProgressAgreement | null>(null);
	// One reader decides whether these bindings are legible, and it can say no.
	// Returning an empty slot list for a shape it did not understand is how a
	// visual with ten declared streams rendered an empty pane with no error.
	const resolvedBindings = useMemo(() => resolveVisualBindings(artifact.bindings), [artifact.bindings]);
	const asyncBindings = useMemo(
		() =>
			resolvedBindings.slots.filter((binding) => {
				if (binding.kind === "trace_v5") return true;
				// Fixture streams are loaded by the shell's bundled examples, the
				// same way live_sse and optimizer_run are subscribed outside bind.
				if (
					binding.kind === "live_sse"
					|| binding.kind === "optimizer_run"
					|| binding.kind === "inline"
					|| binding.kind === "fixture"
				) {
					return false;
				}
				return binding.data === undefined;
			}),
		[resolvedBindings]
	);
	const traceBindings = useMemo(
		() => asyncBindings.filter((binding) => binding.kind === "trace_v5"),
		[asyncBindings]
	);
	const replay = useMemo(
		() => replayStreamsFromBindings(resolvedBindings.slots),
		[resolvedBindings]
	);
	const replayClient = useMemo(
		() =>
			// Native allowlisted polling when the host offers it. Outside the
			// packaged app — browser preview, tests — the client falls back to
			// fetch. The capability is checked, not assumed: a bridge without
			// `pollStream` would otherwise throw on the first poll.
			createReplayClient(replay.streams, typeof bridges.visuals?.pollStream === "function"
				? (pollUrl, after, limit) =>
						bridges.visuals!.pollStream({ visualId: artifact.id, pollUrl, after, limit })
				: undefined),
		[artifact.id, replay.streams]
	);
	const synchronouslyResolved = useMemo(() => {
		if (!isVisualBindings(artifact.bindings) || asyncBindings.length === 0) {
			return propsFromBindings(artifact.bindings);
		}
		const skip = new Set(asyncBindings.map((binding) => `${binding.slot}:${binding.kind}:${binding.source ?? ""}`));
		return propsFromBindings({
			schemaVersion: "synth.visual-bindings.v1",
			slots: artifact.bindings.slots.filter((binding) =>
				!skip.has(`${binding.slot}:${binding.kind}:${binding.source ?? ""}`)
			)
		});
	}, [artifact.bindings, asyncBindings]);
	const [traceResolution, setTraceResolution] = useState<{
		status: "idle" | "loading" | "ready" | "error";
		props: Record<string, unknown>;
		error?: string;
		/** Structured form of the same failure, so the pane can show the stable
		 * code and its remediation instead of only a sentence. */
		failure?: PublicError;
	}>({ status: "idle", props: {} });
	const [lastKnownGoodProps, setLastKnownGoodProps] = useState<Record<string, unknown> | null>(null);
	const [connectionState, setConnectionState] = useState<
		"loading" | "replaying" | "bootstrapping" | "subscribed" | "stale" | "reconnecting" | "terminal" | "failed" | "interrupted"
	>("loading");

	const visualIdentity = useMemo(
		() => ({
			visualId: artifact.visualId ?? artifact.id,
			visualRevision: typeof artifact.revision === "number" ? artifact.revision : null,
		}),
		[artifact.id, artifact.visualId, artifact.revision]
	);

	useEffect(() => {
		if (resolvedBindings.status === "canonical") return;
		if (resolvedBindings.status === "rejected") {
			reportDiagnostic({
				...visualIdentity,
				severity: "error",
				component: "visual-host",
				event: "visual.bindings.invalid",
				code: DIAGNOSTIC_CODES.visualBindingsInvalid,
				message: resolvedBindings.error ?? "Visual bindings are unreadable",
				details: { templateId: artifact.templateId ?? null }
			});
			return;
		}
		// COMPAT: rendered from an upgraded legacy shape. Loud so the writer is
		// fixed before the upgrade path is removed.
		reportDiagnostic({
			...visualIdentity,
			severity: "warn",
			component: "visual-host",
			event: "visual.bindings.upgraded",
			code: DIAGNOSTIC_CODES.visualBindingsUpgraded,
			message: `Rendered from upgraded legacy bindings on ${resolvedBindings.upgradedSlots.join(", ")}`,
			details: { templateId: artifact.templateId ?? null, slots: resolvedBindings.upgradedSlots }
		});
	}, [artifact.templateId, resolvedBindings, visualIdentity]);

	useEffect(() => {
		let cancelled = false;
		const templateId = artifact.templateId;
		setFailed(false);
		setShell(null);
		if (!templateId) {
			setFailed(true);
			reportDiagnostic({
				...visualIdentity,
				severity: "error",
				component: "visual-host",
				event: "visual.template.missing",
				code: DIAGNOSTIC_CODES.visualTemplateUnavailable,
				message: "Visual has no template id to render",
			});
			return;
		}
		void loadVisualShell(templateId)
			.then((Component) => {
				if (cancelled) return;
				if (!Component) {
					setFailed(true);
					reportDiagnostic({
						...visualIdentity,
						severity: "error",
						component: "visual-host",
						event: "visual.shell.unavailable",
						code: DIAGNOSTIC_CODES.visualTemplateUnavailable,
						message: `Template ${templateId} resolved no shell component`,
						details: { templateId },
					});
				} else setShell(() => Component);
			})
			.catch((reason) => {
				if (cancelled) return;
				setFailed(true);
				reportDiagnostic({
					...visualIdentity,
					severity: "error",
					component: "visual-host",
					event: "visual.shell.load_failed",
					code: DIAGNOSTIC_CODES.visualShellLoadFailed,
					message: publicError(reason),
					details: { templateId },
				});
			});
		return () => { cancelled = true; };
	}, [artifact.templateId, visualIdentity]);

	useEffect(() => {
		let cancelled = false;
		if (asyncBindings.length === 0 || !isVisualBindings(artifact.bindings)) {
			setTraceResolution({ status: "idle", props: {} });
			return () => { cancelled = true; };
		}
		const bindings = artifact.bindings;
		const template = artifact.templateId ? resolveTemplate(artifact.templateId) : undefined;
		if (!template) {
			setTraceResolution({ status: "error", props: {}, error: `Template ${artifact.templateId ?? "unknown"} is unavailable` });
			reportDiagnostic({
				...visualIdentity,
				severity: "error",
				component: "visual-host",
				event: "visual.template.unavailable",
				code: DIAGNOSTIC_CODES.visualTemplateUnavailable,
				message: `Template ${artifact.templateId ?? "unknown"} is unavailable`,
				details: { templateId: artifact.templateId ?? null },
			});
			return () => { cancelled = true; };
		}
		if (traceBindings.length > 0 && !bridges.inventory) {
			setTraceResolution({ status: "error", props: {}, error: "Trace projection resolver is unavailable" });
			return () => { cancelled = true; };
		}
		const unsupportedBinding = traceBindings.find((binding) =>
			binding.schema && binding.schema !== "synth.trace-projection.rollout-inspector.v1"
		);
		if (unsupportedBinding) {
			setTraceResolution({ status: "error", props: {}, error: `Unsupported trace projection schema: ${unsupportedBinding.schema}` });
			reportDiagnostic({
				...visualIdentity,
				traceId: typeof unsupportedBinding.source === "string" ? unsupportedBinding.source : null,
				severity: "error",
				component: "visual-host",
				event: "visual.projection.rejected",
				code: DIAGNOSTIC_CODES.unsupportedTraceProjectionSchema,
				message: `Unsupported trace projection schema: ${unsupportedBinding.schema}`,
				details: {
					receivedSchema: unsupportedBinding.schema ?? null,
					expectedSchemas: ["synth.trace-projection.rollout-inspector.v1"],
					templateId: artifact.templateId ?? null,
					slot: unsupportedBinding.slot ?? null,
					remediation: "Project Trace V5 into the visual's accepted input contract.",
				},
			});
			return () => { cancelled = true; };
		}

		setTraceResolution({ status: "loading", props: {} });
		const projectionByDigest = new Map<string, Promise<unknown>>();
		const loadTraceV5 = (source: string) => {
			let pending = projectionByDigest.get(source);
			if (!pending) {
				pending = bridges.inventory!.resolveTraceProjection(source, "rollout-inspector").then((projection) => {
					if (projection.traceDigest !== source) {
						throw new Error(`Trace resolver returned digest ${projection.traceDigest} for ${source}`);
					}
					if (projection.projectionKind !== "rollout-inspector") {
						throw new Error(`Unsupported trace projection kind: ${projection.projectionKind}`);
					}
					if (projection.projectionSchema !== "synth.trace-projection.rollout-inspector.v1") {
						throw new Error(`Unsupported trace projection schema: ${projection.projectionSchema}`);
					}
					return projection.payload;
				});
				projectionByDigest.set(source, pending);
			}
			return pending;
		};
		const loadLocalCas = (source: string) => {
			if (!bridges.runtime) throw new Error(`No local CAS loader for ${source}`);
			return bridges.runtime.request(`/v1/cas/${encodeURIComponent(source)}`);
		};
		const loadQuerySnapshot = (source: string) => {
			if (!bridges.runtime) throw new Error(`No query snapshot loader for ${source}`);
			return bridges.runtime.request("/v1/traces/snapshot", { method: "POST", body: { snapshot_id: source } });
		};
		const loadRun = (source: string) => {
			if (!bridges.optimizers) throw new Error(`No run loader for ${source}`);
			return bridges.optimizers.get(source);
		};
		void bindTemplateSlots(template, bindings, { loadTraceV5, loadLocalCas, loadQuerySnapshot, loadRun, skipOptional: true })
			.then((result) => {
				if (cancelled) return;
				if (result.errors.length > 0) {
					setTraceResolution({ status: "error", props: {}, error: result.errors.join(" · ") });
					reportDiagnostic({
						...visualIdentity,
						severity: "error",
						component: "visual-host",
						event: "visual.binding.unresolved",
						code: DIAGNOSTIC_CODES.visualBindingUnresolved,
						message: result.errors.join(" · "),
						details: { templateId: artifact.templateId ?? null, errorCount: result.errors.length },
					});
					return;
				}
				const props = Object.fromEntries(
					Object.values(result.slots)
						.filter((slot) => slot.kind !== "inline" && slot.kind !== "live_sse" && slot.kind !== "optimizer_run")
						.map((slot) => [slot.slot, slot.data])
				);
				setTraceResolution({ status: "ready", props });
				setLastKnownGoodProps((current) => rememberLastKnownGood(current, props, false));
			})
			.catch((reason) => {
				if (cancelled) return;
				const failure = toPublicError(reason, "Trace projection resolution failed");
				const message = publicError(reason, "Trace projection resolution failed");
				setTraceResolution({ status: "error", props: {}, error: message, failure });
				reportDiagnostic({
					...visualIdentity,
					severity: "error",
					component: "visual-host",
					event: "visual.projection.failed",
					code: message.includes("projection schema")
						? DIAGNOSTIC_CODES.unsupportedTraceProjectionSchema
						: DIAGNOSTIC_CODES.visualBindingUnresolved,
					message,
					details: { templateId: artifact.templateId ?? null },
				});
			});
		return () => { cancelled = true; };
	}, [artifact.id, artifact.revision, artifact.templateId, artifact.bindings, asyncBindings.length, traceBindings.length]);

	/*
	 * The optimizer stream is read through the shared `RunProgressSubscription`
	 * store, not a private loop here. One run can be open in the transcript card,
	 * its dialog, and this pane at once; the store gives all three the same
	 * cursor, the same gap recovery, and one set of upstream reads.
	 *
	 * What stays local to the pane is what is genuinely the visual's: the ready
	 * receipt, and a visual-scoped copy of a stream failure so a blank pane and
	 * the run behind it remain joinable by visual id.
	 */
	useEffect(() => {
		const bindings = artifact.bindings as { slots?: Array<{ slot?: string; kind?: string; source?: string }> } | undefined;
		const slot = bindings?.slots?.find((entry) => entry.slot === "optimizer_run" && entry.kind === "optimizer_run");
		const optimizerRunId = slot?.source;
		if (!optimizerRunId) {
			setOptimizerPayload(null);
			setOptimizerLoadError(null);
			setProgressView(null);
			return;
		}
		let postedReady = false;
		return subscribeToRun(optimizerRunId, (snapshot) => {
			const projection = projectRunProgress(snapshot, Date.now());
			setProgressView(projection ? progressAgreement(projection) : null);
			const lanes = snapshot.run ? splitSnapshotEvents(snapshot.run, snapshot.events) : null;
			const payload = snapshot.run && lanes
				? {
					run: snapshot.run,
					events: lanes.terminalEvents,
					enrichmentEvents: lanes.enrichmentEvents,
					terminalCursor: lanes.terminalCursor,
					enrichmentCursor: lanes.enrichmentCursor
				}
				: null;

			if (snapshot.state === "unavailable") {
				setOptimizerPayload(payload);
				setOptimizerLoadError(snapshot.error ?? "Optimizer bridge is unavailable");
				setConnectionState("failed");
				return;
			}
			if (snapshot.state === "interrupted" || snapshot.state === "failed") {
				if (payload) setOptimizerPayload(payload);
				setOptimizerLoadError(snapshot.error ?? "Optimizer stream interrupted");
				setConnectionState("interrupted");
				reportDiagnostic({
					...visualIdentity,
					optimizerRunId,
					streamId: optimizerRunId,
					severity: "error",
					component: "visual-host",
					event: "stream.interrupted",
					code: DIAGNOSTIC_CODES.streamInterrupted,
					message: snapshot.error ?? "Optimizer stream interrupted",
					retryable: true,
				});
				return;
			}
			if (snapshot.state === "stale") {
				if (payload) setOptimizerPayload(payload);
				setOptimizerLoadError(null);
				setConnectionState("stale");
				reportDiagnostic({
					...visualIdentity,
					optimizerRunId,
					streamId: optimizerRunId,
					severity: "warn",
					component: "visual-host",
					event: "stream.replay.gap",
					code: DIAGNOSTIC_CODES.streamReplayGap,
					message: `Optimizer event history is incomplete at ${snapshot.cursor}`,
					retryable: true,
					details: { cursor: snapshot.cursor, gap: snapshot.gap },
				});
				return;
			}
			if (!snapshot.run || !payload) {
				setConnectionState(snapshot.state === "loading" ? "loading" : "replaying");
				return;
			}
			setOptimizerPayload(payload);
			setOptimizerLoadError(null);
			setConnectionState(
				snapshot.state === "terminal" ? "terminal"
					: snapshot.state === "reconnecting" ? "reconnecting"
						: snapshot.state === "replaying" ? "replaying"
							: "subscribed"
			);
			if (!postedReady) {
				postedReady = true;
				void bridges.optimizers?.recordVisualReady?.({
					visualId: artifact.id,
					optimizerRunId,
					templateId: artifact.templateId ?? "optimizer.run.v1",
					replayedThrough: snapshot.cursor,
					subscribedFrom: snapshot.cursor + 1,
					templateDigest: typeof artifact.metadata?.templateDigest === "string"
						? artifact.metadata.templateDigest
						: undefined
				}).catch(() => undefined);
			}
		});
	}, [artifact.bindings, artifact.id, artifact.templateId, artifact.metadata, visualIdentity]);

	const boundRun = optimizerPayload?.run as { id?: string; algorithmId?: string } | undefined;
	const boundRunId = boundRun?.algorithmId === "gepa" ? boundRun.id ?? null : null;
	useEffect(() => {
		// Best-effort companion run for the GEPA comparison card (Luna vs Sol):
		// the most recent sibling GEPA run sharing the recipe prefix of the id.
		if (!boundRunId || !bridges.optimizers) {
			setComparisonPayload(null);
			return;
		}
		let cancelled = false;
		void (async () => {
			try {
				const prefixOf = (id: string) => id.split("_").slice(0, 2).join("_");
				const runs = await bridges.optimizers!.list({ algorithmId: "gepa" });
				const sibling = runs
					.filter((item) => item.id !== boundRunId && prefixOf(item.id) === prefixOf(boundRunId))
					.sort((a, b) => Date.parse(b.createdAt ?? "") - Date.parse(a.createdAt ?? ""))[0];
				if (!sibling) return;
				const events: unknown[] = [];
				let after = 0;
				for (;;) {
					const page = await bridges.optimizers!.eventsAfter(sibling.id, after, 500);
					if (!Array.isArray(page) || page.length === 0) break;
					events.push(...page);
					const last = page[page.length - 1] as { sequenceNumber?: number; sequence_number?: number };
					const next = Number(last.sequenceNumber ?? last.sequence_number ?? 0);
					if (!next || next <= after || page.length < 500) break;
					after = next;
				}
				if (!cancelled && events.length > 0) {
					setComparisonPayload({ run: sibling, events });
				}
			} catch {
				// The comparison card is optional; the primary run view stands alone.
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [boundRunId]);

	if (failed) return <VisualInvalidState title="Template unavailable" detail={`No bundled shell is registered for ${artifact.templateId ?? "this visual"}.`} />;
	if (resolvedBindings.status === "rejected") {
		return <VisualInvalidState title="Visual bindings unreadable" detail={resolvedBindings.error ?? "This visual's bindings could not be read."} />;
	}
	if (synchronouslyResolved.errors.length > 0) return <VisualInvalidState title="Visual data unavailable" detail={synchronouslyResolved.errors.join(" · ")} />;
	if (traceResolution.status === "loading" && !lastKnownGoodProps) return <p className="visual-loading" role="status">Loading sealed trace…</p>;
	const liveFailed = traceResolution.status === "error";
	const selected = selectRenderedProjection({
		live: liveFailed ? null : { ...synchronouslyResolved.props, ...traceResolution.props },
		lastKnownGood: lastKnownGoodProps,
		liveFailed
	});
	if (liveFailed && !selected.projection) {
		const detail = traceResolution.error ?? "Trace projection resolution failed";
		const lower = detail.toLowerCase();
		const title = lower.includes("quarant") ? "Trace is quarantined"
			: lower.includes("extractor") || lower.includes("projection kind") || lower.includes("not registered") ? "Trace extractor unavailable"
				: lower.includes("unsupported") || lower.includes("schema") ? "Unsupported trace schema"
					: lower.includes("not found") || lower.includes("missing") || lower.includes("archive") ? "Sealed trace archive missing"
						: lower.includes("unavailable") ? "Trace resolver unavailable" : "Trace data unavailable";
		return <VisualInvalidState
			title={title}
			detail={traceResolution.failure?.message ?? detail}
			code={traceResolution.failure?.code}
			remediation={traceResolution.failure?.remediation}
			traceId={typeof traceBindings[0]?.source === "string" ? traceBindings[0].source : undefined}
		/>;
	}
	if (!Shell) return <p className="visual-loading">Loading visual shell…</p>;
	if (
		consumeInjectedRendererCrash(
			artifact.visualId ?? artifact.id,
			typeof artifact.revision === "number" ? artifact.revision : null,
			artifact.metadata?.__crashRenderer === true
		)
	) {
		throw new Error("injected renderer crash");
	}
	const resolvedProps = selected.projection ?? { ...synchronouslyResolved.props, ...traceResolution.props };
	const showConnection = Boolean(optimizerPayload || optimizerLoadError || connectionState !== "loading");
	const boundEvents = Array.isArray(optimizerPayload?.events) ? optimizerPayload.events as unknown[] : [];
	const boundStatus = typeof (optimizerPayload?.run as { status?: string } | undefined)?.status === "string"
		? (optimizerPayload?.run as { status?: string }).status ?? ""
		: "";
	const transportTerminal = connectionState === "terminal" || ["completed", "failed", "cancelled", "succeeded"].includes(boundStatus);
	return (
		<div
			data-testid="visual-template-shell"
			data-connection-state={showConnection ? connectionState : undefined}
			data-visual-transport-state={connectionState === "loading" ? "idle" : connectionState}
			data-visual-terminal={transportTerminal ? "true" : "false"}
			data-visual-semantic-event-count={String(boundEvents.length)}
			data-visual-rollout-count={String(boundEvents.length)}
			data-visual-error={optimizerLoadError ?? (liveFailed ? traceResolution.error : undefined)}
			data-visual-projection-source={selected.source ?? "live"}
			data-visual-projection-stale={selected.stale ? "true" : undefined}
			data-visual-subscription={connectionState}
			data-visual-compute={transportTerminal ? "terminal" : "running"}
			data-visual-review={artifact.status === "review" || artifact.status === "ready" ? artifact.status : "none"}
			data-visual-readiness={artifact.status === "ready" ? "ready" : "waiting"}
			data-visual-pinning={artifact.metadata?.pinned === true ? "pinned" : "unpinned"}
			data-visual-sealing={artifact.metadata?.sealed === true || artifact.metadata?.seal ? "sealed" : "unsealed"}
			data-visual-sharing={typeof artifact.metadata?.visibility === "string" ? String(artifact.metadata.visibility) : "private"}
			data-progress-phase={progressView?.phaseId}
			data-progress-phase-label={progressView?.phaseLabel}
			data-progress-status={progressView?.status}
			data-progress-completed={progressView?.completed != null ? String(progressView.completed) : undefined}
			data-progress-total={progressView?.total != null ? String(progressView.total) : undefined}
			data-progress-cost={progressView ? (progressView.costUsd == null ? "unavailable" : String(progressView.costUsd)) : undefined}
			data-progress-tokens={progressView ? (progressView.promptTokens == null ? "unavailable" : String(progressView.promptTokens)) : undefined}
			data-progress-terminal={progressView ? String(progressView.terminal) : undefined}
			data-progress-result={progressView?.resultHeadline ?? progressView?.resultAbsentReason}
		>
			{selected.stale ? (
				<p className="visual-stale-projection" role="status" data-testid="visual-last-known-good">
					Showing last known good projection while live rendering recovers.
				</p>
			) : null}
			{showConnection ? <p className="visual-connection-state" data-testid="visual-connection-state">{connectionState}</p> : null}
			<Shell
				{...(resolvedProps as ShellProps)}
				title={artifact.title}
				lede={artifact.summary}
				bindings={artifact.bindings}
				visualMetadata={artifact.metadata}
				loadError={optimizerLoadError ?? undefined}
				{...(optimizerPayload ?? {})}
				data={optimizerPayload ?? resolvedProps.optimizer_run}
				comparison={comparisonPayload ?? undefined}
				replay={replayClient}
				replayMissingTransport={replay.missingTransport}
				visualId={artifact.visualId ?? artifact.id}
				revision={typeof artifact.revision === "number" ? artifact.revision : null}
			/>
		</div>
	);
}

function numericAttribute(element: Element, name: string): number {
	const value = Number(element.getAttribute(name));
	return Number.isFinite(value) && value >= 0 ? value : 0;
}

/** The template reports runtime facts as DOM data, but Workshop owns the
 * extractor and readiness decision. Nothing supplied by a template is treated
 * as a passing boolean. */
function VisualObservationBoundary({ artifact, children }: { artifact: ArtifactRef; children: ReactNode }) {
	const root = useRef<HTMLDivElement>(null);
	const [bindingsDigest, setBindingsDigest] = useState<string | null>(null);
	const template = artifact.templateId ? resolveTemplate(artifact.templateId) : undefined;
	const contract = template?.observationContract;

	useEffect(() => {
		let cancelled = false;
		setBindingsDigest(null);
		if (!contract || !artifact.visualId || !artifact.revision || !bridges.visuals) return;
		void bridges.visuals.revisions(artifact.visualId).then((revisions) => {
			const digest = revisions.find((candidate) => candidate.revision === artifact.revision)?.bindingsDigest;
			if (!cancelled && digest) setBindingsDigest(digest);
		});
		return () => { cancelled = true; };
	}, [artifact.revision, artifact.visualId, contract]);

	useEffect(() => {
		const host = root.current;
		if (!host || !contract || !bindingsDigest || !artifact.visualId || !artifact.revision || !bridges.visuals) return;
		let frame: number | null = null;
		const publish = () => {
			frame = null;
			const surface = host.querySelector("[data-visual-transport-state]");
			if (!surface) return;
			const rawError = surface.getAttribute("data-visual-error")?.trim();
			void bridges.visuals?.reportObservation({
				schemaVersion: "synth.rendered-visual-observation.v1",
				visualId: artifact.visualId!,
				renderedRevision: artifact.revision!,
				bindingsDigest,
				transportState: surface.getAttribute("data-visual-transport-state") ?? "unknown",
				rolloutCount: numericAttribute(surface, "data-visual-rollout-count"),
				renderedFrameCount: numericAttribute(surface, "data-visual-rendered-frame-count"),
				semanticEventCount: numericAttribute(surface, "data-visual-semantic-event-count"),
				terminal: surface.getAttribute("data-visual-terminal") === "true",
				error: rawError || null,
				observedAt: new Date().toISOString()
			});
		};
		const schedule = () => {
			if (frame == null) frame = window.requestAnimationFrame(publish);
		};
		const observer = new MutationObserver(schedule);
		observer.observe(host, { subtree: true, childList: true, attributes: true });
		schedule();
		return () => {
			observer.disconnect();
			if (frame != null) window.cancelAnimationFrame(frame);
		};
	}, [artifact.revision, artifact.visualId, bindingsDigest, contract]);

	return <div ref={root} data-visual-observation-contract={contract?.schemaVersion}>{children}</div>;
}

/** A failed pane still has to be diagnosable. The sentence goes on top; the
 * stable code, the trace identity, and the remediation go underneath, because
 * "Trace data unavailable" alone sent agents into blind capture retries. */
function VisualInvalidState({ title, detail, code, remediation, traceId, onRetry }: {
	title: string;
	detail: string;
	code?: string;
	remediation?: string;
	traceId?: string;
	onRetry?: () => void;
}) {
	return (
		<div className="visual-invalid" role="alert" data-testid="visual-invalid" data-error-code={code}>
			<strong>{title}</strong>
			<p>{detail}</p>
			{remediation ? <p className="visual-invalid-remediation">{remediation}</p> : null}
			{onRetry ? <button type="button" className="visual-invalid-retry" onClick={onRetry}>Retry</button> : null}
			{code || traceId ? (
				<p className="visual-invalid-identity">
					{code ? <code data-testid="visual-invalid-code">{code}</code> : null}
					{traceId ? <code data-testid="visual-invalid-trace">{traceId}</code> : null}
				</p>
			) : null}
		</div>
	);
}

class VisualErrorBoundary extends Component<
	{ children: ReactNode; visualId?: string; visualRevision?: number | null; templateId?: string | null },
	{ error: Error | null; retry: number }
> {
	state: { error: Error | null; retry: number } = { error: null, retry: 0 };
	static getDerivedStateFromError(error: Error) { return { error }; }
	componentDidUpdate(prevProps: VisualErrorBoundary["props"]) {
		if (prevProps.visualRevision !== this.props.visualRevision && this.state.error) {
			this.setState({ error: null });
		}
	}
	componentDidCatch(error: Error, info: ErrorInfo) {
		// `console.error` reaches a devtools console nobody has open. The
		// structured record is what the agent can actually query, so the
		// boundary emits both.
		console.error("Visual shell render failed", error, info.componentStack);
		reportDiagnostic({
			severity: "error",
			component: "visual-host",
			event: "visual.render.failed",
			code: DIAGNOSTIC_CODES.visualRenderFailed,
			message: error.message,
			visualId: this.props.visualId ?? null,
			visualRevision: this.props.visualRevision ?? null,
			details: {
				templateId: this.props.templateId ?? null,
				componentStack: info.componentStack?.slice(0, 1_000) ?? null,
			},
		});
	}
	render() {
		if (this.state.error) {
			const presented = toPublicError(this.state.error, "Visual failed to render");
			return <VisualInvalidState
				title="Visual failed to render"
				detail={presented.message}
				code={presented.code}
				remediation={presented.remediation}
				onRetry={() => this.setState((current) => ({ error: null, retry: current.retry + 1 }))}
			/>;
		}
		return <div className="visual-host-boundary" key={this.state.retry}>{this.props.children}</div>;
	}
}

/** Shared host used by chat cards, the right pane, and the Visuals library. */
export function VisualHost({ artifact }: { artifact: ArtifactRef }) {
	const bindingsKey = bindingAuthorityKey(artifact.bindings);
	const isSystemsDynamic =
		artifact.templateId === "diagram.systems.dynamic.v1" || artifact.rendererKind === "systems-dynamic";
	if (isSystemsDynamic) {
		return <VisualErrorBoundary key={`${artifact.id}:systems-dynamic`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}><SystemsDynamicVisual artifact={artifact} /></VisualErrorBoundary>;
	}
	const isSystems = artifact.templateId === "diagram.systems.v1" || artifact.rendererKind === "systems";
	if (isSystems) {
		return <VisualErrorBoundary key={`${artifact.id}:systems`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}><SystemsMapVisual artifact={artifact} /></VisualErrorBoundary>;
	}
	const isMermaid =
		artifact.templateId === "diagram.mermaid.v1" || artifact.rendererKind === "mermaid";
	if (isMermaid) {
		return (
			<VisualErrorBoundary key={`${artifact.id}:mermaid`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}>
				<MermaidVisual artifact={artifact} />
			</VisualErrorBoundary>
		);
	}
	if (artifact.templateId === "synth.subagents.v1") {
		return (
			<VisualErrorBoundary key={`${artifact.id}:${artifact.templateId ?? "subagents"}`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}>
				<SubagentsVisual artifact={artifact} />
			</VisualErrorBoundary>
		);
	}
	if (artifact.preview?.variant && artifact.preview.variant !== "generic" && !artifact.templateId) {
		return (
			<VisualErrorBoundary key={`${artifact.id}:preview`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}>
				<MockFallback artifact={artifact} />
			</VisualErrorBoundary>
		);
	}
	return (
		<VisualErrorBoundary key={`${artifact.id}:${artifact.templateId ?? "missing"}:${bindingsKey}`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}>
			<VisualObservationBoundary artifact={artifact}>
				<TemplateVisualHost artifact={artifact} />
			</VisualObservationBoundary>
		</VisualErrorBoundary>
	);
}

export function VisualPane({ artifact, onClose }: { artifact: ArtifactRef; onClose: () => void }) {
	const [expanded, setExpanded] = useState(false);
	const [annotations, setAnnotations] = useState<VisualAnnotation[]>([]);
	const [seals, setSeals] = useState<VisualSeal[]>([]);
	const [sealedBundle, setSealedBundle] = useState<VisualSealBundle | null>(null);
	const [compareBundle, setCompareBundle] = useState<VisualSealBundle | null>(null);
	const [shareUpload, setShareUpload] = useState<VisualUpload | null>(null);
	const [sharedUrl, setSharedUrl] = useState("");
	const [labeling, setLabeling] = useState(false);
	const [labelPoint, setLabelPoint] = useState<{ x: number; y: number } | null>(null);
	const [labelBody, setLabelBody] = useState("");
	const [artifactError, setArtifactError] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const visualId = artifact.visualId;
	const revision = artifact.revision;
	const qualityGate = artifact.metadata?.qualityGate as { ready?: boolean; revision?: number } | undefined;
	const sealEligible = Boolean(visualId && revision && qualityGate?.ready && qualityGate.revision === revision);

	useEffect(() => {
		let cancelled = false;
		if (!visualId || !bridges.visuals) return;
		void Promise.all([bridges.visuals.annotations(visualId), bridges.visuals.listSeals(visualId)])
			.then(([nextAnnotations, nextSeals]) => {
				if (!cancelled) {
					setAnnotations(nextAnnotations.filter((row) => !row.tombstoned));
					setSeals(nextSeals);
				}
			})
			.catch((reason) => { if (!cancelled) setArtifactError(publicError(reason)); });
		return () => { cancelled = true; };
	}, [visualId, revision]);

	async function createLabel() {
		if (!visualId || !revision || !labelPoint || !bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const annotation = await bridges.visuals.createAnnotation(visualId, {
				visualRevision: revision,
				selector: { type: "chart_mark", markId: "visual-pane", x: labelPoint.x, y: labelPoint.y },
				kind: "note",
				body: labelBody.trim() || null,
				metadata: { coordinateSpace: "normalized", createdFrom: "visual-pane" }
			});
			setAnnotations((current) => [...current, annotation]);
			setLabeling(false);
			setLabelPoint(null);
			setLabelBody("");
		} catch (reason) {
			setArtifactError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}

	async function sealCurrentRevision() {
		if (!visualId || !revision || !bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const nextSeal = await bridges.visuals.seal(visualId, revision);
			setSeals((current) => [nextSeal, ...current.filter((row) => row.receiptDigest !== nextSeal.receiptDigest)]);
			setSealedBundle(await bridges.visuals.getSeal(nextSeal.receiptDigest));
		} catch (reason) {
			setArtifactError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}

	async function reopenSeal(receiptDigest: string) {
		if (!bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const [bundle, upload] = await Promise.all([
				bridges.visuals.getSeal(receiptDigest),
				bridges.visuals.uploadStatus(receiptDigest)
			]);
			setSealedBundle(bundle);
			setCompareBundle(null);
			setShareUpload(upload);
		} catch (reason) {
			setArtifactError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}

	async function compareSeal(receiptDigest: string) {
		if (!bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const bundle = await bridges.visuals.getSeal(receiptDigest);
			if (!sealedBundle) setSealedBundle(bundle);
			else setCompareBundle(bundle);
		} catch (reason) {
			setArtifactError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}

	async function openSharedUrl() {
		if (!bridges.visuals || !sharedUrl.trim()) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const bundle = await bridges.visuals.openShared(sharedUrl.trim());
			setSealedBundle(bundle);
			setCompareBundle(null);
			setShareUpload(null);
		} catch (reason) {
			setArtifactError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}

	async function shareCurrentSeal() {
		if (!sealedBundle || !bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const upload = await bridges.visuals.shareSeal(sealedBundle.seal.receiptDigest);
			setShareUpload(upload);
			if (upload.committedUrl) await navigator.clipboard?.writeText(upload.committedUrl).catch(() => undefined);
		} catch (reason) {
			setArtifactError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}
	const isSubagents = artifact.templateId === "synth.subagents.v1";
	const isMermaid = artifact.templateId === "diagram.mermaid.v1" || artifact.rendererKind === "mermaid";
	const isSystemsDynamic = artifact.templateId === "diagram.systems.dynamic.v1" || artifact.rendererKind === "systems-dynamic";
	const isSystems = artifact.templateId === "diagram.systems.v1" || artifact.rendererKind === "systems";
	const kindLabel = isSubagents ? "Agents" : isSystemsDynamic ? "Benjamin Dicken Style" : isSystems ? "Systems map · 2D" : isMermaid ? "Diagram" : "Visual";
	const revisionSync = artifact.metadata?.revisionSync as {
		loading?: boolean;
		requestedRevision?: number;
		acceptedRevision?: number;
		error?: string | null;
	} | undefined;
	return (
		<aside
			className={`visual-pane${expanded ? " visual-pane-expanded" : ""}`}
			data-testid="visual-pane"
			aria-label={isSubagents ? "Subagents" : "Visual artifact"}
		>
			<header className="visual-pane-head">
				<div className="visual-pane-head-text">
					<span className="visual-pane-kind">
						{kindLabel}{revision ? ` · rev ${revision}` : ""}
						{revisionSync?.loading ? ` · reconciling${(revisionSync.requestedRevision ?? -1) > (revisionSync.acceptedRevision ?? -1) ? ` rev ${revisionSync.requestedRevision}` : ""}` : ""}
					</span>
					<span className="visual-pane-title">{artifact.title}</span>
				</div>
				<div className="visual-pane-head-actions">
					{isSubagents ? null : sealedBundle ? (
						<>
							<button type="button" className="visual-expand" onClick={() => { setSealedBundle(null); setCompareBundle(null); setShareUpload(null); }}>Live revision</button>
							{compareBundle ? <button type="button" className="visual-expand" onClick={() => setCompareBundle(null)}>Close comparison</button> : null}
							<button type="button" className="visual-expand" onClick={() => void shareCurrentSeal()} disabled={busy} title="Human Share uploads this sealed digest privately">
								{shareUpload?.state === "committed" ? "Shared privately" : "Share privately"}
							</button>
						</>
					) : null}
					{isSubagents ? null : (
					<button
						type="button"
						className="visual-expand"
						onClick={() => { setLabeling(true); setLabelPoint(null); }}
						disabled={!visualId || !revision || busy}
						title="Place a durable label on this exact revision"
					>
						Label{annotations.length ? ` · ${annotations.length}` : ""}
					</button>
					)}
					{isSubagents ? null : (
					<button
						type="button"
						className="visual-expand"
						onClick={() => void sealCurrentRevision()}
						disabled={!sealEligible || busy}
						title={sealEligible ? "Seal this exact revision for offline use" : "Pass the E1 visual quality gate before sealing"}
					>
						{busy ? "Working…" : "Seal"}
					</button>
					)}
					<button
						type="button"
						className="visual-expand"
						onClick={() => setExpanded((current) => !current)}
						aria-pressed={expanded}
						aria-label={expanded ? "Restore split view" : "Expand visual"}
						data-testid="toggle-visual-expand"
					>
						{expanded ? "Restore" : "Expand"}
					</button>
					<button type="button" className="visual-close" onClick={onClose} aria-label="Close visual">×</button>
				</div>
			</header>
			{artifactError || revisionSync?.error ? <div className="visual-artifact-error" role="alert">{artifactError ?? `Visual refresh failed · ${revisionSync?.error}`}</div> : null}
			{seals.length ? (
				<div className="visual-seal-strip" aria-label="Sealed revisions">
					<span>Offline:</span>
					{seals.map((seal) => (
						<span key={seal.receiptDigest} className="visual-seal-choice">
							<button type="button" onClick={() => void reopenSeal(seal.receiptDigest)}>
								rev {seal.visualRevision} · {seal.receiptDigest.slice(0, 8)}
							</button>
							{sealedBundle?.seal.receiptDigest !== seal.receiptDigest ? (
								<button type="button" onClick={() => void compareSeal(seal.receiptDigest)}>Compare</button>
							) : null}
						</span>
					))}
				</div>
			) : null}
			<form className="visual-shared-open" onSubmit={(event) => { event.preventDefault(); void openSharedUrl(); }}>
				<input
					value={sharedUrl}
					onChange={(event) => setSharedUrl(event.target.value)}
					placeholder="Paste private artifact URL"
					aria-label="Private artifact URL"
				/>
				<button type="submit" disabled={!sharedUrl.trim() || busy}>Open shared</button>
			</form>
			{shareUpload?.committedUrl ? (
				<div className="visual-share-url">
					<span>Private permalink</span>
					<a href={shareUpload.committedUrl} target="_blank" rel="noreferrer">{shareUpload.committedUrl}</a>
					<button type="button" onClick={() => void navigator.clipboard?.writeText(shareUpload.committedUrl!)}>Copy</button>
				</div>
			) : null}
			{labeling ? (
				<form className="visual-label-form" onSubmit={(event) => { event.preventDefault(); void createLabel(); }}>
					<span>{labelPoint ? `Placed at ${Math.round(labelPoint.x * 100)}%, ${Math.round(labelPoint.y * 100)}%` : "Click the visual to place the label."}</span>
					<input value={labelBody} onChange={(event) => setLabelBody(event.target.value)} placeholder="Label note (optional)" aria-label="Label note" />
					<button type="submit" disabled={!labelPoint || busy}>Save label</button>
					<button type="button" onClick={() => { setLabeling(false); setLabelPoint(null); }}>Cancel</button>
				</form>
			) : null}
			<div
				className={`visual-pane-body${labeling ? " visual-label-target" : ""}`}
				onClick={labeling ? (event) => {
					const bounds = event.currentTarget.getBoundingClientRect();
					setLabelPoint({
						x: Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)),
						y: Math.max(0, Math.min(1, (event.clientY - bounds.top) / bounds.height))
					});
				} : undefined}
			>
				{sealedBundle ? (
					<div className={compareBundle ? "visual-sealed-compare" : "visual-sealed-single"}>
						<iframe
							className="visual-sealed-frame"
							title={`Sealed ${artifact.title} revision ${sealedBundle.seal.visualRevision}`}
							sandbox=""
							srcDoc={sealedBundle.indexHtml}
							data-receipt-digest={sealedBundle.seal.receiptDigest}
						/>
						{compareBundle ? (
							<iframe
								className="visual-sealed-frame"
								title={`Sealed ${artifact.title} revision ${compareBundle.seal.visualRevision}`}
								sandbox=""
								srcDoc={compareBundle.indexHtml}
								data-receipt-digest={compareBundle.seal.receiptDigest}
							/>
						) : null}
					</div>
				) : <VisualHost artifact={artifact} />}
			</div>
		</aside>
	);
}
