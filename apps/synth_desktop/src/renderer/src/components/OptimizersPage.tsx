import { useCallback, useEffect, useMemo, useState } from "react";
import type { OptimizerAlgorithmInfo, OptimizerRunRecord } from "@synth/runtime-protocol";
import type { OptimizerRecipeInfo, PluginActionReceipt, PluginLifecycleOperation, PluginStatus } from "../bridge/types";
import { bridges } from "../runtime/desktopBridge";
import { findPluginStatus, pluginPresentation, type PluginPresentation } from "../runtime/pluginPresentation";

type OptimizerGuide = {
	id: "gepa" | "go-ex" | "sft" | "eval";
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
	/** Owned by useAppController; this page no longer reads the registry itself. */
	pluginStatuses?: readonly PluginStatus[] | null;
	onRefreshPlugins?: () => Promise<void>;
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
	if (id === "eval") return "Eval";
	return id;
}

type EvalScorecard = {
	id: string;
	label: string;
	stage: string;
	isBaseline: boolean;
	trials?: { total: number; valid: number; failed: number };
	metrics?: Array<{ metric: string; mean: number | null; count: number }>;
	pairedLift?: number | null;
};

type EvalSelection = {
	status: string;
	winner_id: string | null;
	primary_metric: string;
	lift: number | null;
	min_lift: number;
	reason: string;
};

type EvalState = {
	scorecards: EvalScorecard[];
	selection: EvalSelection | null;
	runtime: Record<string, unknown>;
	evidenceDir: string | null;
};

function sliceData(slice: unknown): Record<string, unknown> {
	const data = (slice as { data?: unknown })?.data;
	return (data && typeof data === "object" ? data : {}) as Record<string, unknown>;
}

function formatMetric(value: number | null | undefined): string {
	return value == null ? "—" : value.toFixed(3);
}

