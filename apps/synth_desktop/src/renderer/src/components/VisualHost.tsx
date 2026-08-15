import { Component, useEffect, useMemo, useState, type ComponentType, type ErrorInfo, type ReactNode } from "react";
import type { ArtifactRef } from "../types/landing";
import type { VisualRecord } from "@synth/runtime-protocol";
import { bindTemplateSlots, bindingSlots, isVisualBindings, propsFromBindings, resolveTemplate } from "@synth/visuals";
import type { VisualAnnotation, VisualSeal, VisualSealBundle, VisualUpload } from "../bridge";
import { loadVisualShell } from "../runtime/visualsLoader";
import { bridges } from "../runtime/desktopBridge";
import { mergeOptimizerEventPage, type OptimizerEventCursorState } from "../runtime/optimizerEventCursor";
import { MermaidVisual } from "./MermaidVisual";
import { SystemsMapVisual } from "./SystemsMapVisual";
import { SystemsDynamicVisual } from "./SystemsDynamicVisual";
import type { SubagentState } from "../runtime/sessionView";

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
					setDetailError(reason instanceof Error ? reason.message : String(reason));
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
	const traceBindings = useMemo(
		() => bindingSlots(artifact.bindings).filter((binding) => binding.kind === "trace_v5"),
		[artifact.bindings]
	);
	const synchronouslyResolved = useMemo(() => {
		if (!isVisualBindings(artifact.bindings) || traceBindings.length === 0) {
			return propsFromBindings(artifact.bindings);
		}
		return propsFromBindings({
			schemaVersion: "synth.visual-bindings.v1",
			slots: artifact.bindings.slots.filter((binding) => binding.kind !== "trace_v5")
		});
	}, [artifact.bindings, traceBindings.length]);
	const [traceResolution, setTraceResolution] = useState<{
		status: "idle" | "loading" | "ready" | "error";
		props: Record<string, unknown>;
		error?: string;
	}>({ status: "idle", props: {} });
	const [connectionState, setConnectionState] = useState<
		"loading" | "replaying" | "subscribed" | "stale" | "reconnecting" | "terminal" | "failed"
	>("loading");

	useEffect(() => {
		let cancelled = false;
		const templateId = artifact.templateId;
		setFailed(false);
		setShell(null);
		if (!templateId) {
			setFailed(true);
			return;
		}
		void loadVisualShell(templateId)
			.then((Component) => {
				if (cancelled) return;
				if (!Component) setFailed(true);
				else setShell(() => Component);
			})
			.catch(() => {
				if (!cancelled) setFailed(true);
			});
		return () => { cancelled = true; };
	}, [artifact.templateId]);

	useEffect(() => {
		let cancelled = false;
		if (traceBindings.length === 0 || !isVisualBindings(artifact.bindings)) {
			setTraceResolution({ status: "idle", props: {} });
			return () => { cancelled = true; };
		}
		const bindings = artifact.bindings;
		const template = artifact.templateId ? resolveTemplate(artifact.templateId) : undefined;
		if (!template) {
			setTraceResolution({ status: "error", props: {}, error: `Template ${artifact.templateId ?? "unknown"} is unavailable` });
			return () => { cancelled = true; };
		}
		if (!bridges.inventory) {
			setTraceResolution({ status: "error", props: {}, error: "Trace projection resolver is unavailable" });
			return () => { cancelled = true; };
		}
		const unsupportedBinding = traceBindings.find((binding) =>
			binding.schema && binding.schema !== "synth.trace-projection.rollout-inspector.v1"
		);
		if (unsupportedBinding) {
			setTraceResolution({ status: "error", props: {}, error: `Unsupported trace projection schema: ${unsupportedBinding.schema}` });
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
		void bindTemplateSlots(template, bindings, { loadTraceV5, skipOptional: true })
			.then((result) => {
				if (cancelled) return;
				if (result.errors.length > 0) {
					setTraceResolution({ status: "error", props: {}, error: result.errors.join(" · ") });
					return;
				}
				const props = Object.fromEntries(
					Object.values(result.slots)
						.filter((slot) => slot.kind === "trace_v5")
						.map((slot) => [slot.slot, slot.data])
				);
				setTraceResolution({ status: "ready", props });
			})
			.catch((reason) => {
				if (!cancelled) setTraceResolution({ status: "error", props: {}, error: reason instanceof Error ? reason.message : String(reason) });
			});
		return () => { cancelled = true; };
	}, [artifact.id, artifact.revision, artifact.templateId, artifact.bindings, traceBindings.length]);

	useEffect(() => {
		let cancelled = false;
		const bindings = artifact.bindings as { slots?: Array<{ slot?: string; kind?: string; source?: string }> } | undefined;
		const slot = bindings?.slots?.find((entry) => entry.slot === "optimizer_run" && entry.kind === "optimizer_run");
		const optimizerRunId = slot?.source;
		if (!optimizerRunId || !bridges.optimizers) {
			setOptimizerPayload(null);
			setOptimizerLoadError(optimizerRunId ? "Optimizer bridge is unavailable" : null);
			if (optimizerRunId) setConnectionState("failed");
			return;
		}
		const pageSize = 500;
		let current: OptimizerEventCursorState = { events: [], cursor: 0, gap: false };
		let pending = Promise.resolve();
		let stopPolling = false;
		let postedReady = false;
		const terminal = new Set(["completed", "failed", "cancelled", "succeeded"]);
		const readPersistedEvents = async (after: number) => {
			let state: OptimizerEventCursorState = {
				events: after === 0 ? [] : current.events,
				cursor: after,
				gap: false
			};
			for (;;) {
				const page = await bridges.optimizers!.eventsAfter(optimizerRunId, state.cursor, pageSize);
				if (!Array.isArray(page) || page.length === 0) return state;
				const before = state.cursor;
				state = mergeOptimizerEventPage(state, page);
				if (state.gap || state.cursor === before || page.length < pageSize) return state;
			}
		};
		const load = async (snapshot = false) => {
			try {
				if (!snapshot) setConnectionState((current) => current === "subscribed" ? "reconnecting" : current);
				else setConnectionState("replaying");
				const run = await bridges.optimizers!.get(optimizerRunId);
				let next = await readPersistedEvents(snapshot ? 0 : current.cursor);
				const runCursor = typeof run.cursorSeq === "number" ? run.cursorSeq : next.cursor;
				if (!snapshot && (next.gap || runCursor < current.cursor || next.cursor < runCursor)) {
					// A missed notification, truncated page, or replaced local import requires
					// a durable snapshot reload. Never patch over a sequence hole.
					next = await readPersistedEvents(0);
				}
				if (next.gap || next.cursor < runCursor) {
					setConnectionState("stale");
					throw new Error(`Optimizer event history is incomplete at ${next.cursor}/${runCursor}`);
				}
				current = next;
				const runStatus = typeof run.status === "string" ? run.status : "";
				if (terminal.has(runStatus)) {
					stopPolling = true;
				}
				if (!cancelled) {
					setOptimizerPayload({ run, events: current.events });
					setOptimizerLoadError(null);
					setConnectionState(terminal.has(runStatus) ? "terminal" : "subscribed");
					if (!postedReady) {
						postedReady = true;
						void bridges.optimizers?.recordVisualReady?.({
							visualId: artifact.id,
							optimizerRunId,
							templateId: artifact.templateId ?? "optimizer.run.v1",
							replayedThrough: current.cursor,
							subscribedFrom: current.cursor + 1,
							templateDigest: typeof artifact.metadata?.templateDigest === "string"
								? artifact.metadata.templateDigest
								: undefined
						}).catch(() => undefined);
					}
				}
			} catch (reason) {
				if (!cancelled) {
					setOptimizerPayload(null);
					setOptimizerLoadError(reason instanceof Error ? reason.message : String(reason));
					setConnectionState("failed");
				}
			}
		};
		const enqueue = (snapshot = false) => {
			pending = pending.then(() => load(snapshot));
		};
		enqueue(true);
		const unlisten = bridges.optimizers.onEvent((event) => {
			const eventRunId = typeof event.payload?.optimizerRunId === "string"
				? event.payload.optimizerRunId
				: typeof event.payload?.optimizer_run_id === "string" ? event.payload.optimizer_run_id : null;
			if (!eventRunId || eventRunId === optimizerRunId) enqueue(false);
		});
		const poll = window.setInterval(() => {
			if (stopPolling) return;
			void bridges.optimizers!.refresh(optimizerRunId).catch(() => undefined);
		}, 750);
		return () => {
			cancelled = true;
			window.clearInterval(poll);
			unlisten?.();
		};
	}, [artifact.bindings, artifact.id, artifact.templateId, artifact.metadata]);

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
	if (synchronouslyResolved.errors.length > 0) return <VisualInvalidState title="Visual data unavailable" detail={synchronouslyResolved.errors.join(" · ")} />;
	if (traceResolution.status === "loading") return <p className="visual-loading" role="status">Loading sealed trace…</p>;
	if (traceResolution.status === "error") {
		const detail = traceResolution.error ?? "Trace projection resolution failed";
		const lower = detail.toLowerCase();
		const title = lower.includes("quarant") ? "Trace is quarantined"
			: lower.includes("extractor") || lower.includes("projection kind") || lower.includes("not registered") ? "Trace extractor unavailable"
				: lower.includes("unsupported") || lower.includes("schema") ? "Unsupported trace schema"
					: lower.includes("not found") || lower.includes("missing") || lower.includes("archive") ? "Sealed trace archive missing"
						: lower.includes("unavailable") ? "Trace resolver unavailable" : "Trace data unavailable";
		return <VisualInvalidState title={title} detail={detail} />;
	}
	if (!Shell) return <p className="visual-loading">Loading visual shell…</p>;
	const resolvedProps = { ...synchronouslyResolved.props, ...traceResolution.props };
	const showConnection = Boolean(optimizerPayload || optimizerLoadError || connectionState !== "loading");
	return (
		<div data-testid="visual-template-shell" data-connection-state={showConnection ? connectionState : undefined}>
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
			/>
		</div>
	);
}

