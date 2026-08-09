import { useEffect, useState, type ComponentType } from "react";
import type { ArtifactRef } from "../types/landing";
import { loadVisualShell } from "../runtime/visualsLoader";

type Props = {
	artifact: ArtifactRef;
	onClose: () => void;
};

type ShellProps = {
	title?: string;
	lede?: string;
	bindings?: Record<string, unknown>;
	[key: string]: unknown;
};

/** Mock of https://www.usesynth.ai/evals/craftax — native Desktop visual. */
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
						<defs>
							<linearGradient id="paretoFill" x1="0" y1="1" x2="0" y2="0">
								<stop offset="0%" stopColor="rgba(240,95,34,0.08)" />
								<stop offset="100%" stopColor="rgba(240,95,34,0)" />
							</linearGradient>
						</defs>
						{[40, 80, 120, 160].map((y) => (
							<line key={y} x1="36" y1={y} x2="300" y2={y} stroke="#e8eaee" strokeWidth="1" />
						))}
						{[80, 140, 200, 260].map((x) => (
							<line key={x} x1={x} y1="20" x2={x} y2="168" stroke="#eef0f3" strokeWidth="1" />
						))}
						<path
							d="M70 150 C 110 120, 150 95, 190 70 S 250 48, 280 40"
							fill="none"
							stroke="rgba(240,95,34,0.45)"
							strokeWidth="2"
						/>
						{models.map((m, i) => {
							const x = 70 + i * 45;
							const y = 155 - m.ach * 9;
							return (
								<g key={m.name}>
									<circle
										cx={x}
										cy={y}
										r={m.accent ? 8 : 6}
										fill={m.accent ? "#f05f22" : "#5c6573"}
									/>
									<text x={x} y={y - 12} textAnchor="middle" className="pareto-label">
										{m.name}
									</text>
								</g>
							);
						})}
						<text x="168" y="192" textAnchor="middle" className="pareto-axis">
							inference cost / rollout
						</text>
						<text
							x="14"
							y="100"
							textAnchor="middle"
							className="pareto-axis"
							transform="rotate(-90 14 100)"
						>
							achievements
						</text>
					</svg>
				</div>
				<div className="visual-metrics">
					{(artifact.preview?.metrics ?? [
						{ label: "Laguna XS", value: "11.4 ach" },
						{ label: "$ / rollout", value: "$0.12" },
						{ label: "vs Flash", value: "+2.1" }
					]).map((m) => (
						<div key={m.label} className="visual-metric">
							<span>{m.label}</span>
							<strong>{m.value}</strong>
						</div>
					))}
				</div>
			</section>

			<section className="craftax-section">
				<div className="craftax-section-head">
					<h3>Per-achievement breakdown</h3>
					<span>66 achievements · mock slice</span>
				</div>
				<div className="achievement-matrix" aria-label="Achievement matrix">
					{achievements.map((row, ri) => (
						<div key={ri} className="achievement-row">
							{row.map((cell, ci) => {
								const heat = ((ri * 6 + ci) % 7) / 6;
								return (
									<div
										key={cell}
										className="achievement-tile"
										style={{
											background: `rgba(240, 95, 34, ${0.12 + heat * 0.72})`,
											color: heat > 0.45 ? "#fff" : "#5a3a28"
										}}
										title={cell}
									>
										{cell}
									</div>
								);
							})}
						</div>
					))}
				</div>
				<div className="family-legend">
					{["Survival", "Gathering", "Mining", "Crafting", "Combat", "Explore"].map((f) => (
						<span key={f}>{f}</span>
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
				<p className="env-caption">
					Player (12,17) · HP 7 · wood 3 · zombie 4E — accessible projection beside canvas
				</p>
			</div>
		</div>
	);
}

function MockFallback({ artifact }: { artifact: ArtifactRef }) {
	const variant = artifact.preview?.variant ?? "generic";
	if (variant === "craftax_pareto") return <CraftaxEvalVisual artifact={artifact} />;
	if (variant === "craftax_frame") return <CraftaxFrameVisual artifact={artifact} />;
	return (
		<div className="visual-generic">
			<h2>{artifact.title}</h2>
			{artifact.summary ? <p>{artifact.summary}</p> : null}
			{artifact.templateId ? (
				<p className="visual-template-id">template · {artifact.templateId}</p>
			) : null}
		</div>
	);
}

function TemplateShellHost({ artifact }: { artifact: ArtifactRef }) {
	const [Shell, setShell] = useState<ComponentType<ShellProps> | null>(null);
	const [failed, setFailed] = useState(false);

	useEffect(() => {
		let cancelled = false;
		const templateId = artifact.templateId;
		if (!templateId) {
			setFailed(true);
			return;
		}

		async function load() {
			try {
				const Component = await loadVisualShell(templateId!);
				if (!Component) {
					if (!cancelled) setFailed(true);
					return;
				}
				if (!cancelled) setShell(() => Component);
			} catch {
				if (!cancelled) setFailed(true);
			}
		}

		void load();
		return () => {
			cancelled = true;
		};
	}, [artifact.templateId]);

	if (failed) {
		// Map known craftax templates onto fixture mocks when shell import fails.
		const mapped: ArtifactRef = {
			...artifact,
			preview: {
				variant:
					artifact.templateId?.includes("eval_matrix") ||
					artifact.templateId?.includes("craftax")
						? "craftax_pareto"
						: artifact.templateId?.includes("rollout") ||
							  artifact.templateId?.includes("scrub")
							? "craftax_frame"
							: (artifact.preview?.variant ?? "generic"),
				metrics: artifact.preview?.metrics
			}
		};
		return <MockFallback artifact={mapped} />;
	}

	if (!Shell) {
		return <p className="visual-loading">Loading visual shell…</p>;
	}

	return (
		<div data-testid="visual-template-shell">
			<Shell
				{...(artifact.bindings as unknown as ShellProps | undefined)}
				title={artifact.title}
				lede={artifact.summary}
				bindings={artifact.bindings}
			/>
		</div>
	);
}

export function VisualPane({ artifact, onClose }: Props) {
	const useTemplate =
		Boolean(artifact.templateId);

	// Explicit craftax mock variants always win; otherwise try @synth/visuals.
	const showTemplate =
		useTemplate ||
		(Boolean(artifact.templateId) && !artifact.preview?.variant) ||
		(Boolean(artifact.templateId) && artifact.preview?.variant === "generic");

	return (
		<aside className="visual-pane" data-testid="visual-pane" aria-label="Visual artifact">
			<header className="visual-pane-head">
				<div className="visual-pane-head-text">
					<span className="visual-pane-kind">Visual</span>
					<span className="visual-pane-title">{artifact.title}</span>
				</div>
				<button type="button" className="visual-close" onClick={onClose} aria-label="Close visual">
					×
				</button>
			</header>
			<div className="visual-pane-body">
				{showTemplate ? (
					<TemplateShellHost artifact={artifact} />
				) : (
					<MockFallback artifact={artifact} />
				)}
			</div>
			<footer className="visual-pane-foot">
				Esc or chip again to hide · usesynth.ai/evals/craftax
			</footer>
		</aside>
	);
}