function formatLift(value: number | null | undefined): string {
	if (value == null) return "—";
	return `${value > 0 ? "+" : ""}${value.toFixed(3)}`;
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

/**
 * Lifecycle actions offered to the human, mirroring what `plugin_manage`
 * already exposes to agents. Availability here only decides what to *offer*;
 * whether an action is permitted is decided natively by the approval broker and
 * the active-run guards, and every outcome comes back as a receipt.
 */
type LifecycleAction = {
	operation: PluginLifecycleOperation;
	label: string;
	destructive?: boolean;
	/** Confirmation copy; omitted for reversible actions. */
	confirm?: (status: PluginStatus, presentation: PluginPresentation) => string;
	available: (status: PluginStatus, presentation: PluginPresentation) => boolean;
};

const LIFECYCLE_ACTIONS: readonly LifecycleAction[] = [
	{
		operation: "install",
		label: "Install",
		available: (status) => status.phase === "not_installed"
	},
	{
		operation: "enable",
		label: "Enable",
		available: (status) => status.phase !== "not_installed" && !status.enabled
	},
	{
		operation: "start",
		label: "Start",
		available: (status) => status.enabled && (status.phase === "installed" || status.phase === "stopped")
	},
	{
		operation: "stop",
		label: "Stop",
		confirm: (_status, presentation) => presentation.activeRuns > 0
			? `Stop the optimizer service while ${presentation.activeRuns} run(s) are active? The service refuses this until they finish.`
			: "Stop the optimizer service? Runs and their artifacts are retained.",
		available: (status) => status.enabled && (status.phase === "ready" || status.phase === "degraded")
	},
	{
		operation: "update",
		label: "Update",
		available: (status) => status.enabled && status.installedVersion != null
			&& status.installedVersion !== status.catalogVersion
	},
	{
		operation: "disable",
		label: "Disable",
		// `disable` clears the registry flag only — the sidecar keeps running
		// and there is no native active-run guard on it, so say so plainly.
		confirm: (_status, presentation) => presentation.activeRuns > 0
			? `Disable Optimizers? ${presentation.activeRuns} run(s) keep running; only the plugin is turned off.`
			: "Disable Optimizers? Installed files and runs are retained.",
		available: (status) => status.phase !== "not_installed" && status.enabled
	},
	{
		operation: "remove",
		label: "Remove",
		destructive: true,
		confirm: () => "Remove the installed optimizer distribution? Runs and artifacts are retained; the distribution is deleted and must be downloaded again.",
		available: (status) => status.installedVersion != null
	}
];

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

export function OptimizersPage({
	onOpenVisual,
	onStartAgent,
	onBack,
	pluginStatuses = null,
	onRefreshPlugins
}: Props) {
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
	const [startingSftFixture, setStartingSftFixture] = useState(false);
	const [evalRecipes, setEvalRecipes] = useState<OptimizerRecipeInfo[]>([]);
	const [evalState, setEvalState] = useState<EvalState | null>(null);
	// Set only by setReleaseChannel, which returns a fresh status; otherwise the
	// app controller's listing is the source of truth.
	const [pluginOverride, setPluginOverride] = useState<PluginStatus | null>(null);
	const [changingReleaseChannel, setChangingReleaseChannel] = useState(false);
	const plugin = pluginOverride ?? findPluginStatus(pluginStatuses, "optimizers");
	const presentation = pluginPresentation(plugin);

	const [lifecycleBusy, setLifecycleBusy] = useState<PluginLifecycleOperation | null>(null);
	const [receipt, setReceipt] = useState<PluginActionReceipt | null>(null);

	const refreshPlugin = useCallback(async () => {
		setPluginOverride(null);
		await onRefreshPlugins?.();
	}, [onRefreshPlugins]);

	const runLifecycle = async (action: LifecycleAction) => {
		if (!bridges.plugins?.manage || !plugin) return;
		const question = action.confirm?.(plugin, presentation);
		if (question && !window.confirm(question)) return;
		setLifecycleBusy(action.operation);
		setError(null);
		try {
			const next = await bridges.plugins.manage(action.operation, "optimizers");
			setReceipt(next);
			// The native side may have rejected the approval rather than acted;
			// the receipt says which, so surface it rather than assuming success.
			if (next.error) setError(next.error);
			await refreshPlugin();
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setLifecycleBusy(null);
		}
	};

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
			bridges.optimizers.listRecipes().catch(() => [] as OptimizerRecipeInfo[])
		]);
		setRuns(nextRuns);
		setAlgorithms(nextAlgorithms);
		setEvalRecipes(nextRecipes.filter((recipe) => recipe.algorithmId === "eval"));
		if (!selectedId && nextRuns[0]) setSelectedId(nextRuns[0].id);
	}, [algorithm, search, selectedId, source, status]);

	// No plugin poller here. Registry status arrives from useAppController,
	// which subscribes to `optimizer:status`; this page polled it every 750 ms
	// and each poll re-probed the live sidecar.
	useEffect(() => {
		void refresh().catch((reason) => setError(String(reason)));
		const unlisten = bridges.optimizers?.onEvent?.(() => {
			void refresh().catch(() => undefined);
			void refreshPlugin().catch(() => undefined);
		});
		return () => unlisten?.();
	}, [refresh, refreshPlugin]);

	const selected = useMemo(
		() => runs.find((run) => run.id === selectedId) ?? null,
		[runs, selectedId]
	);

	useEffect(() => {
		if (!selected || selected.algorithmId !== "eval" || !bridges.optimizers) {
			setEvalState(null);
			return;
		}
		let live = true;
		void bridges.optimizers
			.getStateBatch(selected.id, ["eval.scorecard", "eval.evidence", "eval.runtime"])
			.then((slices) => {
				if (!live) return;
				const byId = new Map(
					(slices as Array<{ sliceId?: string }>).map((slice) => [slice?.sliceId, slice])
				);
				const evidence = sliceData(byId.get("eval.evidence"));
				setEvalState({
					scorecards: (sliceData(byId.get("eval.scorecard")).candidates ?? []) as EvalScorecard[],
					selection: (evidence.selection ?? null) as EvalSelection | null,
					runtime: sliceData(byId.get("eval.runtime")),
					evidenceDir: (evidence.evidenceDir ?? null) as string | null
				});
			})
			.catch(() => undefined);
		return () => {
			live = false;
		};
	}, [selected]);
	const setReleaseChannel = async (channel: "official" | "dev") => {
		if (!bridges.plugins) return;
		setChangingReleaseChannel(true);
		setError(null);
		try {
			setPluginOverride(await bridges.plugins.setReleaseChannel("optimizers", channel));
			void onRefreshPlugins?.();
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setChangingReleaseChannel(false);
		}
	};
	const pluginPhaseLabel = presentation.label;

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

	const startSftFixture = async () => {
		if (!bridges.optimizers) return;
		setStartingSftFixture(true);
		setError(null);
		try {
			const run = await bridges.optimizers.startRecipe({
				recipeId: "sft.hosted.fixture.v1",
				openVisual: true
			});
			setSelectedId(run.id);
			await refresh();
			const visualId = run.visualRefs.find((ref) => ref.kind === "visual")?.id;
			if (visualId) onOpenVisual(visualId);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setStartingSftFixture(false);
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
			"Local OSS GEPA or legacy optimizer run path (workspace, run dir, or events.jsonl)"
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
				? "Hosted service"
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
					{presentation.activeRuns > 0 ? (
						<p className="optimizer-plugin-active-runs" data-testid="optimizer-plugin-active-runs">
							{presentation.activeRuns} run{presentation.activeRuns === 1 ? "" : "s"} still active.
						</p>
					) : null}
					<div className="optimizer-plugin-actions" data-testid="optimizer-plugin-actions">
						{LIFECYCLE_ACTIONS.filter((action) => action.available(plugin, presentation)).map((action) => (
							<button
								key={action.operation}
								type="button"
								className={`secondary-button${action.destructive ? " optimizer-danger-button" : ""}`}
								data-testid={`plugin-${action.operation}`}
								disabled={lifecycleBusy !== null}
								onClick={() => void runLifecycle(action)}
							>
								{lifecycleBusy === action.operation ? `${action.label}…` : action.label}
							</button>
						))}
					</div>
					{receipt ? (
						<p className="optimizer-plugin-receipt" data-testid="plugin-action-receipt" role="status">
							{receipt.action} · {receipt.result}
							{receipt.error ? ` · ${receipt.error}` : ""}
							{receipt.retainedData ? ` · retained: ${receipt.retainedData}` : ""}
						</p>
					) : null}
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
							<button
								className="secondary-button"
								type="button"
								// A plugin that is disabled, stopped, uninstalled, or
								// unhealthy cannot take work; offering the launch would
								// fail deep inside the sidecar instead of here.
								disabled={startingAgent !== null || (plugin != null && !presentation.isUsable)}
								title={plugin != null && !presentation.isUsable && presentation.label
									? `Optimizers: ${presentation.label}`
									: undefined}
								onClick={() => void startAgent(guide)}
								data-testid={`start-${guide.id}-agent`}
							>
								{startingAgent === guide.id ? "Opening agent…" : "Plan with agent"}
							</button>
							{guide.id === "sft" ? (
								<>
									<button className="secondary-button" type="button" disabled={startingSftFixture} onClick={() => void startSftFixture()} data-testid="start-sft-fixture">
										{startingSftFixture ? "Starting fixture…" : "Run free fixture"}
									</button>
									<small>Public Optimizers fixture · no provider charges</small>
								</>
							) : null}
						</article>
					))}
				</div>
			</section>

			{evalRecipes.length > 0 ? (
				<section className="optimizer-recipes optimizer-eval-catalog" aria-labelledby="optimizer-eval-title">
					<div className="optimizer-recipes-head">
						<div><span className="optimizer-eyebrow">Eval · local</span><h2 id="optimizer-eval-title">Score staged policies</h2></div>
						<p>Fixed recipes run against pinned local container targets and do not install the Optimizers plugin.</p>
					</div>
					<div className="optimizer-recipe-grid">
						{evalRecipes.map((recipe) => {
							const limits = recipe.limits ?? {};
							const screening = (limits.screeningSeeds as number[] | undefined) ?? [];
							const confirmation = (limits.confirmationSeeds as number[] | undefined) ?? [];
							const selection = (limits.selection as Record<string, unknown> | undefined) ?? {};
							const available = recipe.availability === "available";
							return (
								<article className="optimizer-recipe-card" aria-labelledby={`optimizer-eval-${recipe.id}`} data-testid={`optimizer-eval-recipe-${recipe.id}`} key={recipe.id}>
									<div className="optimizer-recipe-top">
										<span className="optimizer-recipe-mark">EV</span>
										<span className={`optimizer-status ${available ? "completed" : "failed"}`}>{recipe.availability}</span>
									</div>
									<h3 id={`optimizer-eval-${recipe.id}`}>{recipe.title}</h3>
									<code className="optimizer-eval-id">{recipe.id}</code>
									<dl className="optimizer-eval-limits">
										<dt>Screen</dt><dd>{screening.join(", ") || "—"}</dd>
										<dt>Confirm</dt><dd>{confirmation.join(", ") || "—"}</dd>
										<dt>Primary</dt><dd>{String(selection.primary_metric ?? "—")}</dd>
										<dt>Decision</dt><dd>{String(selection.decision_mode ?? "—")}</dd>
										<dt>Parallel</dt><dd>{String(limits.max_parallel_trials ?? "—")}</dd>
									</dl>
									{recipe.availabilityReason ? <small data-testid={`optimizer-eval-blocked-${recipe.id}`}>{recipe.availabilityReason}</small> : null}
									<button
										className="secondary-button"
										type="button"
										disabled={!available || startingAgent !== null}
										data-testid={`start-eval-${recipe.id}`}
										onClick={() => void startAgent({
											id: "eval",
											label: "EV",
											name: recipe.title,
											description: recipe.description ?? "",
											flow: ["Stage", "Score", "Select"],
											prompt: `Run the Workshop eval recipe ${recipe.id} on policy variants in this project. Stage the policy files with optimizer_stage_eval_candidates using workspace-relative paths, kind python-code.v1, entrypoint policy:Policy, one labelled candidate each, marking the baseline; then call optimizer_start_recipe with the recipe id and returned candidate_set_id. Report the run status and selection status separately, the per-candidate scorecard, and the evidence directory.`
										})}
									>
										{startingAgent === "eval" ? "Opening agent…" : "Set up run"}
									</button>
								</article>
							);
						})}
					</div>
				</section>
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
							{evalState ? (
								<section className="optimizer-eval-scorecard" data-testid="optimizer-eval-scorecard">
									<span className="optimizer-eyebrow">Scorecard</span>
									<table>
										<thead><tr><th>Candidate</th><th>Stage</th><th>Valid</th><th>Failed</th><th>Primary</th><th>Lift</th></tr></thead>
										<tbody>
											{evalState.scorecards.map((card) => {
												const primary = evalState.selection?.primary_metric;
												const metric = card.metrics?.find((entry) => entry.metric === primary);
												return (
													<tr key={`${card.stage}:${card.id}`} data-testid={`eval-scorecard-row-${card.id}-${card.stage}`}>
														<td>{card.label}{card.isBaseline ? " · baseline" : ""}</td>
														<td>{card.stage}</td>
														<td>{card.trials?.valid ?? 0}</td>
														<td>{card.trials?.failed ?? 0}</td>
														<td>{formatMetric(metric?.mean)}</td>
														<td>{formatLift(card.pairedLift)}</td>
													</tr>
												);
											})}
										</tbody>
									</table>
									{evalState.selection ? (
										<dl className="optimizer-eval-selection" data-testid="optimizer-eval-selection">
											<dt>Selection</dt><dd>{evalState.selection.status}</dd>
											<dt>Lift</dt><dd>{formatLift(evalState.selection.lift)} / {evalState.selection.min_lift}</dd>
											<dt>Why</dt><dd>{evalState.selection.reason}</dd>
										</dl>
									) : null}
									{evalState.evidenceDir ? <code className="optimizer-eval-evidence" data-testid="optimizer-eval-evidence">{evalState.evidenceDir}</code> : null}
								</section>
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
