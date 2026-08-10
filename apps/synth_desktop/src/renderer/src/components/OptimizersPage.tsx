import { useCallback, useEffect, useMemo, useState } from "react";
import type { OptimizerAlgorithmInfo, OptimizerRunRecord } from "@synth/runtime-protocol";

const BANKING77_RECIPE_ID = "gepa.banking77.smoke.v1";
const BANKING77_SMOKE_COST_USD = 0.25;
const BANKING77_SMOKE_ROLLOUTS = 8;
const CRAFTAX_SFT_RECIPE_ID = "sft.craftax.gpt-oss.smoke.v1";
const CRAFTAX_SFT_ROLLOUTS = 8;
const CRAFTAX_SFT_STEPS = 4;

type Props = {
	onOpenVisual: (visualId: string) => void;
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

export function OptimizersPage({ onOpenVisual, onBack }: Props) {
	const [runs, setRuns] = useState<OptimizerRunRecord[]>([]);
	const [algorithms, setAlgorithms] = useState<OptimizerAlgorithmInfo[]>([]);
	const [recipes, setRecipes] = useState<Array<{ id: string; availability: string }>>([]);
	const [search, setSearch] = useState("");
	const [status, setStatus] = useState("all");
	const [algorithm, setAlgorithm] = useState("all");
	const [source, setSource] = useState("all");
	const [error, setError] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const [launcherOpen, setLauncherOpen] = useState(false);
	const [launchConfirmed, setLaunchConfirmed] = useState(false);
	const [sftLauncherOpen, setSftLauncherOpen] = useState(false);
	const [sftLaunchConfirmed, setSftLaunchConfirmed] = useState(false);

	const refresh = useCallback(async () => {
		if (!window.synthOptimizers) {
			setError("Optimizer bridge is unavailable");
			return;
		}
		setError(null);
		const [nextRuns, nextAlgorithms, nextRecipes] = await Promise.all([
			window.synthOptimizers.list({
				search: search.trim() || undefined,
				status: status === "all" ? undefined : status,
				algorithmId: algorithm === "all" ? undefined : algorithm,
				source: source === "all" ? undefined : source
			}),
			window.synthOptimizers.listAlgorithms(),
			window.synthOptimizers.listRecipes?.() ?? Promise.resolve([])
		]);
		setRuns(nextRuns);
		setAlgorithms(nextAlgorithms);
		setRecipes(nextRecipes);
		if (!selectedId && nextRuns[0]) setSelectedId(nextRuns[0].id);
	}, [algorithm, search, selectedId, source, status]);

	useEffect(() => {
		void refresh().catch((reason) => setError(String(reason)));
		const unlisten = window.synthOptimizers?.onEvent?.(() => {
			void refresh().catch(() => undefined);
		});
		return () => unlisten?.();
	}, [refresh]);

	const selected = useMemo(
		() => runs.find((run) => run.id === selectedId) ?? null,
		[runs, selectedId]
	);
	const banking77RecipeAvailable = recipes.some(
		(recipe) => recipe.id === BANKING77_RECIPE_ID && recipe.availability === "available"
	);
	const craftaxSftRecipeAvailable = recipes.some(
		(recipe) => recipe.id === CRAFTAX_SFT_RECIPE_ID && recipe.availability === "available"
	);

	const seedFixture = async (fixture: string) => {
		if (!window.synthOptimizers) return;
		setBusy(true);
		setError(null);
		try {
			const run = await window.synthOptimizers.create({
				algorithmId: fixture === "goex" ? "go-ex" : fixture,
				seedFixture: fixture,
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

	const openSelectedVisual = async () => {
		if (!selected || !window.synthOptimizers) return;
		setBusy(true);
		try {
			const run = await window.synthOptimizers.openVisual(selected.id);
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
		if (!window.synthOptimizers) return;
		const path = window.prompt(
			"Local OSS GEPA or optimizers-beta run path (workspace, run dir, or events.jsonl)"
		);
		if (!path?.trim()) return;
		setBusy(true);
		setError(null);
		try {
			const run = await window.synthOptimizers.importLocal({
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
		if (!window.synthOptimizers) return;
		setBusy(true);
		setError(null);
		try {
			const cloudRuns = await window.synthOptimizers.listCloud({ limit: 20 });
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
				await window.synthOptimizers.reconcileCloud({
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

	const launchBanking77Smoke = async () => {
		if (!window.synthOptimizers) return;
		if (!launchConfirmed) return;
		setBusy(true);
		setError(null);
		try {
			const run = await window.synthOptimizers.startRecipe({
				recipeId: BANKING77_RECIPE_ID,
				openVisual: true
			});
			setLauncherOpen(false);
			setLaunchConfirmed(false);
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

	const launchCraftaxSftSmoke = async () => {
		if (!window.synthOptimizers || !sftLaunchConfirmed) return;
		setBusy(true);
		setError(null);
		try {
			const run = await window.synthOptimizers.startRecipe({
				recipeId: CRAFTAX_SFT_RECIPE_ID,
				openVisual: true
			});
			setSftLauncherOpen(false);
			setSftLaunchConfirmed(false);
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

	const selectedExecution = selected
		? selected.executionBindings.length > 0
			? selected.executionBindings.map((binding) => binding.label ?? binding.kind).join(" · ")
			: selected.source === "cloud" ? "Cloud managed" : "Local process"
		: null;
	const selectedDiagnostic = optimizerDiagnostic(selected?.error);
	const selectedRunDirectory = selected && typeof selected.summary?.runDirectory === "string"
		? selected.summary.runDirectory
		: null;

	const refreshSelected = async () => {
		if (!selected || !window.synthOptimizers) return;
		setBusy(true);
		try {
			if (selected.source === "cloud") {
				await window.synthOptimizers.reconcileCloud({
					optimizerRunId: selected.id,
					openVisual: false
				});
			} else {
				await window.synthOptimizers.refresh(selected.id);
			}
			await refresh();
		} catch (reason) {
			setError(String(reason));
		} finally {
			setBusy(false);
		}
	};

	const controlSelected = async (action: "cancel" | "pause" | "resume") => {
		if (!selected || !window.synthOptimizers) return;
		setBusy(true);
		setError(null);
		try {
			const run = await window.synthOptimizers[action](selected.id);
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
				<button type="button" className="ghost-button" onClick={onBack}>
					← Back
				</button>
				<div>
					<h1>Optimizers</h1>
					<p className="inventory-lede">Track local and cloud training runs, inspect progress, and open their visuals.</p>
				</div>
			</header>

			{error ? <p className="inventory-error" role="alert" data-testid="optimizer-error">{error}</p> : null}

			<section className="optimizer-launch-card" aria-labelledby="banking77-launch-title" data-testid="banking77-gepa-launch-card">
				<div>
					<span className="optimizer-eyebrow">Pinned recipe · Local process</span>
					<h2 id="banking77-launch-title">Banking77 GEPA smoke</h2>
					<p>One generation, one proposal, at most {BANKING77_SMOKE_ROLLOUTS} rollouts, capped at ${BANKING77_SMOKE_COST_USD.toFixed(2)}.</p>
				</div>
				<button className="primary-button" type="button" disabled={busy || !banking77RecipeAvailable} onClick={() => setLauncherOpen(true)} data-testid="configure-banking77-gepa-smoke">
					{banking77RecipeAvailable ? "Configure bounded smoke run" : "Recipe unavailable"}
				</button>
			</section>

			<section className="optimizer-launch-card" aria-labelledby="craftax-sft-launch-title" data-testid="craftax-sft-launch-card">
				<div>
					<span className="optimizer-eyebrow">Pinned recipe · Local process + Tinker</span>
					<h2 id="craftax-sft-launch-title">Craftax GPT-OSS SFT smoke</h2>
					<p>Four teacher rollouts, {CRAFTAX_SFT_STEPS} LoRA steps, and base-vs-adapter evaluation across {CRAFTAX_SFT_ROLLOUTS} total environment rollouts.</p>
				</div>
				<button className="primary-button" type="button" disabled={busy || !craftaxSftRecipeAvailable} onClick={() => setSftLauncherOpen(true)} data-testid="configure-craftax-sft-smoke">
					{craftaxSftRecipeAvailable ? "Configure bounded SFT run" : "Recipe unavailable"}
				</button>
			</section>

			{launcherOpen ? (
				<div className="optimizer-launch-dialog" role="dialog" aria-modal="true" aria-labelledby="banking77-dialog-title" data-testid="banking77-gepa-launch-dialog">
					<div className="optimizer-launch-dialog-card">
						<div className="optimizer-launch-dialog-head">
							<div><span className="optimizer-eyebrow">Review before starting compute</span><h2 id="banking77-dialog-title">Banking77 GEPA bounded smoke</h2></div>
							<button type="button" className="ghost-button" aria-label="Close Banking77 GEPA launcher" onClick={() => setLauncherOpen(false)} data-testid="close-banking77-gepa-launcher">×</button>
						</div>
						<dl className="optimizer-launch-summary" data-testid="banking77-gepa-bounds">
							<div><dt>Optimizer</dt><dd>GEPA</dd></div><div><dt>Dataset</dt><dd>Banking77</dd></div>
							<div><dt>Execution</dt><dd>Local recipe process</dd></div><div><dt>Run ID</dt><dd>Assigned on start</dd></div>
							<div><dt>Rollout ceiling</dt><dd>{BANKING77_SMOKE_ROLLOUTS}</dd></div><div><dt>Cost ceiling</dt><dd>${BANKING77_SMOKE_COST_USD.toFixed(2)}</dd></div>
						</dl>
						<p className="optimizer-launch-prereq">Uses 4 train rows and 2 heldout rows. Requires the Banking77 cookbook checkout and an OpenAI API key available to the Desktop runtime.</p>
						<label className="optimizer-launch-confirm"><input type="checkbox" checked={launchConfirmed} onChange={(event) => setLaunchConfirmed(event.target.checked)} data-testid="confirm-banking77-gepa-cost" /> I approve this bounded run and its API usage.</label>
						<div className="optimizer-launch-dialog-actions">
							<button type="button" className="secondary-button" onClick={() => setLauncherOpen(false)}>Cancel</button>
							<button type="button" className="primary-button" disabled={busy || !launchConfirmed} onClick={() => void launchBanking77Smoke()} data-testid="start-banking77-gepa-smoke">{busy ? "Starting…" : "Start bounded Banking77 GEPA smoke"}</button>
						</div>
					</div>
				</div>
			) : null}

			{sftLauncherOpen ? (
				<div className="optimizer-launch-dialog" role="dialog" aria-modal="true" aria-labelledby="craftax-sft-dialog-title" data-testid="craftax-sft-launch-dialog">
					<div className="optimizer-launch-dialog-card">
						<div className="optimizer-launch-dialog-head">
							<div><span className="optimizer-eyebrow">Review before starting paid compute</span><h2 id="craftax-sft-dialog-title">Craftax GPT-OSS bounded SFT smoke</h2></div>
							<button type="button" className="ghost-button" aria-label="Close Craftax SFT launcher" onClick={() => setSftLauncherOpen(false)}>×</button>
						</div>
						<dl className="optimizer-launch-summary" data-testid="craftax-sft-bounds">
							<div><dt>Teacher</dt><dd>GPT-OSS-120B via Groq</dd></div><div><dt>Student</dt><dd>GPT-OSS-20B LoRA via Tinker</dd></div>
							<div><dt>Teacher rollouts</dt><dd>4</dd></div><div><dt>Training steps</dt><dd>{CRAFTAX_SFT_STEPS}</dd></div>
							<div><dt>Held-out seeds</dt><dd>2 × base and adapter</dd></div><div><dt>Environment rollouts</dt><dd>{CRAFTAX_SFT_ROLLOUTS} maximum</dd></div>
						</dl>
						<p className="optimizer-launch-prereq">Uses the trusted Craftax binary (reusing port 8098 or owning it for this run) plus Groq and Tinker credentials in the trusted Desktop environment. Provider charges apply; this recipe is bounded by rollouts and steps, not by a dollar ceiling.</p>
						<label className="optimizer-launch-confirm"><input type="checkbox" checked={sftLaunchConfirmed} onChange={(event) => setSftLaunchConfirmed(event.target.checked)} data-testid="confirm-craftax-sft-compute" /> I approve the bounded Groq and Tinker compute.</label>
						<div className="optimizer-launch-dialog-actions">
							<button type="button" className="secondary-button" onClick={() => setSftLauncherOpen(false)}>Cancel</button>
							<button type="button" className="primary-button" disabled={busy || !sftLaunchConfirmed} onClick={() => void launchCraftaxSftSmoke()} data-testid="start-craftax-sft-smoke">{busy ? "Starting…" : "Start bounded Craftax SFT smoke"}</button>
						</div>
					</div>
				</div>
			) : null}

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
						<option value="all">All sources</option><option value="local">Local</option><option value="cloud">Cloud</option>
					</select>
				</div>
				<div className="optimizer-actions">
					<button className="secondary-button" type="button" disabled={busy} onClick={() => void importLocal()} data-testid="import-local-optimizer">Import local</button>
					<button className="secondary-button" type="button" disabled={busy} onClick={() => void syncCloud()} data-testid="sync-cloud-optimizers">Sync cloud</button>
					<button className="primary-button" type="button" disabled={busy || !banking77RecipeAvailable} onClick={() => setLauncherOpen(true)} data-testid="create-cloud-optimizer">Banking77 GEPA smoke</button>
					<button className="primary-button" type="button" disabled={busy || !craftaxSftRecipeAvailable} onClick={() => setSftLauncherOpen(true)} data-testid="create-craftax-sft-optimizer">Craftax SFT smoke</button>
				</div>
			</div>

			<div className="optimizer-workbench">
				<section className="optimizer-runs" aria-label="Optimizer runs">
					<div className="optimizer-section-head"><div><span className="optimizer-eyebrow">Runs</span><strong>{runs.length} total</strong></div><div className="optimizer-fixtures"><span>Demo data</span><button type="button" disabled={busy} onClick={() => void seedFixture("gepa")} data-testid="seed-gepa-fixture">GEPA</button><button type="button" disabled={busy} onClick={() => void seedFixture("goex")}>GELO</button><button type="button" disabled={busy} onClick={() => void seedFixture("sft")}>SFT</button></div></div>
					<ul className="inventory-list optimizer-list">
						{runs.map((run) => (
							<li key={run.id}>
								<button
									type="button"
									className={`inventory-row${selectedId === run.id ? " active" : ""}`}
									data-testid={`optimizer-run-${run.id}`}
									onClick={() => setSelectedId(run.id)}
								>
									<span className="optimizer-run-main"><span className="optimizer-algorithm">{algorithmLabel(run.algorithmId)}</span><strong>{run.objective ?? run.id}</strong><small>{formatWhen(run.finishedAt ?? run.startedAt ?? run.createdAt)}</small></span>
									<span className="optimizer-run-meta"><span className={`optimizer-status ${run.status}`}>{run.status}</span><small>{run.source} · ${(run.usage.costUsd ?? 0).toFixed(2)}</small></span>
								</button>
							</li>
						))}
						{runs.length === 0 ? <li className="optimizer-empty"><span className="optimizer-empty-icon" aria-hidden>↗</span><strong>No optimizer runs yet</strong><p>Import a local run, connect cloud history, or start the pinned bounded Banking77 recipe.</p><button className="primary-button" type="button" disabled={busy || !banking77RecipeAvailable} onClick={() => setLauncherOpen(true)}>Configure Banking77 smoke</button></li> : null}
					</ul>
				</section>

				<section className="optimizer-inspector" aria-label="Optimizer inspector">
					{selected ? (
						<div data-testid="optimizer-inspector">
							<span className="optimizer-eyebrow">Run details</span><h2>{algorithmLabel(selected.algorithmId)}</h2><p>{selected.objective ?? selected.id}</p>
							<dl>
								<dt>Status</dt><dd>{selected.status}</dd>
								<dt>Source</dt><dd>{selected.source}</dd>
								<dt>Execution</dt><dd data-testid="optimizer-execution-mode">{selectedExecution}</dd>
								<dt>Live events</dt><dd>{selected.capabilities.streamEvents ? "Available" : "Replay / refresh"}</dd>
								<dt>Cursor</dt><dd>{selected.cursorSeq}</dd>
								<dt>Cost</dt><dd>${(selected.usage.costUsd ?? 0).toFixed(2)}</dd>
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
