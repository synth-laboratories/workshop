import { Component, useEffect, useMemo, useState, type ComponentType, type ErrorInfo, type ReactNode } from "react";
import type { ArtifactRef } from "../types/landing";
import type { VisualRecord } from "@synth/runtime-protocol";
import { propsFromBindings } from "@synth/visuals";
import { loadVisualShell } from "../runtime/visualsLoader";

type ShellProps = {
	title?: string;
	lede?: string;
	bindings?: Record<string, unknown>;
	[key: string]: unknown;
};

type SubagentRow = {
	id: string;
	title: string;
	summary?: string;
	status: "active" | "done" | "failed";
	startedAt: string;
	updatedAt: string;
};

export function artifactFromVisualRecord(visual: VisualRecord): ArtifactRef {
	return {
		id: visual.id,
		kind: "report",
		title: visual.title,
		templateId: visual.templateId,
		visualId: visual.id,
		bindings: visual.bindings,
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

function SubagentsVisual({ artifact }: { artifact: ArtifactRef }) {
	const resolved = propsFromBindings(artifact.bindings);
	const agents = Array.isArray(resolved.props.agents) ? resolved.props.agents as SubagentRow[] : [];
	const [now, setNow] = useState(Date.now());
	useEffect(() => {
		const timer = window.setInterval(() => setNow(Date.now()), 1_000);
		return () => window.clearInterval(timer);
	}, []);
	const groups = [
		{ label: "Active", agents: agents.filter((agent) => agent.status === "active") },
		{ label: "Done", agents: agents.filter((agent) => agent.status !== "active") }
	];
	return (
		<div className="subagents-visual" data-testid="visual-subagents">
			{groups.map((group) => (
				<section key={group.label} className="subagents-group">
					<h3>{group.label} · {group.agents.length}</h3>
					{group.agents.length === 0 ? <p className="subagents-empty">No {group.label.toLowerCase()} subagents</p> : null}
					{group.agents.map((agent, index) => (
						<div className="subagent-row" key={agent.id} data-status={agent.status}>
							<span className={`subagent-mark mark-${agent.status}`} aria-hidden>{index % 2 ? "✣" : "✺"}</span>
							<div className="subagent-copy">
								<strong>{agent.title}</strong>
								{agent.summary ? <p>{agent.summary}</p> : null}
							</div>
							<time dateTime={agent.updatedAt}>{agent.status === "active" ? elapsedLabel(agent.startedAt, now) : elapsedLabel(agent.updatedAt, now) + " ago"}</time>
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
	const isSubagents = artifact.templateId === "synth.subagents.v1";
	return (
		<aside className="visual-pane" data-testid="visual-pane" aria-label={isSubagents ? "Subagents" : "Visual artifact"}>
			<header className="visual-pane-head">
				<div className="visual-pane-head-text">
					<span className="visual-pane-kind">{isSubagents ? "Agents" : "Visual"}</span>
					<span className="visual-pane-title">{artifact.title}</span>
				</div>
				<button type="button" className="visual-close" onClick={onClose} aria-label="Close visual">×</button>
			</header>
			<div className="visual-pane-body">
				<VisualHost artifact={artifact} />
			</div>
		</aside>
	);
}
