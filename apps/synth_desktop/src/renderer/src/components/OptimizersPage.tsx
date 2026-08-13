import { useCallback, useEffect, useMemo, useState } from "react";
import type { OptimizerAlgorithmInfo, OptimizerRunRecord } from "@synth/runtime-protocol";
import { bridges } from "../runtime/desktopBridge";

const BANKING77_LUNA_RECIPE_ID = "gepa.banking77.luna.v1";
const BANKING77_SOL_RECIPE_ID = "gepa.banking77.sol.v1";
const BANKING77_RUN_COST_USD = 2.45;
const BANKING77_RUN_ROLLOUTS = 240;
const BANKING77_PAIR_COST_USD = BANKING77_RUN_COST_USD * 2;
const BANKING77_PAIR_ROLLOUTS = BANKING77_RUN_ROLLOUTS * 2;
const CRAFTAX_SFT_RECIPE_ID = "sft.craftax.gpt-oss.smoke.v1";
const HOSTED_SFT_FIXTURE_RECIPE_ID = "sft.hosted.fixture.v1";
const HOSTED_SFT_NEMOTRON_RECIPE_ID = "sft.craftax.nemotron-nano.tinker.v1";
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
	const containerFailure = raw.match(/container error:\s*([^\n]+)/i)?.[1]?.trim();
	if (containerFailure) {
		return {
			title: "Container rollout stream failed",
			message: containerFailure,
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

function reportedCost(costUsd: number | null | undefined): string {
	return costUsd != null && costUsd > 0 ? `$${costUsd.toFixed(2)}` : "—";
}

function optimizerStatusClass(status: string): string {
	if (status === "running") return "ws-badge-running";
	if (status === "completed") return "ws-badge-success";
	if (status === "failed" || status === "cancelled") return "ws-badge-danger";
	if (status === "paused" || status === "queued") return "ws-badge-warn";
	return "";
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
		if (!bridges.optimizers) {
			setError("Optimizer bridge is unavailable");
			return;
		}
		setError(null);
		const [nextRuns, nextAlgorithms, nextRecipes] = await Promise.all([
			bridges.optimizers.list({
				search: search.trim() || undefined,
				status: status === "all" ? undefined : status,
				algorithmId: algorithm === "all" ? undefined : algorithm,
				source: source === "all" ? undefined : source
			}),
			bridges.optimizers.listAlgorithms(),
			bridges.optimizers.listRecipes?.() ?? Promise.resolve([])
		]);
		setRuns(nextRuns);
		setAlgorithms(nextAlgorithms);
		setRecipes(nextRecipes);
		if (!selectedId && nextRuns[0]) setSelectedId(nextRuns[0].id);
	}, [algorithm, search, selectedId, source, status]);

	useEffect(() => {
		void refresh().catch((reason) => setError(String(reason)));
		const unlisten = bridges.optimizers?.onEvent?.(() => {
			void refresh().catch(() => undefined);
		});
		return () => unlisten?.();
	}, [refresh]);

	const selected = useMemo(
		() => runs.find((run) => run.id === selectedId) ?? null,
		[runs, selectedId]
	);
	const banking77RecipeAvailable = [BANKING77_LUNA_RECIPE_ID, BANKING77_SOL_RECIPE_ID].every(
		(recipeId) => recipes.some((recipe) => recipe.id === recipeId && recipe.availability === "available")
	);
	const craftaxSftRecipeAvailable = recipes.some(
		(recipe) => recipe.id === CRAFTAX_SFT_RECIPE_ID && recipe.availability === "available"
	);
	const hostedSftRecipeAvailable = recipes.some(
		(recipe) => recipe.id === HOSTED_SFT_FIXTURE_RECIPE_ID && recipe.availability === "available"
	);
	const nemotronSftRecipeAvailable = recipes.some(
		(recipe) => recipe.id === HOSTED_SFT_NEMOTRON_RECIPE_ID && recipe.availability === "available"
	);

	const seedFixture = async (fixture: string) => {
		if (!bridges.optimizers) return;
		setBusy(true);
		setError(null);
		try {
			const run = await bridges.optimizers.create({
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

	const launchBanking77Comparison = async () => {
		if (!bridges.optimizers) return;
		if (!launchConfirmed) return;
		setBusy(true);
		setError(null);
		// The bridge may remain pending for the lifetime of the local recipe. Close
		// the paid-compute confirmation immediately so live run events and visuals
		// stay reachable while both jobs are executing.
		setLauncherOpen(false);
		setLaunchConfirmed(false);
		try {
			const [lunaRun, solRun] = await Promise.all([
				bridges.optimizers.startRecipe({
					recipeId: BANKING77_LUNA_RECIPE_ID,
					openVisual: true
				}),
				bridges.optimizers.startRecipe({
					recipeId: BANKING77_SOL_RECIPE_ID,
					openVisual: true
				})
			]);
			setSelectedId(lunaRun.id);
			await refresh();
			const visualId = lunaRun.visualRefs.find((ref) => ref.kind === "visual")?.id
				?? solRun.visualRefs.find((ref) => ref.kind === "visual")?.id;
			if (visualId) onOpenVisual(visualId);
		} catch (reason) {
			setError(String(reason));
		} finally {
			setBusy(false);
		}
	};

	const launchHostedSftFixture = async () => {
		if (!bridges.optimizers || !hostedSftRecipeAvailable) return;
		setBusy(true);
		setError(null);
		try {
			const run = await bridges.optimizers.startRecipe({
				recipeId: HOSTED_SFT_FIXTURE_RECIPE_ID,
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

	const launchCraftaxNemotronSft = async () => {
		if (!bridges.optimizers || !nemotronSftRecipeAvailable) return;
		setBusy(true);
		setError(null);
		try {
			const run = await bridges.optimizers.startRecipe({
				recipeId: HOSTED_SFT_NEMOTRON_RECIPE_ID,
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

	const launchCraftaxSftSmoke = async () => {
		if (!bridges.optimizers || !sftLaunchConfirmed) return;
		setBusy(true);
		setError(null);
		try {
			const run = await bridges.optimizers.startRecipe({
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
		<div className="ws-page" data-testid="optimizers-page">
			<header className="ws-page-head">
				<button type="button" className="ws-btn ws-btn-ghost" onClick={onBack}>
					← Back
				</button>
				<div className="ws-page-head-text">
					<h1 className="ws-title">Optimizers</h1>
					<p className="ws-lede">Track local and cloud training runs, inspect progress, and open their visuals.</p>
				</div>
				<div className="ws-page-head-actions">
					<button className="ws-btn ws-btn-secondary" type="button" disabled={busy} onClick={() => void importLocal()} data-testid="import-local-optimizer">Import local</button>
					<button className="ws-btn ws-btn-secondary" type="button" disabled={busy} onClick={() => void syncCloud()} data-testid="sync-cloud-optimizers">Sync cloud</button>
				</div>
			</header>

			{error ? <p className="ws-note ws-note-danger" role="alert" data-testid="optimizer-error">{error}</p> : null}

			<div className="ws-card-group">
			<section className="ws-card ws-card-split" aria-labelledby="banking77-launch-title" data-testid="banking77-gepa-launch-card">
				<div className="ws-card-body">
					<span className="ws-eyebrow">Pinned recipe · Local process</span>
					<h2 className="ws-card-title" id="banking77-launch-title">Banking77 GEPA · Luna vs Sol</h2>
					<p className="ws-card-text">Two concurrent, isolated runs: one Luna-medium proposer and one Sol-medium proposer. At most {BANKING77_PAIR_ROLLOUTS} total rollouts, capped at ${BANKING77_PAIR_COST_USD.toFixed(2)} total.</p>
				</div>
				<div className="ws-card-aside">
					{!banking77RecipeAvailable ? <span className="ws-badge ws-badge-warn">Recipes unavailable</span> : null}
					<button className="ws-btn ws-btn-primary" type="button" disabled={busy || !banking77RecipeAvailable} onClick={() => setLauncherOpen(true)} data-testid="create-cloud-optimizer">
						<span data-testid="configure-banking77-gepa-smoke">Configure Luna vs Sol</span>
					</button>
				</div>
			</section>

			<section className="ws-card ws-card-split" aria-labelledby="hosted-sft-launch-title" data-testid="hosted-sft-launch-card">
				<div className="ws-card-body">
					<span className="ws-eyebrow">Pinned recipe · optimizers-beta</span>
					<h2 className="ws-card-title" id="hosted-sft-launch-title">Hosted SFT fixture</h2>
					<p className="ws-card-text">Stream <code>optimizer_event.v1</code> pages from a running optimizers-beta into <code>optimizer.sft.live.v1</code>. Fixture backend; no Tinker or provider charges.</p>
				</div>
				<div className="ws-card-aside">
					{!hostedSftRecipeAvailable ? <span className="ws-badge ws-badge-warn">Beta not configured</span> : null}
					<button className="ws-btn ws-btn-secondary" type="button" disabled={busy || !hostedSftRecipeAvailable} onClick={() => void launchHostedSftFixture()} data-testid="create-hosted-sft-optimizer">
						<span data-testid="start-hosted-sft-fixture">Start hosted SFT fixture</span>
					</button>
				</div>
			</section>

			<section className="ws-card ws-card-split" aria-labelledby="craftax-nemotron-sft-launch-title" data-testid="craftax-nemotron-sft-launch-card">
				<div className="ws-card-body">
					<span className="ws-eyebrow">Hosted recipe · Tinker · local Craftax slot</span>
					<h2 className="ws-card-title" id="craftax-nemotron-sft-launch-title">Craftax Nemotron 3.5 Lightning Tinker SFT</h2>
					<p className="ws-card-text">Hosted <code>algorithm_id: sft</code> against a local Craftax slot. Student LoRA on Tinker. Default student is Nemotron 3.5 Lightning from <code>docs/sft_tinker_base_models.toml</code>. Checkpoint campaigns stream reward and cost only when the producer emits them.</p>
				</div>
				<div className="ws-card-aside">
					{!nemotronSftRecipeAvailable ? <span className="ws-badge ws-badge-warn">Beta or local slot not ready</span> : null}
					<button className="ws-btn ws-btn-secondary" type="button" disabled={busy || !nemotronSftRecipeAvailable} onClick={() => void launchCraftaxNemotronSft()} data-testid="start-craftax-nemotron-sft">Start Craftax Nemotron SFT</button>
				</div>
			</section>

			<section className="ws-card ws-card-split" aria-labelledby="craftax-sft-launch-title" data-testid="craftax-sft-launch-card">
				<div className="ws-card-body">
					<span className="ws-eyebrow">Pinned recipe · Local process + Tinker</span>
					<h2 className="ws-card-title" id="craftax-sft-launch-title">Craftax GPT-OSS SFT smoke</h2>
					<p className="ws-card-text">Four teacher rollouts, {CRAFTAX_SFT_STEPS} LoRA steps, and base-vs-adapter evaluation across {CRAFTAX_SFT_ROLLOUTS} total environment rollouts.</p>
				</div>
				<div className="ws-card-aside">
					{!craftaxSftRecipeAvailable ? <span className="ws-badge ws-badge-warn">Recipe unavailable</span> : null}
					<button className="ws-btn ws-btn-secondary" type="button" disabled={busy || !craftaxSftRecipeAvailable} onClick={() => setSftLauncherOpen(true)} data-testid="create-craftax-sft-optimizer">
						<span data-testid="configure-craftax-sft-smoke">Configure bounded SFT run</span>
					</button>
				</div>
			</section>
			</div>

			{launcherOpen ? (
				<div className="ws-dialog-scrim" role="dialog" aria-modal="true" aria-labelledby="banking77-dialog-title" data-testid="banking77-gepa-launch-dialog">
					<div className="ws-dialog">
						<div className="ws-dialog-head">
							<div className="ws-stack-tight"><span className="ws-eyebrow">Review before starting paid compute</span><h2 className="ws-dialog-title" id="banking77-dialog-title">Banking77 GEPA · Luna vs Sol</h2></div>
							<button type="button" className="ws-btn ws-btn-ghost ws-btn-small" aria-label="Close Banking77 GEPA launcher" onClick={() => setLauncherOpen(false)} data-testid="close-banking77-gepa-launcher">×</button>
						</div>
						<dl className="ws-kv" data-testid="banking77-gepa-bounds">
							<dt>Optimizer</dt><dd>GEPA × 2</dd><dt>Dataset</dt><dd>Banking77</dd>
							<dt>Proposers</dt><dd>Luna medium · Sol medium</dd><dt>Run IDs</dt><dd>Two, assigned on start</dd>
							<dt>Rollout ceiling</dt><dd>{BANKING77_PAIR_ROLLOUTS} total</dd><dt>Cost ceiling</dt><dd>${BANKING77_PAIR_COST_USD.toFixed(2)} total</dd>
						</dl>
						<p className="ws-dialog-copy">Each run uses 50 train rows, 20-example minibatches, and 50 heldout rows. Proposers use the signed-in Codex ChatGPT account; Banking77 candidate evaluation uses the trusted Desktop OpenAI credential.</p>
						<label className="ws-dialog-confirm"><input type="checkbox" checked={launchConfirmed} onChange={(event) => setLaunchConfirmed(event.target.checked)} data-testid="confirm-banking77-gepa-cost" /> I approve both bounded runs and their API usage.</label>
						<div className="ws-btn-row ws-btn-row-end">
							<button type="button" className="ws-btn ws-btn-secondary" onClick={() => setLauncherOpen(false)}>Cancel</button>
							<button type="button" className="ws-btn ws-btn-primary" disabled={busy || !launchConfirmed} onClick={() => void launchBanking77Comparison()} data-testid="start-banking77-gepa-smoke">{busy ? "Starting both…" : "Start Luna + Sol comparison"}</button>
						</div>
					</div>
				</div>
			) : null}

			{sftLauncherOpen ? (
				<div className="ws-dialog-scrim" role="dialog" aria-modal="true" aria-labelledby="craftax-sft-dialog-title" data-testid="craftax-sft-launch-dialog">
					<div className="ws-dialog">
						<div className="ws-dialog-head">
							<div className="ws-stack-tight"><span className="ws-eyebrow">Review before starting paid compute</span><h2 className="ws-dialog-title" id="craftax-sft-dialog-title">Craftax GPT-OSS bounded SFT smoke</h2></div>
							<button type="button" className="ws-btn ws-btn-ghost ws-btn-small" aria-label="Close Craftax SFT launcher" onClick={() => setSftLauncherOpen(false)}>×</button>
						</div>
						<dl className="ws-kv" data-testid="craftax-sft-bounds">
							<dt>Teacher</dt><dd>GPT-OSS-120B via Groq</dd><dt>Student</dt><dd>GPT-OSS-20B LoRA via Tinker</dd>
							<dt>Teacher rollouts</dt><dd>4</dd><dt>Training steps</dt><dd>{CRAFTAX_SFT_STEPS}</dd>
							<dt>Held-out seeds</dt><dd>2 × base and adapter</dd><dt>Environment rollouts</dt><dd>{CRAFTAX_SFT_ROLLOUTS} maximum</dd>
						</dl>
						<p className="ws-dialog-copy">Uses the trusted Craftax binary (reusing port 8098 or owning it for this run) plus Groq and Tinker credentials in the trusted Desktop environment. Provider charges apply; this recipe is bounded by rollouts and steps, not by a dollar ceiling.</p>
						<label className="ws-dialog-confirm"><input type="checkbox" checked={sftLaunchConfirmed} onChange={(event) => setSftLaunchConfirmed(event.target.checked)} data-testid="confirm-craftax-sft-compute" /> I approve the bounded Groq and Tinker compute.</label>
						<div className="ws-btn-row ws-btn-row-end">
							<button type="button" className="ws-btn ws-btn-secondary" onClick={() => setSftLauncherOpen(false)}>Cancel</button>
							<button type="button" className="ws-btn ws-btn-primary" disabled={busy || !sftLaunchConfirmed} onClick={() => void launchCraftaxSftSmoke()} data-testid="start-craftax-sft-smoke">{busy ? "Starting…" : "Start bounded Craftax SFT smoke"}</button>
						</div>
					</div>
				</div>
			) : null}

			<div className="ws-toolbar ws-toolbar-wrap" data-testid="optimizer-toolbar">
				<div className="ws-toolbar-filters">
					<label className="ws-search">
						<span aria-hidden>⌕</span>
						<input aria-label="Search optimizers" placeholder="Search runs" value={search} onChange={(event) => setSearch(event.target.value)} data-testid="optimizers-search" />
					</label>
					<select className="ws-select" aria-label="Status filter" value={status} onChange={(e) => setStatus(e.target.value)}>
						<option value="all">All statuses</option><option value="running">Running</option><option value="paused">Paused</option><option value="completed">Completed</option><option value="failed">Failed</option><option value="queued">Queued</option>
					</select>
					<select className="ws-select" aria-label="Algorithm filter" value={algorithm} onChange={(e) => setAlgorithm(e.target.value)}>
						<option value="all">All algorithms</option>
						{algorithms.map((item) => <option key={item.id} value={item.id}>{item.title} · {item.availability}</option>)}
					</select>
					<select className="ws-select" aria-label="Source filter" value={source} onChange={(e) => setSource(e.target.value)}>
						<option value="all">All sources</option><option value="local">Local</option><option value="hosted">Hosted</option><option value="cloud">Cloud</option>
					</select>
				</div>
			</div>

			<div className="ws-workbench">
				<section aria-label="Optimizer runs">
					<div className="ws-list-head"><div className="ws-list-head-label"><span className="ws-eyebrow">Runs</span><strong>{runs.length} total</strong></div><div className="ws-btn-row"><span className="ws-faint">Demo data</span><button className="ws-btn ws-btn-secondary ws-btn-small" type="button" disabled={busy} onClick={() => void seedFixture("gepa")} data-testid="seed-gepa-fixture">GEPA</button><button className="ws-btn ws-btn-secondary ws-btn-small" type="button" disabled={busy} onClick={() => void seedFixture("goex")}>GELO</button><button className="ws-btn ws-btn-secondary ws-btn-small" type="button" disabled={busy} onClick={() => void seedFixture("sft")}>SFT</button></div></div>
					<ul className="ws-list">
						{runs.map((run) => (
							<li key={run.id}>
								<button
									type="button"
									className={`ws-item ws-item-button${selectedId === run.id ? " is-selected" : ""}`}
									aria-current={selectedId === run.id}
									data-testid={`optimizer-run-${run.id}`}
									onClick={() => setSelectedId(run.id)}
								>
									<span className="ws-item-main"><span className="ws-item-title"><span className="ws-tag">{algorithmLabel(run.algorithmId)}</span> {run.objective ?? run.id}</span><small className="ws-item-meta">{formatWhen(run.finishedAt ?? run.startedAt ?? run.createdAt)}</small></span>
									<span className="ws-item-aside-stack"><span className={`ws-badge ${optimizerStatusClass(run.status)}`}>{run.status}</span><small className="ws-item-meta">{run.source} · {reportedCost(run.usage.costUsd)}</small></span>
								</button>
							</li>
						))}
						{runs.length === 0 ? <li className="ws-empty"><strong className="ws-empty-title">No optimizer runs yet</strong><p>Import a local run, connect cloud history, or start the pinned Banking77 comparison.</p></li> : null}
					</ul>
				</section>

				<section className="ws-inspector" aria-label="Optimizer inspector">
					{selected ? (
						<div className="ws-stack ws-stack-loose" data-testid="optimizer-inspector">
							<div className="ws-inspector-head"><span className="ws-eyebrow">Run details</span><h2 className="ws-inspector-title">{algorithmLabel(selected.algorithmId)}</h2><p className="ws-lede">{selected.objective ?? selected.id}</p></div>
							<dl className="ws-kv">
								<dt>Status</dt><dd>{selected.status}</dd>
								<dt>Source</dt><dd>{selected.source}</dd>
								<dt>Execution</dt><dd data-testid="optimizer-execution-mode">{selectedExecution}</dd>
								<dt>Live events</dt><dd>{selected.capabilities.streamEvents ? "Available" : "Replay / refresh"}</dd>
								<dt>Cursor</dt><dd>{selected.cursorSeq}</dd>
								<dt>Cost</dt><dd>{reportedCost(selected.usage.costUsd)}</dd>
								<dt>Created</dt><dd>{formatWhen(selected.createdAt)}</dd>
							</dl>
							{selectedDiagnostic ? (
								<section className="ws-note ws-note-danger" role="alert" data-testid="optimizer-diagnostic">
									<span className="ws-eyebrow">Why it stopped</span>
									<strong>{selectedDiagnostic.title}</strong>
									<p>{selectedDiagnostic.message}</p>
									{selectedDiagnostic.field ? <code>{selectedDiagnostic.field}</code> : null}
									{selectedDiagnostic.raw ? (
										<details>
											<summary>Show technical details</summary>
											<pre data-testid="optimizer-stderr-tail">{selectedDiagnostic.raw}</pre>
										</details>
									) : null}
									{selectedDiagnostic.logPath ? <small>Log · {fileName(selectedDiagnostic.logPath)}</small> : null}
								</section>
							) : null}
							{selectedRunDirectory ? (
								<details className="ws-note" data-testid="optimizer-run-files">
									<summary>Logs &amp; artifacts</summary>
									<code className="ws-mono">{selectedRunDirectory}</code>
									<ul><li>workshop.stdout.log</li><li>workshop.stderr.log</li><li>events.jsonl</li><li>result_manifest.json</li></ul>
								</details>
							) : null}
							<div className="ws-btn-row">
								<button className="ws-btn ws-btn-secondary" type="button" disabled={busy} onClick={() => void openSelectedVisual()} data-testid="open-optimizer-visual">Open visual</button>
								<button className="ws-btn ws-btn-secondary" type="button" disabled={busy} onClick={() => void refreshSelected()} data-testid="refresh-optimizer-run">Refresh</button>
								{selected.capabilities.pause && selected.status === "running" ? <button className="ws-btn ws-btn-secondary" type="button" disabled={busy} onClick={() => void controlSelected("pause")} data-testid="pause-optimizer-run">Pause</button> : null}
								{selected.capabilities.resume && selected.status === "paused" ? <button className="ws-btn ws-btn-secondary" type="button" disabled={busy} onClick={() => void controlSelected("resume")} data-testid="resume-optimizer-run">Resume</button> : null}
								{selected.capabilities.cancel && !["completed", "failed", "cancelled"].includes(selected.status) ? <button className="ws-btn ws-btn-danger" type="button" disabled={busy} onClick={() => void controlSelected("cancel")} data-testid="cancel-optimizer-run">Cancel</button> : null}
							</div>
						</div>
					) : (
						<div className="ws-empty"><strong className="ws-empty-title">Select a run</strong><p>Run details, usage, and linked visuals appear here.</p></div>
					)}
				</section>
			</div>
		</div>
	);
}