function VisualInvalidState({ title, detail }: { title: string; detail: string }) {
	return <div className="visual-invalid" role="alert" data-testid="visual-invalid"><strong>{title}</strong><p>{detail}</p></div>;
}

class VisualErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
	state: { error: Error | null } = { error: null };
	static getDerivedStateFromError(error: Error) { return { error }; }
	componentDidCatch(error: Error, info: ErrorInfo) {
		console.error("Visual shell render failed", error, info.componentStack);
	}
	render() {
		if (this.state.error) return <VisualInvalidState title="Visual failed to render" detail={this.state.error.message} />;
		return <div className="visual-host-boundary">{this.props.children}</div>;
	}
}

/** Shared host used by chat cards, the right pane, and the Visuals library. */
export function VisualHost({ artifact }: { artifact: ArtifactRef }) {
	const isSystemsDynamic =
		artifact.templateId === "diagram.systems.dynamic.v1" || artifact.rendererKind === "systems-dynamic";
	if (isSystemsDynamic) {
		return <VisualErrorBoundary key={`${artifact.id}:systems-dynamic`}><SystemsDynamicVisual artifact={artifact} /></VisualErrorBoundary>;
	}
	const isSystems = artifact.templateId === "diagram.systems.v1" || artifact.rendererKind === "systems";
	if (isSystems) {
		return <VisualErrorBoundary key={`${artifact.id}:systems`}><SystemsMapVisual artifact={artifact} /></VisualErrorBoundary>;
	}
	const isMermaid =
		artifact.templateId === "diagram.mermaid.v1" || artifact.rendererKind === "mermaid";
	if (isMermaid) {
		return (
			<VisualErrorBoundary key={`${artifact.id}:mermaid`}>
				<MermaidVisual artifact={artifact} />
			</VisualErrorBoundary>
		);
	}
	if (artifact.templateId === "synth.subagents.v1") {
		return (
			<VisualErrorBoundary key={`${artifact.id}:${artifact.templateId ?? "subagents"}`}>
				<SubagentsVisual artifact={artifact} />
			</VisualErrorBoundary>
		);
	}
	if (artifact.preview?.variant && artifact.preview.variant !== "generic" && !artifact.templateId) {
		return (
			<VisualErrorBoundary key={`${artifact.id}:preview`}>
				<MockFallback artifact={artifact} />
			</VisualErrorBoundary>
		);
	}
	return (
		<VisualErrorBoundary key={`${artifact.id}:${artifact.templateId ?? "missing"}`}>
			<TemplateVisualHost artifact={artifact} />
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
			.catch((reason) => { if (!cancelled) setArtifactError(String(reason)); });
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
			setArtifactError(String(reason));
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
			setArtifactError(String(reason));
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
			setArtifactError(String(reason));
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
			setArtifactError(String(reason));
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
			setArtifactError(String(reason));
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
			setArtifactError(String(reason));
		} finally {
			setBusy(false);
		}
	}
	const isSubagents = artifact.templateId === "synth.subagents.v1";
	const isMermaid = artifact.templateId === "diagram.mermaid.v1" || artifact.rendererKind === "mermaid";
	const isSystemsDynamic = artifact.templateId === "diagram.systems.dynamic.v1" || artifact.rendererKind === "systems-dynamic";
	const isSystems = artifact.templateId === "diagram.systems.v1" || artifact.rendererKind === "systems";
	const kindLabel = isSubagents ? "Agents" : isSystemsDynamic ? "Benjamin Dicken Style" : isSystems ? "Systems map · 2D" : isMermaid ? "Diagram" : "Visual";
	return (
		<aside
			className={`visual-pane${expanded ? " visual-pane-expanded" : ""}`}
			data-testid="visual-pane"
			aria-label={isSubagents ? "Subagents" : "Visual artifact"}
		>
			<header className="visual-pane-head">
				<div className="visual-pane-head-text">
					<span className="visual-pane-kind">{kindLabel}</span>
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
			{artifactError ? <div className="visual-artifact-error" role="alert">{artifactError}</div> : null}
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
