import { useCallback, useEffect, useMemo, useState } from "react";
import type { OptimizerAlgorithmInfo, OptimizerRunRecord } from "@synth/runtime-protocol";
import type { PluginStatus } from "../bridge/types";
import { bridges } from "../runtime/desktopBridge";

type OptimizerGuide = {
	id: "gepa" | "go-ex" | "sft";
	label: string;
	name: string;
	description: string;
	flow: string[];
	prompt: string;
};

const OPTIMIZER_GUIDES: OptimizerGuide[] = [
	{
		id: "gepa",
		label: "GE",
		name: "GEPA",
		description: "Improve prompts by proposing candidates, evaluating them, and maintaining a quality frontier.",
		flow: ["Propose", "Evaluate", "Select"],
		prompt: "Help me set up a GEPA optimization in Workshop. Do not start compute yet. First ask what I want to optimize, then help me choose or create the evaluation Container, dataset splits, scoring contract, proposer model, budget, and stopping criteria. Verify the target and event-stream contracts before proposing a run. Explain tradeoffs and wait for my explicit approval before starting paid compute."
	},
	{
		id: "go-ex",
		label: "GX",
		name: "GELO",
		description: "Explore prompt-policy variants from rollout evidence and branch from useful intermediate states.",
		flow: ["Explore", "Branch", "Verify"],
		prompt: "Help me set up a prompt-only GELO (GoEx) optimization in Workshop. Do not start compute yet. First ask what behavior I want to improve and which Container or evaluation target should measure it. Discover the target's actual capabilities, including streaming, rewards, prompt treatment, checkpoints, and restore support; fail the plan early if required affordances are missing. Then help me choose seeds, proposer policy, budget, heldout evaluation, and stopping criteria. Wait for my explicit approval before starting paid compute."
	},
	{
		id: "sft",
		label: "SF",
		name: "SFT",
		description: "Collect strong demonstrations, train checkpoints, and compare the adapted model against its baseline.",
		flow: ["Collect", "Train", "Compare"],
		prompt: "Help me set up an SFT optimization in Workshop. Do not start compute yet. First ask what capability I want to improve and which Container or evaluation target should measure it. Help me design demonstration collection and filtering, the student and training provider, checkpoint cadence, baseline-versus-checkpoint evaluation, budget, and uplift criteria. Verify that training and inference targets are executable and that lifecycle events will be inspectable. Wait for my explicit approval before starting paid compute."
	}
];

type Props = {
	onOpenVisual: (visualId: string) => void;
	onStartAgent: (guide: OptimizerGuide) => Promise<void>;
	onBack: () => void;
};

function formatWhen(iso: string): string {
	try {
		return new Date(iso).toLocaleString();
	} catch {
		return iso;
	}
}

function algorithmLabel(id: string): string {
	if (id === "gepa") return "GEPA";
	if (id === "go-ex") return "GELO";
	if (id === "sft") return "SFT";
	return id;
}

type OptimizerDiagnostic = {
	title: string;
	message: string;
	field?: string;
	raw?: string;
	logPath?: string;
};

function optimizerDiagnostic(error: unknown): OptimizerDiagnostic | null {
	if (!error) return null;
	const value = typeof error === "object" ? error as Record<string, unknown> : {};
	const message = typeof error === "string"
		? error
		: typeof value.message === "string" ? value.message : String(error);
	const raw = typeof value.stderrTail === "string" ? value.stderrTail : message;
	const missingField = raw.match(/configuration error:\s*([a-z0-9_.]+)\s+is required and must be positive/i)?.[1];
	if (missingField) {
		const estimate = missingField.includes("rollout") ? "rollout" : missingField.includes("proposer") ? "proposer" : "optimizer";
		return {
			title: `Missing ${estimate} cost estimate`,
			message: "The safety budget rejected this recipe before compute started.",
			field: missingField,
			raw,
			logPath: typeof value.logPath === "string" ? value.logPath : undefined
		};
	}
	return {
		title: "Optimizer run failed",
		message,
		raw: raw !== message ? raw : undefined,
		logPath: typeof value.logPath === "string" ? value.logPath : undefined
	};
}

