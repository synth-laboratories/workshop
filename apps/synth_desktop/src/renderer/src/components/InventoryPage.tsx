import { useCallback, useEffect, useState } from "react";
import { runtimeClient } from "@synth/runtime-client";
import type {
	ContainerDeployment,
	RuntimeHealth,
	TraceV5Record,
	UsageLedgerEntry,
	VisualInstanceRecord
} from "@synth/runtime-protocol";

export type InventoryTab = "containers" | "traces" | "visuals" | "usage";

type Props = {
	initialTab?: InventoryTab;
	onOpenVisual: (visual: VisualInstanceRecord) => void;
	onBack: () => void;
};

function formatWhen(iso: string): string {
	try {
		return new Date(iso).toLocaleString();
	} catch {
		return iso;
	}
}

export function InventoryPage({
	initialTab = "containers",
	onOpenVisual,
	onBack
}: Props) {
	const [tab, setTab] = useState<InventoryTab>(initialTab);
	const [containers, setContainers] = useState<ContainerDeployment[]>([]);
	const [traces, setTraces] = useState<TraceV5Record[]>([]);
	const [visuals, setVisuals] = useState<VisualInstanceRecord[]>([]);
	const [usage, setUsage] = useState<UsageLedgerEntry[]>([]);
	const [health, setHealth] = useState<RuntimeHealth | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [busyId, setBusyId] = useState<string | null>(null);

	const refresh = useCallback(async () => {
		setError(null);
		try {
			const [nextContainers, nextTraces, nextVisuals, nextUsage, nextHealth] = await Promise.all([
				runtimeClient.listContainers(),
				runtimeClient.listTraces(),
				runtimeClient.listVisuals(),
				runtimeClient.listUsage(100),
				runtimeClient.health()
			]);
			setContainers(nextContainers);
			setTraces(nextTraces);
			setVisuals(nextVisuals);
			setUsage(nextUsage);
			setHealth(nextHealth);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		}
	}, []);

	useEffect(() => {
		void refresh();
	}, [refresh]);

	const probe = async (containerId: string) => {
		setBusyId(containerId);
		setError(null);
		try {
			await runtimeClient.probeContainer(containerId);
			await refresh();
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusyId(null);
		}
	};

	return (
		<div className="inventory-page" data-testid="inventory-page">
			<header className="inventory-head">
				<button type="button" className="desk-back" onClick={onBack}>
					← Back
				</button>
				<div>
					<h1>Inventory</h1>
					<p className="inventory-lede">
						Local containers, Trace V5 records, and visual instances from the runtime vault.
					</p>
				</div>
				<button type="button" className="inventory-refresh" onClick={() => void refresh()}>
					Refresh
				</button>
			</header>

			{error ? (
				<div className="inventory-error" role="alert">
					{error}
				</div>
			) : null}

			<div className="inventory-tabs" role="tablist" aria-label="Inventory sections">
				{(
					[
						["containers", "Containers", containers.length],
						["traces", "Traces", traces.length],
						["visuals", "Visuals", visuals.length]
						,["usage", "Usage", usage.length]
					] as const
				).map(([id, label, count]) => (
					<button
						key={id}
						type="button"
						role="tab"
						aria-selected={tab === id}
						className={`inventory-tab${tab === id ? " active" : ""}`}
						onClick={() => setTab(id)}
						data-testid={`inventory-tab-${id}`}
					>
						{label}
						<span className="inventory-tab-count">{count}</span>
					</button>
				))}
			</div>

			{tab === "containers" ? (
				<div className="inventory-panel" data-testid="inventory-containers">
					{containers.length === 0 ? (
						<p className="inventory-empty">No containers yet.</p>
					) : (
						<ul className="inventory-list">
							{containers.map((c) => (
								<li key={c.id} className="inventory-row" data-testid={`inventory-container-${c.id}`}>
									<div className="inventory-row-main">
										<strong>{c.name}</strong>
										<span className="inventory-row-meta">
											{c.location} · {c.status}
											{c.taskFamily ? ` · ${c.taskFamily}` : ""}
										</span>
										<span className="inventory-row-when">{formatWhen(c.updatedAt)}</span>
									</div>
									<button
										type="button"
										className="inventory-row-action"
										disabled={busyId === c.id}
										onClick={() => void probe(c.id)}
										data-testid={`probe-container-${c.id}`}
									>
										{busyId === c.id ? "Probing…" : "Probe"}
									</button>
								</li>
							))}
						</ul>
					)}
				</div>
			) : null}

			{tab === "traces" ? (
				<div className="inventory-panel" data-testid="inventory-traces">
					{traces.length === 0 ? (
						<p className="inventory-empty">No traces yet.</p>
					) : (
						<ul className="inventory-list">
							{traces.map((t) => (
								<li key={t.id} className="inventory-row" data-testid={`inventory-trace-${t.id}`}>
									<div className="inventory-row-main">
										<strong>{t.title}</strong>
										<span className="inventory-row-meta">
											{t.source} · {t.digest.slice(0, 16)}…
											{t.reward != null ? ` · reward ${t.reward}` : ""}
										</span>
										<span className="inventory-row-when">{formatWhen(t.createdAt)}</span>
									</div>
								</li>
							))}
						</ul>
					)}
				</div>
			) : null}

			{tab === "visuals" ? (
				<div className="inventory-panel" data-testid="inventory-visuals">
					{visuals.length === 0 ? (
						<p className="inventory-empty">No visuals yet.</p>
					) : (
						<ul className="inventory-list">
							{visuals.map((v) => (
								<li key={v.id} className="inventory-row" data-testid={`inventory-visual-${v.id}`}>
									<div className="inventory-row-main">
										<strong>{v.title}</strong>
										<span className="inventory-row-meta">{v.templateId}</span>
										<span className="inventory-row-when">{formatWhen(v.updatedAt)}</span>
									</div>
									<button
										type="button"
										className="inventory-row-action"
										onClick={() => onOpenVisual(v)}
										data-testid={`open-visual-${v.id}`}
									>
										Open
									</button>
								</li>
							))}
						</ul>
					)}
				</div>
			) : null}

			{tab === "usage" ? (
				<div className="inventory-panel" data-testid="inventory-usage">
					<div className="storage-summary">
						<strong>Runtime data store</strong>
						<span>{health?.dataStore?.projects ?? 0} projects · {health?.dataStore?.sessions ?? 0} sessions · {health?.dataStore?.events ?? 0} events</span>
						<code>{health?.dataStore?.path ?? "connecting"}</code>
					</div>
					{usage.length === 0 ? <p className="inventory-empty">No usage entries yet.</p> : (
						<ul className="inventory-list">
							{usage.map((entry) => (
								<li key={entry.id} className="inventory-row">
									<div className="inventory-row-main"><strong>{entry.model}</strong><span className="inventory-row-meta">{entry.provider} · {entry.totalTokens} tokens{entry.costUsd != null ? ` · $${entry.costUsd.toFixed(4)}` : ""}</span><span className="inventory-row-when">{formatWhen(entry.createdAt)}</span></div>
								</li>
							))}
						</ul>
					)}
				</div>
			) : null}
		</div>
	);
}
