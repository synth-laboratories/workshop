import { Component, useEffect, useMemo, useState, type ComponentType, type ErrorInfo, type ReactNode } from "react";
import type { ArtifactRef } from "../types/landing";
import type { VisualRecord } from "@synth/runtime-protocol";
import { propsFromBindings } from "@synth/visuals";
import { loadVisualShell } from "../runtime/visualsLoader";
import { bridges } from "../runtime/desktopBridge";
import { mergeOptimizerEventPage, type OptimizerEventCursorState } from "../runtime/optimizerEventCursor";
import { MermaidVisual } from "./MermaidVisual";
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
	const [now, setNow] = useState(Date.now());
	useEffect(() => {
		const timer = window.setInterval(() => setNow(Date.now()), 1_000);
		return () => window.clearInterval(timer);
	}, []);
	const groups = [
		{ label: "Working", agents: agents.filter((agent) => agent.status === "starting" || agent.status === "working") },
		{ label: "Needs attention", agents: agents.filter((agent) => agent.status === "interrupted" || agent.status === "failed" || agent.status === "stopped" || agent.status === "unavailable") },
		{ label: "Completed", agents: agents.filter((agent) => agent.status === "completed") }
	];
	return (
		<div className="subagents-visual" data-testid="visual-subagents">
			{groups.map((group) => (
				<section key={group.label} className="subagents-group">
					<h3>{group.label} · {group.agents.length}</h3>
					{group.agents.length === 0 ? <p className="subagents-empty">No {group.label.toLowerCase()} subagents</p> : null}
					{group.agents.map((agent) => (
						<div className="subagent-row" key={agent.id} data-status={agent.status}>
							<span className={`subagent-mark mark-${agent.status}`} aria-hidden>{subagentMarker(agent.id)}</span>
							<div className="subagent-copy">
								<div className="subagent-title-row"><strong>{agent.title}</strong><span className={`subagent-state state-${agent.status}`}>{subagentStatusLabel(agent.status)}</span></div>
								{agent.summary ? <p>{agent.summary}</p> : null}
							</div>
							<time dateTime={agent.updatedAt}>{agent.status === "starting" || agent.status === "working" ? elapsedLabel(agent.startedAt, now) : elapsedLabel(agent.updatedAt, now) + " ago"}</time>
						</div>
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
	const resolved = useMemo(() => propsFromBindings(artifact.bindings), [artifact.bindings]);

	useEffect(() => {
		let cancelled = false;
		const templateId = artifact.templateId;
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
		const bindings = artifact.bindings as { slots?: Array<{ slot?: string; kind?: string; source?: string }> } | undefined;
		const slot = bindings?.slots?.find((entry) => entry.slot === "optimizer_run" && entry.kind === "optimizer_run");
		const optimizerRunId = slot?.source;
		if (!optimizerRunId || !bridges.optimizers) {
			setOptimizerPayload(null);
			setOptimizerLoadError(optimizerRunId ? "Optimizer bridge is unavailable" : null);
			return;
		}
		const pageSize = 500;
		let current: OptimizerEventCursorState = { events: [], cursor: 0, gap: false };
		let pending = Promise.resolve();
		let stopPolling = false;
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
				const run = await bridges.optimizers!.get(optimizerRunId);
				let next = await readPersistedEvents(snapshot ? 0 : current.cursor);
				const runCursor = typeof run.cursorSeq === "number" ? run.cursorSeq : next.cursor;
				if (!snapshot && (next.gap || runCursor < current.cursor || next.cursor < runCursor)) {
					// A missed notification, truncated page, or replaced local import requires
					// a durable snapshot reload. Never patch over a sequence hole.
					next = await readPersistedEvents(0);
				}
				if (next.gap || next.cursor < runCursor) {
					throw new Error(`Optimizer event history is incomplete at ${next.cursor}/${runCursor}`);
				}
				current = next;
				if (typeof run.status === "string" && terminal.has(run.status)) {
					stopPolling = true;
				}
				if (!cancelled) {
					setOptimizerPayload({ run, events: current.events });
					setOptimizerLoadError(null);
				}
			} catch (reason) {
				if (!cancelled) {
					setOptimizerPayload(null);
					setOptimizerLoadError(reason instanceof Error ? reason.message : String(reason));
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
	}, [artifact.bindings]);

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
	if (resolved.errors.length > 0) return <VisualInvalidState title="Visual data unavailable" detail={resolved.errors.join(" · ")} />;
	if (!Shell) return <p className="visual-loading">Loading visual shell…</p>;
	return (
		<div data-testid="visual-template-shell">
			<Shell
				{...(resolved.props as ShellProps)}
				title={artifact.title}
				lede={artifact.summary}
				bindings={artifact.bindings}
				visualMetadata={artifact.metadata}
				loadError={optimizerLoadError ?? undefined}
				{...(optimizerPayload ?? {})}
				data={optimizerPayload ?? resolved.props.optimizer_run}
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
	const isSubagents = artifact.templateId === "synth.subagents.v1";
	const isMermaid = artifact.templateId === "diagram.mermaid.v1" || artifact.rendererKind === "mermaid";
	const kindLabel = isSubagents ? "Agents" : isMermaid ? "Diagram" : "Visual";
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
			<div className="visual-pane-body">
				<VisualHost artifact={artifact} />
			</div>
		</aside>
	);
}