function fileName(path: string): string {
	return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

function runTitle(run: OptimizerRunRecord): string {
	const objective = run.objective ?? run.id;
	const importedPath = objective.startsWith("imported from ")
		? objective.slice("imported from ".length)
		: null;
	if (!importedPath) return objective;
	const parts = importedPath.split(/[\\/]/).filter(Boolean);
	let artifactName = parts.at(-1)?.includes("events.") ? parts.at(-2) : parts.at(-1);
	if (artifactName === "artifacts") artifactName = parts.at(-3);
	const algorithmTokens = new Set([run.algorithmId, algorithmLabel(run.algorithmId), "goex"]
		.map((token) => token.toLowerCase().replace(/[^a-z0-9]/g, "")));
	return (artifactName ?? run.id)
		.split(/[_-]+/g)
		.filter((token) => !algorithmTokens.has(token.toLowerCase().replace(/[^a-z0-9]/g, "")))
		.join(" ")
		.replace(/\bmed\b/gi, "medium")
		.replace(/\b\w/g, (character) => character.toUpperCase());
}

export function OptimizersPage({ onOpenVisual, onStartAgent, onBack }: Props) {
	const [runs, setRuns] = useState<OptimizerRunRecord[]>([]);
	const [algorithms, setAlgorithms] = useState<OptimizerAlgorithmInfo[]>([]);
	const [search, setSearch] = useState("");
	const [status, setStatus] = useState("all");
	const [algorithm, setAlgorithm] = useState("all");
	const [source, setSource] = useState("all");
	const [error, setError] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const [startingAgent, setStartingAgent] = useState<OptimizerGuide["id"] | null>(null);
	const [plugin, setPlugin] = useState<PluginStatus | null>(null);
	const [changingReleaseChannel, setChangingReleaseChannel] = useState(false);

	const refreshPlugin = useCallback(async () => {
		if (!bridges.plugins) return;
		const status = await bridges.plugins.status("optimizers");
		setPlugin(status);
	}, []);

	const refresh = useCallback(async () => {
		if (!bridges.optimizers) {
			setError("Optimizer bridge is unavailable");
			return;
		}
		setError(null);
		const [nextRuns, nextAlgorithms] = await Promise.all([
			bridges.optimizers.list({
				search: search.trim() || undefined,
				status: status === "all" ? undefined : status,
				algorithmId: algorithm === "all" ? undefined : algorithm,
				source: source === "all" ? undefined : source
			}),
			bridges.optimizers.listAlgorithms()
		]);
		setRuns(nextRuns);
		setAlgorithms(nextAlgorithms);
		if (!selectedId && nextRuns[0]) setSelectedId(nextRuns[0].id);
	}, [algorithm, search, selectedId, source, status]);

	useEffect(() => {
		void refresh().catch((reason) => setError(String(reason)));
		void refreshPlugin().catch(() => undefined);
		const unlisten = bridges.optimizers?.onEvent?.(() => {
			void refresh().catch(() => undefined);
			void refreshPlugin().catch(() => undefined);
		});
		const timer = window.setInterval(() => {
			void refreshPlugin().catch(() => undefined);
		}, 5_000);
		return () => {
			unlisten?.();
			window.clearInterval(timer);
		};
	}, [refresh, refreshPlugin]);

	const selected = useMemo(
		() => runs.find((run) => run.id === selectedId) ?? null,
		[runs, selectedId]
	);
	const setReleaseChannel = async (channel: "official" | "dev") => {
		if (!bridges.plugins) return;
		setChangingReleaseChannel(true);
		setError(null);
		try {
			setPlugin(await bridges.plugins.setReleaseChannel("optimizers", channel));
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setChangingReleaseChannel(false);
		}
	};
	const pluginPhaseLabel = plugin
		? ({
			not_installed: "Not installed",
			downloading: "Downloading",
			verifying: "Verifying",
			installed: "Installed",
			starting: "Starting",
			ready: "Ready",
			stopping: "Stopping",
			stopped: "Stopped",
			updating: "Updating",
			removing: "Removing",
			degraded: "Degraded",
			error: "Error",
			disabled: "Disabled"
		}[plugin.phase] ?? plugin.phase)
		: null;

	const startAgent = async (guide: OptimizerGuide) => {
		setStartingAgent(guide.id);
		setError(null);
		try {
			await onStartAgent(guide);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setStartingAgent(null);
		}
	};

	const openSelectedVisual = async () => {
		if (!selected || !bridges.optimizers) return;
		setBusy(true);
		try {
			const run = await bridges.optimizers.openVisual(selected.id);
			const visualId = run.visualRefs.find((ref) => ref.kind === "visual")?.id;
			if (visualId) onOpenVisual(visualId);
			await refresh();
		} catch (reason) {
			setError(String(reason));
		} finally {
			setBusy(false);
		}
	};

	const importLocal = async () => {
		if (!bridges.optimizers) return;
		const path = window.prompt(
			"Local OSS GEPA or optimizers-beta run path (workspace, run dir, or events.jsonl)"
		);
		if (!path?.trim()) return;
		setBusy(true);
		setError(null);
		try {
			const run = await bridges.optimizers.importLocal({
				path: path.trim(),
				openVisual: true
			});
			setSelectedId(run.id);
			await refresh();
			const visualId = run.visualRefs.find((ref) => ref.kind === "visual")?.id;
			if (visualId) onOpenVisual(visualId);
		} catch (reason) {
			setError(String(reason));
		} finally {
			setBusy(false);
		}
	};

	const syncCloud = async () => {
		if (!bridges.optimizers) return;
		setBusy(true);
		setError(null);
		try {
			const cloudRuns = await bridges.optimizers.listCloud({ limit: 20 });
			for (const item of cloudRuns) {
				const runId =
					item && typeof item === "object"
						? String(
								(item as { run_id?: string; id?: string }).run_id ??
									(item as { run_id?: string; id?: string }).id ??
									""
							)
						: "";
				if (!runId) continue;
				await bridges.optimizers.reconcileCloud({
					optimizerRunId: runId,
					afterSeq: 0,
					openVisual: false
				});
			}
			await refresh();
		} catch (reason) {
			setError(String(reason));
		} finally {
			setBusy(false);
		}
	};

	const selectedExecution = selected
		? selected.executionBindings.length > 0
			? selected.executionBindings.map((binding) => binding.label ?? binding.kind).join(" · ")
			: selected.source === "hosted"
				? "optimizers-beta"
				: selected.source === "cloud"
					? "Cloud managed"
					: "Local process"
		: null;
	const selectedDiagnostic = optimizerDiagnostic(selected?.error);
	const selectedRunDirectory = selected && typeof selected.summary?.runDirectory === "string"
		? selected.summary.runDirectory
		: null;

	const refreshSelected = async () => {
		if (!selected || !bridges.optimizers) return;
		setBusy(true);
		try {
			if (selected.source === "cloud") {
				await bridges.optimizers.reconcileCloud({
					optimizerRunId: selected.id,
					openVisual: false
				});
			} else {
				await bridges.optimizers.refresh(selected.id);
			}
			await refresh();
		} catch (reason) {
			setError(String(reason));
		} finally {
			setBusy(false);
		}
	};

	const controlSelected = async (action: "cancel" | "pause" | "resume") => {
		if (!selected || !bridges.optimizers) return;
		setBusy(true);
		setError(null);
		try {
			const run = await bridges.optimizers[action](selected.id);
			setSelectedId(run.id);
			await refresh();
		} catch (reason) {
			setError(String(reason));
		} finally {
			setBusy(false);
		}
	};

	return (
		<div className="inventory-page optimizers-page" data-testid="optimizers-page">
			<header className="inventory-head optimizer-head">
				<button type="button" className="optimizer-back-button" aria-label="Back" onClick={onBack}>←</button>
				<div className="optimizer-head-copy">
					<span className="optimizer-eyebrow">Workshop</span>
					<h1>Optimizers</h1>
					<p className="inventory-lede">Run, compare, and inspect optimization jobs.</p>
				</div>
				<div className="optimizer-head-actions">
					<button className="secondary-button" type="button" disabled={busy} onClick={() => void importLocal()} data-testid="import-local-optimizer">Import run</button>
					<button className="secondary-button" type="button" disabled={busy} onClick={() => void syncCloud()} data-testid="sync-cloud-optimizers">Sync cloud</button>
				</div>
			</header>

			{error ? <p className="inventory-error" role="alert" data-testid="optimizer-error">{error}</p> : null}
			{plugin ? (
				<section className="optimizer-plugin-status" data-testid="optimizer-plugin-status" data-phase={plugin.phase}>
					<div className="optimizer-plugin-summary">
						<span className="optimizer-eyebrow">Plugin</span>
						<strong data-testid="optimizer-plugin-phase">{pluginPhaseLabel}</strong>
						{plugin.installedVersion ? <span>Installed v{plugin.installedVersion}</span> : null}
						<span>Selected v{plugin.catalogVersion}</span>
						{plugin.digest ? <code>{plugin.digest}</code> : null}
					</div>
					<label className="optimizer-release-channel" htmlFor="optimizer-release-channel">
						<span>Release channel</span>
						<select
							id="optimizer-release-channel"
							data-testid="optimizer-release-channel"
							value={plugin.releaseChannel}
							disabled={changingReleaseChannel}
							onChange={(event) => void setReleaseChannel(event.target.value as "official" | "dev")}
						>
							<option value="official">Official releases (Recommended)</option>
							<option value="dev">Dev nightlies</option>
						</select>
					</label>
					{plugin.releaseChannel === "dev" ? (
						<p className="optimizer-release-warning" data-testid="optimizer-release-warning">
							Nightlies may change between Workshop releases. Installs remain pinned and verified.
						</p>
					) : null}
					{plugin.detail ? <p>{plugin.detail}</p> : null}
				</section>
			) : null}

			<section className="optimizer-recipes" aria-labelledby="optimizer-recipes-title">
				<div className="optimizer-recipes-head">
					<div><span className="optimizer-eyebrow">Agent-guided setup</span><h2 id="optimizer-recipes-title">What do you want to optimize?</h2></div>
					<p>The agent will help choose the Container, evaluation, and budget.</p>
				</div>
				<div className="optimizer-recipe-grid">
					{OPTIMIZER_GUIDES.map((guide) => (
						<article className="optimizer-recipe-card" aria-labelledby={`optimizer-guide-${guide.id}`} data-testid={`optimizer-guide-${guide.id}`} key={guide.id}>
							<div className="optimizer-recipe-top"><span className="optimizer-recipe-mark">{guide.label}</span><span className="optimizer-recipe-runtime">Optimization algorithm</span></div>
							<h3 id={`optimizer-guide-${guide.id}`}>{guide.name}</h3>
							<p>{guide.description}</p>
							<div className="optimizer-recipe-flow" aria-label={`${guide.name} workflow`}>{guide.flow.map((step) => <span key={step}>{step}</span>)}</div>
							<button className="secondary-button" type="button" disabled={startingAgent !== null} onClick={() => void startAgent(guide)} data-testid={`start-${guide.id}-agent`}>
								{startingAgent === guide.id ? "Opening agent…" : "Plan with agent"}
							</button>
						</article>
					))}
				</div>
			</section>

			<div className="optimizer-toolbar" data-testid="optimizer-toolbar">
				<div className="optimizer-filters">
					<label className="optimizer-search">
						<span aria-hidden>⌕</span>
						<input aria-label="Search optimizers" placeholder="Search runs" value={search} onChange={(event) => setSearch(event.target.value)} data-testid="optimizers-search" />
					</label>
					<select aria-label="Status filter" value={status} onChange={(e) => setStatus(e.target.value)}>
						<option value="all">All statuses</option><option value="running">Running</option><option value="paused">Paused</option><option value="completed">Completed</option><option value="failed">Failed</option><option value="queued">Queued</option>
					</select>
					<select aria-label="Algorithm filter" value={algorithm} onChange={(e) => setAlgorithm(e.target.value)}>
						<option value="all">All algorithms</option>
						{algorithms.map((item) => <option key={item.id} value={item.id}>{item.title} · {item.availability}</option>)}
					</select>
					<select aria-label="Source filter" value={source} onChange={(e) => setSource(e.target.value)}>
						<option value="all">All sources</option><option value="local">Local</option><option value="hosted">Hosted</option><option value="cloud">Cloud</option>
					</select>
				</div>
			</div>

			<div className="optimizer-workbench">
				<section className="optimizer-runs" aria-label="Optimizer runs">
					<div className="optimizer-section-head"><div><span className="optimizer-eyebrow">Runs</span><strong>{runs.length} total</strong></div></div>
					<ul className="inventory-list optimizer-list">
						{runs.map((run) => (
							<li key={run.id}>
								<button
									type="button"
									className={`inventory-row${selectedId === run.id ? " active" : ""}`}
									data-testid={`optimizer-run-${run.id}`}
									onClick={() => setSelectedId(run.id)}
								>
									<span className="optimizer-run-main"><span className="optimizer-algorithm">{algorithmLabel(run.algorithmId)}</span><strong>{runTitle(run)}</strong><small>{formatWhen(run.finishedAt ?? run.startedAt ?? run.createdAt)}</small></span>
									<span className="optimizer-run-meta"><span className={`optimizer-status ${run.status}`}>{run.status}</span><small>{run.source} · {run.usage.costUsd == null ? "—" : `$${run.usage.costUsd.toFixed(2)}`}</small></span>
								</button>
							</li>
						))}
						{runs.length === 0 ? <li className="optimizer-empty"><span className="optimizer-empty-icon" aria-hidden>↗</span><strong>No optimizer runs yet</strong><p>Plan one with an agent above, import an existing run, or sync cloud history.</p></li> : null}
					</ul>
				</section>

				<section className="optimizer-inspector" aria-label="Optimizer inspector">
					{selected ? (
						<div data-testid="optimizer-inspector">
							<span className="optimizer-eyebrow">Run details</span><h2>{algorithmLabel(selected.algorithmId)}</h2><p>{runTitle(selected)}</p>
							<dl>
								<dt>Status</dt><dd>{selected.status}</dd>
								<dt>Source</dt><dd>{selected.source}</dd>
								<dt>Execution</dt><dd data-testid="optimizer-execution-mode">{selectedExecution}</dd>
								<dt>Live events</dt><dd>{selected.capabilities.streamEvents ? "Available" : "Replay / refresh"}</dd>
								<dt>Cursor</dt><dd>{selected.cursorSeq}</dd>
								<dt>Cost</dt><dd>{selected.usage.costUsd == null ? "—" : `$${selected.usage.costUsd.toFixed(2)}`}</dd>
								<dt>Created</dt><dd>{formatWhen(selected.createdAt)}</dd>
							</dl>
							{selectedDiagnostic ? (
								<section className="optimizer-diagnostic" role="alert" data-testid="optimizer-diagnostic">
									<span className="optimizer-diagnostic-kicker">Why it stopped</span>
									<strong>{selectedDiagnostic.title}</strong>
									<p>{selectedDiagnostic.message}</p>
									{selectedDiagnostic.field ? <code className="optimizer-diagnostic-field">{selectedDiagnostic.field}</code> : null}
									{selectedDiagnostic.raw ? (
										<details className="optimizer-diagnostic-details">
											<summary>Show technical details</summary>
											<pre data-testid="optimizer-stderr-tail">{selectedDiagnostic.raw}</pre>
										</details>
									) : null}
									{selectedDiagnostic.logPath ? <small>Log · {fileName(selectedDiagnostic.logPath)}</small> : null}
								</section>
							) : null}
							{selectedRunDirectory ? (
								<details className="optimizer-run-files" data-testid="optimizer-run-files">
									<summary>Logs &amp; artifacts</summary>
									<code>{selectedRunDirectory}</code>
									<ul><li>workshop.stdout.log</li><li>workshop.stderr.log</li><li>events.jsonl</li><li>result_manifest.json</li></ul>
								</details>
							) : null}
							<div className="optimizer-inspector-actions">
								<button className="primary-button" type="button" disabled={busy} onClick={() => void openSelectedVisual()} data-testid="open-optimizer-visual">Open visual</button>
								<button className="secondary-button" type="button" disabled={busy} onClick={() => void refreshSelected()} data-testid="refresh-optimizer-run">Refresh</button>
								{selected.capabilities.pause && selected.status === "running" ? <button className="secondary-button" type="button" disabled={busy} onClick={() => void controlSelected("pause")} data-testid="pause-optimizer-run">Pause</button> : null}
								{selected.capabilities.resume && selected.status === "paused" ? <button className="secondary-button" type="button" disabled={busy} onClick={() => void controlSelected("resume")} data-testid="resume-optimizer-run">Resume</button> : null}
								{selected.capabilities.cancel && !["completed", "failed", "cancelled"].includes(selected.status) ? <button className="secondary-button optimizer-danger-button" type="button" disabled={busy} onClick={() => void controlSelected("cancel")} data-testid="cancel-optimizer-run">Cancel</button> : null}
							</div>
						</div>
					) : (
						<div className="optimizer-empty optimizer-empty-inspector"><span className="optimizer-empty-icon" aria-hidden>◎</span><strong>Select a run</strong><p>Run details, usage, and linked visuals appear here.</p></div>
					)}
				</section>
			</div>
		</div>
	);
}
