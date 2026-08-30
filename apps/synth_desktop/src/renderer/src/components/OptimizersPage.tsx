import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { OptimizerAlgorithmInfo, OptimizerRunRecord } from "@synth/runtime-protocol";
import type { HostedTrainingModel, OptimizerRecipeInfo, OptimizerRunOutputs, PluginActionReceipt, PluginLifecycleOperation, PluginStatus, SavedLoraCheckpoint, TrainingProjection } from "../bridge/types";
import { bridges } from "../runtime/desktopBridge";
import { canonicalEvalState, type CanonicalEvalState } from "../runtime/evalAggregate";
import { isLagunaCompatibleAdapter, LOCAL_FT_POLICY } from "../runtime/lagunaPolicies";
import { findPluginStatus, pluginPresentation, type PluginPresentation } from "../runtime/pluginPresentation";
import { isTerminalRunStatus } from "../runtime/runProgress/types";
import { starterPromptForRecipe, workshopStarter } from "../runtime/starterCatalog";
import { TrainingWorkspace } from "./TrainingWorkspace";
import { TrainingEvaluationCurve } from "./TrainingEvaluationCurve";
import { RunInspector } from "./optimizers/RunInspector";
import { algorithmLabel, formatWhen, runFacets, runTitle, runWhenMs, sealedWorkCounts, statusChipClass, statusText, truncateMiddle, workFractionLabel } from "./optimizers/runPresentation";

type OptimizerGuide = {
	id: "gepa" | "go-ex" | "sft" | "cispo" | "ppo" | "eval";
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
		description: "Collect strong demonstrations, train checkpoints, and compare the adapted model against its baseline. This Mac (MLX) or hosted.",
		flow: ["Collect", "Train", "Compare"],
		prompt: "Help me set up an SFT optimization in Workshop. Do not start compute yet. Ask whether I want This Mac (recipe sft.qwen35-2b.mlx.v1) or hosted Tinker. Never dial :8787 or name synth-mlx-rl. Wait for my explicit approval before starting paid compute."
	},
	{
		id: "cispo",
		label: "CI",
		name: "CISPO · slime reference",
		description: "Run on-policy training with the pinned slime CISPO objective. This Mac (MLX) or hosted after the clip canary.",
		flow: ["Preflight", "Roll out", "Train"],
		prompt: "Help me set up CISPO in Workshop. Do not start paid compute yet. Prefer recipe cispo.banking77.mlx.v1 on this Mac, or cispo.slime.hosted.v1 if hosted is admitted. Never draft a free-form HostedOptimizerClient.launch_training call. Wait for my explicit approval before launch."
	},
];

/**
 * The page's four surfaces. `runs` is the landing tab: the page's primary job
 * is inspecting work that exists, not launching more of it. No URL router
 * exists in this app, so the tab is component state; cross-surface links
 * (checkpoint → run, checkpoint → hosted launch) switch tabs explicitly
 * instead of scrolling a single long page.
 */
const OPTIMIZER_TABS = [
	{ id: "runs", label: "Runs" },
	{ id: "launch", label: "Launch" },
	{ id: "checkpoints", label: "Checkpoints" },
	{ id: "plugin", label: "Plugin" }
] as const;
type OptimizersTab = (typeof OPTIMIZER_TABS)[number]["id"];

type Props = {
	onOpenVisual: (visualId: string) => void;
	onStartAgent: (guide: OptimizerGuide) => Promise<void>;
	onBack: () => void;
	/** Data-selected registered container; binds workspace baseline evals. */
	selectedContainerId?: string | null;
	/** Owned by useAppController; this page no longer reads the registry itself. */
	pluginStatuses?: readonly PluginStatus[] | null;
	onRefreshPlugins?: () => Promise<void>;
	initialRunId?: string | null;
	initialStarterId?: string | null;
	onSelectedRunIdChange?: (runId: string | null) => void;
};

function isWorkspaceBaselineEval(recipe: OptimizerRecipeInfo): boolean {
	return recipe.algorithmId === "eval" && recipe.source === "workspace" && recipe.semantics === "baseline_eval";
}

function formatBytes(value: number | null | undefined): string {
	if (value == null) return "—";
	if (value < 1024) return `${value} B`;
	if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`;
	if (value < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MB`;
	return `${(value / 1024 ** 3).toFixed(1)} GB`;
}

/** The generated catalog types `algorithms` as `unknown`; read it defensively. */
type HostedAlgorithmSupport = { status?: string; block_reason?: string };

function hostedAlgorithmSupport(
	model: HostedTrainingModel | undefined,
	algorithm: string
): HostedAlgorithmSupport | undefined {
	const algorithms = model?.algorithms;
	if (!algorithms || typeof algorithms !== "object") return undefined;
	const entry = (algorithms as Record<string, unknown>)[algorithm];
	return entry && typeof entry === "object" ? entry as HostedAlgorithmSupport : undefined;
}

function evalSelectionReason(selection: CanonicalEvalState["aggregate"]["selection"]): string {
	return selection === "promotion_not_applicable"
		? "Baseline-only evaluation; no promotion decision applies."
		: "Promotion was applicable, but the evidence did not establish a winner.";
}

function formatMetric(value: number | null | undefined): string {
	// A metric no valid trial produced is unknown, not zero.
	return value == null ? "—" : value.toFixed(3);
}

function objectValue(value: unknown): Record<string, unknown> {
	return value && typeof value === "object" ? value as Record<string, unknown> : {};
}

function checkpointValue(value: unknown): Record<string, unknown> {
	const payload = objectValue(value);
	return objectValue(payload.checkpoint ?? payload);
}

type ErrorPresentation = {
	message: string;
	details?: string;
};

function stringifyDiagnostic(value: unknown): string | undefined {
	if (typeof value === "string") return value;
	if (value == null) return undefined;
	try {
		return JSON.stringify(value, null, 2);
	} catch {
		return undefined;
	}
}

/** Tauri errors are structured objects, never strings. Do not coerce one with
 * `publicError(error)`, which is how lifecycle failures became `[object Object]`. */
function presentError(reason: unknown): ErrorPresentation {
	if (reason instanceof Error) return { message: reason.message };
	if (typeof reason === "string") return { message: reason };
	if (reason && typeof reason === "object") {
		const value = reason as Record<string, unknown>;
		const message = [value.safeMessage, value.safe_message, value.message, value.error]
			.find((candidate): candidate is string => typeof candidate === "string" && candidate.trim().length > 0)
			?? "The optimizer operation failed.";
		const details = [value.responseBody, value.response_body, value.detail]
			.map(stringifyDiagnostic)
			.find((candidate): candidate is string => Boolean(candidate) && candidate !== message);
		return { message, details };
	}
	return { message: "The optimizer operation failed." };
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

export function OptimizersPage({
	onOpenVisual,
	onStartAgent,
	onBack,
	selectedContainerId = null,
	pluginStatuses = null,
	onRefreshPlugins,
	initialRunId = null,
	initialStarterId = null,
	onSelectedRunIdChange
}: Props) {
	const [tab, setTab] = useState<OptimizersTab>(initialStarterId ? "launch" : "runs");
	const [runs, setRuns] = useState<OptimizerRunRecord[]>([]);
	const [algorithms, setAlgorithms] = useState<OptimizerAlgorithmInfo[]>([]);
	const [search, setSearch] = useState("");
	const [status, setStatus] = useState("all");
	const [algorithm, setAlgorithm] = useState("all");
	const [source, setSource] = useState("all");
	// Client-side facets over the loaded records; the list command has no
	// recipe/container/model/date parameters, and the facets live in fields
	// the payload already carries (summary, inputRefs, executionBindings).
	const [recipeFilter, setRecipeFilter] = useState("all");
	const [containerFilter, setContainerFilter] = useState("all");
	const [modelFilter, setModelFilter] = useState("all");
	const [dateFrom, setDateFrom] = useState("");
	const [dateTo, setDateTo] = useState("");
	const [error, setError] = useState<string | null>(null);
	const [errorDetails, setErrorDetails] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const [selectedId, setSelectedId] = useState<string | null>(initialRunId);
	const [startingAgent, setStartingAgent] = useState<OptimizerGuide["id"] | null>(null);
	const [startingLocalSft, setStartingLocalSft] = useState(false);
	const [startingLocalCispo, setStartingLocalCispo] = useState(false);
	const [evalRecipes, setEvalRecipes] = useState<OptimizerRecipeInfo[]>([]);
	const [hostedCispoRecipe, setHostedCispoRecipe] = useState<OptimizerRecipeInfo | null>(null);
	const [localCispoRecipe, setLocalCispoRecipe] = useState<OptimizerRecipeInfo | null>(null);
	const [evalState, setEvalState] = useState<CanonicalEvalState | null>(null);
	const [trainingProjection, setTrainingProjection] = useState<TrainingProjection | null>(null);
	const trainingAlgorithm = "cispo" as const;
	const [trainingModel, setTrainingModel] = useState("openai/gpt-oss-20b");
	const [trainingTask, setTrainingTask] = useState("banking77");
	const [trainingContainerUrl, setTrainingContainerUrl] = useState("http://127.0.0.1:8000");
	const [trainingSteps, setTrainingSteps] = useState(2);
	const [trainingWallSeconds, setTrainingWallSeconds] = useState(300);
	const [trainingCostUsd, setTrainingCostUsd] = useState(0.1);
	const [trainingCheckpointEvery, setTrainingCheckpointEvery] = useState(1);
	const [trainingWarmStartCheckpointId, setTrainingWarmStartCheckpointId] = useState("");
	const [hostedTrainingModels, setHostedTrainingModels] = useState<HostedTrainingModel[]>([]);
	const [hostedModelCatalogRevision, setHostedModelCatalogRevision] = useState("");
	const [savedLoras, setSavedLoras] = useState<SavedLoraCheckpoint[]>([]);
	const [hostedSftWarmStarts, setHostedSftWarmStarts] = useState<SavedLoraCheckpoint[]>([]);
	const [savedLoraTotal, setSavedLoraTotal] = useState(0);
	const [savedLoraSearch, setSavedLoraSearch] = useState("");
	const [savedLoraScope, setSavedLoraScope] = useState<"all" | "mine" | "org">("all");
	const [savedLoraProvider, setSavedLoraProvider] = useState("all");
	const [savedLoraAlgorithm, setSavedLoraAlgorithm] = useState("all");
	const [savedLoraKind, setSavedLoraKind] = useState("all");
	const [savedLoraPlacement, setSavedLoraPlacement] = useState<"all" | "this_mac" | "hosted">("all");
	const [savedLoraBusy, setSavedLoraBusy] = useState(false);
	const [inferPrompt, setInferPrompt] = useState("");
	const [inferResult, setInferResult] = useState<string | null>(null);
	const [inferringId, setInferringId] = useState<string | null>(null);
	const [selectedRunCheckpoints, setSelectedRunCheckpoints] = useState<SavedLoraCheckpoint[]>([]);
	const [selectedCheckpointCounts, setSelectedCheckpointCounts] = useState({ total: 0, inference: 0, training: 0 });
	const [selectedRunOutputs, setSelectedRunOutputs] = useState<OptimizerRunOutputs | null>(null);
	// Set only by setReleaseChannel, which returns a fresh status; otherwise the
	// app controller's listing is the source of truth.
	const [pluginOverride, setPluginOverride] = useState<PluginStatus | null>(null);
	const [changingReleaseChannel, setChangingReleaseChannel] = useState(false);
	const plugin = pluginOverride ?? findPluginStatus(pluginStatuses, "optimizers");
	const presentation = pluginPresentation(plugin);

	useEffect(() => {
		onSelectedRunIdChange?.(selectedId);
	}, [onSelectedRunIdChange, selectedId]);

	const [lifecycleBusy, setLifecycleBusy] = useState<PluginLifecycleOperation | null>(null);
	const [receipt, setReceipt] = useState<PluginActionReceipt | null>(null);
	const selectedStarter = workshopStarter(initialStarterId);
	const orderedEvalRecipes = useMemo(() => {
		if (!selectedStarter) return evalRecipes;
		return [...evalRecipes].sort((left, right) =>
			Number(right.id === selectedStarter.recipeId) - Number(left.id === selectedStarter.recipeId)
		);
	}, [evalRecipes, selectedStarter]);

	useEffect(() => {
		if (!selectedStarter || tab !== "launch") return;
		window.setTimeout(() => {
			document.querySelector<HTMLElement>(`[data-testid="optimizer-eval-recipe-${selectedStarter.recipeId}"]`)
				?.scrollIntoView({ behavior: "smooth", block: "center" });
		}, 0);
	}, [selectedStarter, tab, evalRecipes.length]);

	const refreshPlugin = useCallback(async () => {
		setPluginOverride(null);
		await onRefreshPlugins?.();
	}, [onRefreshPlugins]);

	/**
	 * Switch tabs, then bring one section into view and hand it focus. The
	 * timeout matters: the target section only exists after the tab renders.
	 */
	const revealSection = useCallback((nextTab: OptimizersTab, selector: string) => {
		setTab(nextTab);
		window.setTimeout(() => {
			const element = document.querySelector<HTMLElement>(selector);
			element?.scrollIntoView({ behavior: "smooth", block: "start" });
			element?.focus({ preventScroll: true });
		}, 0);
	}, []);

	const runLifecycle = async (action: LifecycleAction) => {
		if (!bridges.plugins?.manage || !plugin) return;
		const question = action.confirm?.(plugin, presentation);
		if (question && !window.confirm(question)) return;
		setLifecycleBusy(action.operation);
		setError(null);
		setErrorDetails(null);
		try {
			const next = await bridges.plugins.manage(action.operation, "optimizers");
			setReceipt(next);
			// The native side may have rejected the approval rather than acted;
			// the receipt says which, so surface it rather than assuming success.
			if (next.error) setError(next.error);
			await refreshPlugin();
		} catch (reason) {
			const failure = presentError(reason);
			setError(failure.message);
			setErrorDetails(failure.details ?? null);
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
		setHostedCispoRecipe(nextRecipes.find((recipe) => recipe.id === "cispo.slime.hosted.v1") ?? null);
		setLocalCispoRecipe(nextRecipes.find((recipe) => recipe.id === "cispo.mlx.v1") ?? null);
		if (!selectedId && nextRuns[0]) setSelectedId(nextRuns[0].id);
	}, [algorithm, search, selectedId, source, status]);

	// No plugin poller here. Registry status arrives from useAppController,
	// which subscribes to `optimizer:status`; this page polled it every 750 ms
	// and each poll re-probed the live sidecar.
	useEffect(() => {
		void refresh().catch((reason) => setError(presentError(reason).message));
		const unlisten = bridges.optimizers?.onEvent?.(() => {
			void refresh().catch(() => undefined);
			void refreshPlugin().catch(() => undefined);
		});
		return () => unlisten?.();
	}, [refresh, refreshPlugin]);

	useEffect(() => {
		let live = true;
		const loadHostedTrainingModels = bridges.optimizers?.hostedTrainingModels;
		if (typeof loadHostedTrainingModels !== "function") return () => { live = false; };
		void loadHostedTrainingModels().then((catalog) => {
			if (!live) return;
			setHostedTrainingModels(catalog.models);
			setHostedModelCatalogRevision(catalog.catalogRevision);
			if (!catalog.models.some((model) => model.modelId === trainingModel)) {
				const preferred = catalog.models.find((model) => hostedAlgorithmSupport(model, trainingAlgorithm)?.status !== "blocked");
				if (preferred) setTrainingModel(preferred.modelId);
			}
		}).catch((reason) => {
			if (live) setError(presentError(reason).message);
		});
		return () => { live = false; };
	}, [trainingAlgorithm]);

	const refreshSavedLoras = useCallback(async () => {
		if (typeof bridges.optimizers?.searchSavedLoras !== "function") return;
		setSavedLoraBusy(true);
		try {
			const page = await bridges.optimizers.searchSavedLoras({
				search: savedLoraSearch.trim() || undefined,
				scope: savedLoraScope,
				provider: savedLoraProvider === "all" ? undefined : savedLoraProvider,
				optimizerAlgorithm: savedLoraAlgorithm === "all" ? undefined : savedLoraAlgorithm as "sft" | "cispo" | "ppo",
				checkpointKind: savedLoraKind === "all" ? undefined : savedLoraKind,
				placement: savedLoraPlacement === "all" ? undefined : savedLoraPlacement,
				status: "ready",
				limit: 50
			});
			setSavedLoras(page.items);
			setSavedLoraTotal(page.total);
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			setSavedLoraBusy(false);
		}
	}, [savedLoraAlgorithm, savedLoraKind, savedLoraPlacement, savedLoraProvider, savedLoraScope, savedLoraSearch]);

	useEffect(() => {
		const timer = window.setTimeout(() => void refreshSavedLoras(), 250);
		return () => window.clearTimeout(timer);
	}, [refreshSavedLoras]);

	useEffect(() => {
		let live = true;
		const searchSavedLoras = bridges.optimizers?.searchSavedLoras;
		if (typeof searchSavedLoras !== "function") return () => { live = false; };
		void searchSavedLoras({
			provider: "tinker",
			optimizerAlgorithm: "sft",
			checkpointKind: "training",
			status: "ready",
			limit: 100
		}).then((page) => {
			if (!live) return;
			setHostedSftWarmStarts(page.items.filter((checkpoint) =>
				Boolean(checkpoint.lineage?.providerCheckpointReference ?? checkpoint.providerCheckpointReference)
			));
		}).catch((reason) => {
			if (live) setError(presentError(reason).message);
		});
		return () => { live = false; };
	}, []);

	const downloadSavedLora = async (checkpoint: SavedLoraCheckpoint) => {
		if (!bridges.optimizers) return;
		setSavedLoraBusy(true);
		try {
			const download = await bridges.optimizers.savedLoraDownload(checkpoint.checkpointId);
			window.open(download.url, "_blank", "noopener,noreferrer");
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			setSavedLoraBusy(false);
		}
	};

	const archiveSavedLora = async (checkpoint: SavedLoraCheckpoint) => {
		if (!bridges.optimizers) return;
		const local = checkpoint.placement === "this_mac";
		if (!window.confirm(`Archive “${checkpoint.name}”?${local ? "" : " The Wasabi object is retained."}`)) return;
		setSavedLoraBusy(true);
		try {
			await bridges.optimizers.archiveSavedLora(checkpoint.checkpointId);
			await refreshSavedLoras();
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			setSavedLoraBusy(false);
		}
	};

	const importSavedLora = async () => {
		if (!bridges.optimizers) return;
		const selected = await open({ directory: true, title: "Import mlx-lora.v1 folder" });
		const path = Array.isArray(selected) ? selected[0] : selected;
		if (!path) return;
		setSavedLoraBusy(true);
		try {
			await bridges.optimizers.importSavedLora(path);
			await refreshSavedLoras();
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			setSavedLoraBusy(false);
		}
	};

	const inferSavedLora = async (checkpoint: SavedLoraCheckpoint, family: "chat_completions" | "responses") => {
		if (!bridges.optimizers) return;
		const prompt = inferPrompt.trim() || "hello";
		setInferringId(`${checkpoint.checkpointId}:${family}`);
		setInferResult("");
		let painted = "";
		const stop = bridges.optimizers.onInferDelta?.((event) => {
			if (event.checkpointId !== checkpoint.checkpointId || event.family !== family) return;
			if (!event.delta) return;
			painted += event.delta;
			setInferResult(painted);
		});
		try {
			const body = family === "responses"
				? { input: prompt, model: checkpoint.baseModel, stream: true }
				: { messages: [{ role: "user", content: prompt }], model: checkpoint.baseModel, stream: true };
			const response = await bridges.optimizers.inferCheckpoint({
				checkpointId: checkpoint.checkpointId,
				family,
				body
			}) as Record<string, unknown>;
			const chatText = (response as { choices?: Array<{ message?: { content?: string } }> }).choices?.[0]?.message?.content;
			const responsesText = (response as { output?: Array<{ content?: Array<{ text?: string }> }> }).output?.[0]?.content?.[0]?.text;
			setInferResult(painted || chatText || responsesText || JSON.stringify(response, null, 2));
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			stop?.();
			setInferringId(null);
		}
	};

	const patchSavedLora = async (checkpoint: SavedLoraCheckpoint, patch: { name?: string; description?: string; tags?: string[] }) => {
		if (!bridges.optimizers?.patchSavedLora) return;
		setSavedLoraBusy(true);
		try {
			const next = await bridges.optimizers.patchSavedLora(checkpoint.checkpointId, patch);
			setSavedLoras((current) => current.map((item) => item.checkpointId === next.checkpointId ? next : item));
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			setSavedLoraBusy(false);
		}
	};

	const useInComposer = async (checkpoint: SavedLoraCheckpoint) => {
		setSavedLoraBusy(true);
		try {
			await bridges.laguna?.registerPolicy?.(checkpoint.checkpointId, LOCAL_FT_POLICY);
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			setSavedLoraBusy(false);
		}
	};

	const publishSavedLora = async (checkpoint: SavedLoraCheckpoint) => {
		if (!bridges.optimizers?.publishSavedLora) return;
		setSavedLoraBusy(true);
		try {
			await bridges.optimizers.publishSavedLora(checkpoint.checkpointId);
			await refreshSavedLoras();
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			setSavedLoraBusy(false);
		}
	};

	const openCheckpointRun = async (checkpoint: SavedLoraCheckpoint) => {
		if (!bridges.optimizers) return;
		const runId = checkpoint.lineage?.runId ?? checkpoint.runId;
		if (!runId) return;
		try {
			if (!runs.some((run) => run.id === runId)) {
				const run = await bridges.optimizers.get(runId);
				setRuns((current) => [run, ...current.filter((item) => item.id !== run.id)]);
			}
			setSelectedId(runId);
			revealSection("runs", "#optimizer-run-inspector");
		} catch (reason) {
			setError(presentError(reason).message);
		}
	};

	const showCheckpointInCatalog = (checkpoint: SavedLoraCheckpoint) => {
		setSavedLoraSearch(checkpoint.name);
		revealSection("checkpoints", "#optimizer-checkpoint-library");
	};

	const selected = useMemo(
		() => runs.find((run) => run.id === selectedId) ?? null,
		[runs, selectedId]
	);

	const facetsById = useMemo(
		() => new Map(runs.map((run) => [run.id, runFacets(run)] as const)),
		[runs]
	);
	const facetOptions = useMemo(() => {
		const recipes = new Set<string>();
		const containers = new Set<string>();
		const models = new Set<string>();
		for (const facets of facetsById.values()) {
			if (facets.recipeId) recipes.add(facets.recipeId);
			if (facets.containerId) containers.add(facets.containerId);
			if (facets.model) models.add(facets.model);
		}
		const sorted = (values: Set<string>) => [...values].sort((a, b) => a.localeCompare(b));
		return { recipes: sorted(recipes), containers: sorted(containers), models: sorted(models) };
	}, [facetsById]);
	const clientFiltersActive = recipeFilter !== "all" || containerFilter !== "all"
		|| modelFilter !== "all" || dateFrom !== "" || dateTo !== "";
	const clearClientFilters = () => {
		setRecipeFilter("all");
		setContainerFilter("all");
		setModelFilter("all");
		setDateFrom("");
		setDateTo("");
	};
	const visibleRuns = useMemo(() => {
		if (!clientFiltersActive) return runs;
		// Local midnight bounds: the inputs are dates, not instants.
		const fromMs = dateFrom ? new Date(`${dateFrom}T00:00:00`).getTime() : null;
		const toMs = dateTo ? new Date(`${dateTo}T23:59:59.999`).getTime() : null;
		return runs.filter((run) => {
			const facets = facetsById.get(run.id) ?? { recipeId: null, containerId: null, model: null };
			if (recipeFilter !== "all" && facets.recipeId !== recipeFilter) return false;
			if (containerFilter !== "all" && facets.containerId !== containerFilter) return false;
			if (modelFilter !== "all" && facets.model !== modelFilter) return false;
			const when = runWhenMs(run);
			if (fromMs != null && when < fromMs) return false;
			if (toMs != null && when > toMs) return false;
			return true;
		});
	}, [clientFiltersActive, containerFilter, dateFrom, dateTo, facetsById, modelFilter, recipeFilter, runs]);

	useEffect(() => {
		if (!selected || selected.source !== "cloud" || !bridges.optimizers) {
			setSelectedRunCheckpoints([]);
			setSelectedCheckpointCounts({ total: 0, inference: 0, training: 0 });
			setSelectedRunOutputs(null);
			return;
		}
		let live = true;
		void bridges.optimizers.runOutputs(selected.id).then((outputs) => {
			if (!live) return;
			setSelectedRunOutputs(outputs);
			setSelectedRunCheckpoints(outputs.modelCheckpoints);
			setSelectedCheckpointCounts({
				total: outputs.modelCheckpoints.length,
				inference: outputs.modelCheckpoints.filter((item) => item.checkpointKind === "inference").length,
				training: outputs.modelCheckpoints.filter((item) => item.checkpointKind === "training").length
			});
		}).catch((reason) => {
			if (live) setError(presentError(reason).message);
		});
		return () => { live = false; };
	}, [selected?.id, selected?.algorithmId, selected?.source, selected?.status]);

	useEffect(() => {
		if (!selected || selected.algorithmId !== "eval" || !bridges.optimizers) {
			setEvalState(null);
			return;
		}
		let live = true;
		void bridges.optimizers.runViewV2(selected.id)
			.then((view) => {
				if (live) setEvalState(canonicalEvalState(view, selected.id));
			})
			.catch((reason) => {
				if (!live) return;
				setEvalState(null);
				setError(presentError(reason).message);
			});
		return () => {
			live = false;
		};
	}, [selected?.algorithmId, selected?.cursorSeq, selected?.id, selected?.status]);

	useEffect(() => {
		if (!selected || selected.source !== "cloud" || !["sft", "cispo", "ppo"].includes(selected.algorithmId) || !bridges.optimizers) {
			setTrainingProjection(null);
			return;
		}
		let live = true;
		let timer: number | undefined;
		const reconcile = async () => {
			try {
				const snapshot = await bridges.optimizers?.reconcileTraining(selected.id);
				if (!live || !snapshot) return;
				setTrainingProjection(snapshot.projection);
				const updated = await bridges.optimizers?.get(selected.id);
				if (live && updated) setRuns((current) => current.map((run) => run.id === updated.id ? updated : run));
			} catch (reason) {
				if (live) setError(presentError(reason).message);
			}
			if (live && !isTerminalRunStatus(selected.status)) {
				timer = window.setTimeout(() => void reconcile(), 2500);
			}
		};
		void reconcile();
		return () => {
			live = false;
			if (timer != null) window.clearTimeout(timer);
		};
	}, [selected?.id, selected?.algorithmId, selected?.source, selected?.status]);
	const setReleaseChannel = async (channel: "official" | "dev") => {
		if (!bridges.plugins) return;
		setChangingReleaseChannel(true);
		setError(null);
		try {
			setPluginOverride(await bridges.plugins.setReleaseChannel("optimizers", channel));
			void onRefreshPlugins?.();
		} catch (reason) {
			setError(presentError(reason).message);
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
			setError(presentError(reason).message);
		} finally {
			setStartingAgent(null);
		}
	};

	const reviewTrainingLaunch = async () => {
		const guide = OPTIMIZER_GUIDES.find((item) => item.id === trainingAlgorithm);
		if (!guide) return;
		const warmStartReference = selectedWarmStart?.lineage?.providerCheckpointReference
			?? selectedWarmStart?.providerCheckpointReference;
		if (trainingAlgorithm === "cispo" && (!selectedWarmStart || !warmStartReference)) {
			setError("Select a ready Tinker SFT training-state checkpoint before launching hosted CISPO.");
			return;
		}
		if (trainingAlgorithm === "cispo" && selectedWarmStart && selectedWarmStart.baseModel !== trainingModel) {
			setError(`The selected SFT checkpoint uses ${selectedWarmStart.baseModel}; choose the same model for CISPO.`);
			return;
		}
		const warmStartLines = selectedWarmStart && warmStartReference
			? `\n- SFT checkpoint id: ${selectedWarmStart.checkpointId}\n- SFT provider state: ${warmStartReference}\n- producing SFT run: ${selectedWarmStart.lineage?.runId ?? selectedWarmStart.runId ?? "unknown"}\n- SFT base model: ${selectedWarmStart.baseModel}`
			: "";
		await startAgent({
			...guide,
			prompt: `${guide.prompt}\n\nThe user supplied this typed launch draft:\n- model: ${trainingModel}\n- task: ${trainingTask}\n- local container URL: ${trainingContainerUrl}\n- hard step cap: ${trainingSteps}\n- hard wall-clock cap: ${trainingWallSeconds} seconds\n- hard cost cap: $${trainingCostUsd}\n- checkpoint every: ${trainingCheckpointEvery} step(s)${warmStartLines}\n\nUse the synth-optimizers HostedTrainingSpec and HostedOptimizerClient.launch_training path so the client performs provider preflight, container capability validation, SynthTunnel setup, and lease ownership. For CISPO, put the exact SFT provider state in algorithm_config.initial_state_path, its checkpoint id in algorithm_config.source_checkpoint_id, and its producing run in algorithm_config.source_run_id; do not substitute latest, another checkpoint, or a sampler-only checkpoint. Echo the effective config, both capability hashes, the SFT checkpoint id, provider state, and producing run. If preflight is supported, ask for one final paid-compute confirmation and then launch; if it is unsupported, stop before spend and report the exact missing capability.`
		});
	};

	const startCheckpointWorkflow = async (
		action: "evaluate" | "resume" | "compare_report",
		checkpoint: Record<string, unknown>
	) => {
		if (!selected) return;
		const checkpointId = String(checkpoint.checkpoint_id ?? checkpoint.artifact_id ?? "");
		const actionPrompt = action === "evaluate"
			? "Evaluate this immutable checkpoint on the same task/container contract, then attach the evaluation evidence to the run."
			: action === "resume"
				? "Resume this run from the checkpoint using HostedOptimizerClient.resume_training with a fresh validated SynthTunnel lease and a new idempotent attempt."
				: "Compare this checkpoint and its evaluation against the baseline, attach or update the experiment visual, and create or update the report with run, attempt, checkpoint lineage, config/task digests, usage reconciliation, and artifact links.";
		await startAgent({
			id: action === "resume" && selected.algorithmId === "ppo" ? "ppo" : action === "resume" ? "cispo" : "eval",
			label: action === "evaluate" ? "EV" : action === "resume" ? "RE" : "RP",
			name: action === "evaluate" ? "Evaluate checkpoint" : action === "resume" ? "Resume checkpoint" : "Compare and report",
			description: `${actionPrompt} Run ${selected.id}, checkpoint ${checkpointId}.`,
			flow: action === "resume" ? ["Preflight", "Resume", "Follow"] : ["Evaluate", "Compare", "Report"],
			prompt: `${actionPrompt}\n\nRun: ${selected.id}\nAlgorithm: ${selected.algorithmId}\nCheckpoint: ${checkpointId}\nTask: ${String(objectValue(selected.summary).taskId ?? "from the sealed run config")}\nDo not substitute another checkpoint. Verify ready/evaluation/resume eligibility from canonical backend evidence before acting.`
		});
	};

	const startBoundedRecipe = async (recipeId: string, setter: (busy: boolean) => void) => {
		if (!bridges.optimizers) return;
		setter(true);
		setError(null);
		try {
			const run = await bridges.optimizers.startRecipe({
				recipeId,
				openVisual: true,
				containerId: selectedContainerId ?? undefined
			});
			setSelectedId(run.id);
			await refresh();
			const visualId = run.visualRefs?.find((ref) => ref.kind === "visual")?.id;
			if (visualId) onOpenVisual(visualId);
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			setter(false);
		}
	};

	const openSelectedVisual = async () => {
		if (!selected || !bridges.optimizers) return;
		setBusy(true);
		try {
			const run = await bridges.optimizers.openVisual(selected.id);
			const visualId = run.visualRefs?.find((ref) => ref.kind === "visual")?.id;
			if (visualId) onOpenVisual(visualId);
			await refresh();
		} catch (reason) {
			setError(presentError(reason).message);
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
			const visualId = run.visualRefs?.find((ref) => ref.kind === "visual")?.id;
			if (visualId) onOpenVisual(visualId);
		} catch (reason) {
			setError(presentError(reason).message);
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
			setError(presentError(reason).message);
		} finally {
			setBusy(false);
		}
	};

	const selectedExecution = selected
		? (selected.executionBindings ?? []).length > 0
			? (selected.executionBindings ?? []).map((binding) => binding.label ?? binding.kind).join(" · ")
			: selected.source === "hosted"
				? "Hosted service"
				: selected.source === "cloud"
					? "Cloud managed"
					: "Local process"
		: null;
	const selectedTrainingUsage = trainingProjection?.provider_usage ?? null;
	const selectedTrainingCheckpoints = trainingProjection?.checkpoints.map(checkpointValue) ?? [];
	const selectedTrainingEvaluations = trainingProjection?.evaluations ?? [];
	const selectedHostedModel = hostedTrainingModels.find((model) => model.modelId === trainingModel);
	const selectedHostedSupport = hostedAlgorithmSupport(selectedHostedModel, trainingAlgorithm);
	const selectedWarmStart = hostedSftWarmStarts.find((checkpoint) => checkpoint.checkpointId === trainingWarmStartCheckpointId);
	const hostedCispoAdmitted = hostedCispoRecipe?.availability === "available";
	const localCispoAvailable = localCispoRecipe?.availability === "available";
	const warmStartMismatch = trainingAlgorithm === "cispo" && selectedWarmStart?.baseModel !== trainingModel;
	const hostedLaunchBlocked = !hostedCispoAdmitted
		|| !selectedHostedSupport
		|| selectedHostedSupport.status === "blocked"
		|| (trainingAlgorithm === "cispo" && (!selectedWarmStart || warmStartMismatch));

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
			setError(presentError(reason).message);
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
			setError(presentError(reason).message);
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

			{error ? (
				<section className="inventory-error" role="alert" data-testid="optimizer-error">
					<p>{error}</p>
					{errorDetails ? (
						<details data-testid="optimizer-error-details">
							<summary>Show technical details</summary>
							<pre>{errorDetails}</pre>
						</details>
					) : null}
				</section>
			) : null}

			<nav className="optimizer-tabs" aria-label="Optimizer sections" data-testid="optimizer-tabs">
				{OPTIMIZER_TABS.map((item) => (
					<button
						key={item.id}
						type="button"
						aria-current={tab === item.id ? "page" : undefined}
						onClick={() => setTab(item.id)}
						data-testid={`optimizer-tab-${item.id}`}
					>
						{item.label}
						{item.id === "plugin" && presentation.label && (presentation.tone === "warning" || presentation.tone === "danger") ? (
							<span className="optimizer-tab-flag" data-tone={presentation.tone}>{presentation.label}</span>
						) : null}
					</button>
				))}
			</nav>

			{tab === "plugin" ? (plugin ? (
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
			) : (
				<div className="optimizer-empty" role="status" data-testid="optimizer-plugin-missing">
					<span className="optimizer-empty-icon" aria-hidden>◌</span>
					<strong>No plugin status reported</strong>
					<p>The plugin registry has not reported the Optimizers plugin in this session. Lifecycle controls appear when it does.</p>
				</div>
			)) : null}

			{tab === "launch" ? (<>
			<TrainingWorkspace onStartAgent={() => { const guide = OPTIMIZER_GUIDES.find((item) => item.id === "sft"); if (guide) void startAgent(guide); }} />

			<section className="optimizer-recipes" aria-labelledby="optimizer-recipes-title">
				<div className="optimizer-recipes-head">
					<div><span className="optimizer-eyebrow">Agent-guided setup</span><h2 id="optimizer-recipes-title">What do you want to optimize?</h2></div>
				</div>
				<div className="optimizer-recipe-grid">
					{OPTIMIZER_GUIDES.filter((guide) => guide.id !== "sft" && guide.id !== "cispo").map((guide) => (
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
									<button className="secondary-button" type="button" disabled={startingLocalSft || (plugin != null && !presentation.isUsable)} onClick={() => void startBoundedRecipe("sft.qwen35-2b.mlx.v1", setStartingLocalSft)} data-testid="start-sft-mlx">
										{startingLocalSft ? "Starting…" : "This Mac · Qwen 2B MLX"}
									</button>
									<small>Sidecar admits local MLX or hosted public SFT. Never dial :8787.</small>
								</>
							) : null}
							{guide.id === "cispo" ? (
								<>
									<button className="secondary-button" type="button" disabled={startingLocalCispo || (plugin != null && !presentation.isUsable)} onClick={() => void startBoundedRecipe("cispo.mlx.v1", setStartingLocalCispo)} data-testid="start-cispo-mlx">
										{startingLocalCispo ? "Starting…" : "This Mac · Banking77 CISPO"}
									</button>
									<small>Hosted CISPO stays fail-closed until the slime clip canary admits it.</small>
								</>
							) : null}
						</article>
					))}
				</div>
			</section>

			<section className="optimizer-training-launch" aria-labelledby="optimizer-training-launch-title" data-testid="optimizer-training-launch">
				<div className="optimizer-recipes-head">
					<div><span className="optimizer-eyebrow">Hosted on-policy training</span><h2 id="optimizer-training-launch-title">{hostedCispoAdmitted ? "Configure a bounded launch" : "Hosted CISPO is not available"}</h2></div>
				</div>
				{hostedCispoAdmitted ? <><div className="optimizer-training-form">
					<label><span>Algorithm</span><select value={trainingAlgorithm} disabled><option value="cispo">CISPO · slime reference</option></select></label>
					<label><span>Model</span><select value={trainingModel} onChange={(event) => setTrainingModel(event.target.value)}>{hostedTrainingModels.map((model) => { const support = hostedAlgorithmSupport(model, trainingAlgorithm); return <option key={model.modelId} value={model.modelId} disabled={support?.status === "blocked"}>{model.label} · {support?.status ?? "not validated"}</option>; })}</select></label>
					{trainingAlgorithm === "cispo" ? <label><span>SFT warm start</span><select aria-label="SFT warm-start checkpoint" value={trainingWarmStartCheckpointId} onChange={(event) => setTrainingWarmStartCheckpointId(event.target.value)} data-testid="hosted-cispo-warm-start"><option value="">Select a retained SFT training state…</option>{hostedSftWarmStarts.map((checkpoint) => <option key={checkpoint.checkpointId} value={checkpoint.checkpointId}>{checkpoint.name} · {checkpoint.baseModel} · step {checkpoint.step ?? "—"}</option>)}</select></label> : null}
					<label><span>Task</span><input value={trainingTask} onChange={(event) => setTrainingTask(event.target.value)} /></label>
					<label><span>Local Container URL</span><input value={trainingContainerUrl} onChange={(event) => setTrainingContainerUrl(event.target.value)} /></label>
					<label><span>Steps</span><input type="number" min={1} value={trainingSteps} onChange={(event) => setTrainingSteps(Math.max(1, Number(event.target.value)))} /></label>
					<label><span>Wall clock (seconds)</span><input type="number" min={1} value={trainingWallSeconds} onChange={(event) => setTrainingWallSeconds(Math.max(1, Number(event.target.value)))} /></label>
					<label><span>Cost cap (USD)</span><input type="number" min={0.01} step={0.01} value={trainingCostUsd} onChange={(event) => setTrainingCostUsd(Math.max(0.01, Number(event.target.value)))} /></label>
					<label><span>Checkpoint every</span><input type="number" min={1} value={trainingCheckpointEvery} onChange={(event) => setTrainingCheckpointEvery(Math.max(1, Number(event.target.value)))} /></label>
				</div>
				<div className="optimizer-training-launch-actions">
					<button className="primary-button" type="button" disabled={startingAgent !== null || hostedLaunchBlocked || !trainingModel.trim() || !trainingTask.trim() || !trainingContainerUrl.trim()} onClick={() => void reviewTrainingLaunch()} data-testid="review-hosted-training-launch">Review &amp; launch</button>
					{hostedLaunchBlocked ? <span className="optimizer-availability" data-available={false}>Unavailable</span> : null}
					<small>{trainingAlgorithm === "cispo" && !selectedWarmStart ? "Select a ready Tinker SFT training-state checkpoint; hosted CISPO never defaults to latest." : warmStartMismatch ? `Checkpoint/model mismatch: ${selectedWarmStart?.baseModel} ≠ ${trainingModel}.` : hostedLaunchBlocked ? selectedHostedSupport?.block_reason ?? "This model and algorithm combination is not admitted by the hosted catalog." : `Warm-start ${selectedWarmStart?.checkpointId} → CISPO → ${trainingTask}.`}{hostedModelCatalogRevision ? ` Catalog ${hostedModelCatalogRevision}; live provider preflight still required.` : ""}</small>
				</div></> : (
					<div className="optimizer-empty" data-testid="hosted-cispo-not-admitted" role="status">
						<strong>Hosted slime CISPO has not passed runtime admission.</strong>
						<p>{hostedCispoRecipe?.availabilityReason ?? "The Optimizers runtime does not advertise the hosted CISPO placement in this build."}</p>
						<p>Adding a model or SFT checkpoint will not unlock hosted CISPO.</p>
						{localCispoAvailable ? (
							<button className="primary-button" type="button" disabled={startingLocalCispo || (plugin != null && !presentation.isUsable)} onClick={() => void startBoundedRecipe("cispo.mlx.v1", setStartingLocalCispo)} data-testid="start-cispo-mlx-from-hosted-block">
								{startingLocalCispo ? "Starting…" : "Run CISPO on this Mac"}
							</button>
						) : <p data-testid="local-cispo-not-available">This Mac CISPO is also unavailable: {localCispoRecipe?.availabilityReason ?? "the runtime did not advertise the local recipe"}.</p>}
					</div>
				)}
			</section>
			</>) : null}

			{tab === "checkpoints" ? (
			<section id="optimizer-checkpoint-library" className="optimizer-checkpoint-library" aria-labelledby="optimizer-checkpoint-library-title" data-testid="optimizer-checkpoint-library" tabIndex={-1}>
				<div className="optimizer-recipes-head">
					<div><span className="optimizer-eyebrow">This Mac MLX · hosted Tinker SFT/CISPO</span><h2 id="optimizer-checkpoint-library-title">Checkpoint catalog</h2></div>
					<p>{savedLoraTotal} visible. Inference LoRAs can be called with Chat Completions or Responses.</p>
				</div>
				<div className="optimizer-checkpoint-filters">
					<label className="optimizer-search"><span aria-hidden>⌕</span><input aria-label="Search saved LoRA checkpoints" placeholder="Search names, models, references, or tags" value={savedLoraSearch} onChange={(event) => setSavedLoraSearch(event.target.value)} data-testid="saved-lora-search" /></label>
					<select aria-label="Checkpoint placement" value={savedLoraPlacement} onChange={(event) => setSavedLoraPlacement(event.target.value as "all" | "this_mac" | "hosted")}>
						<option value="all">This Mac + hosted</option><option value="this_mac">This Mac</option><option value="hosted">Hosted</option>
					</select>
					<select aria-label="Checkpoint ownership scope" value={savedLoraScope} onChange={(event) => setSavedLoraScope(event.target.value as "all" | "mine" | "org")}>
						<option value="all">Mine + organization</option><option value="mine">Mine</option><option value="org">Organization</option>
					</select>
					<select aria-label="Checkpoint provider" value={savedLoraProvider} onChange={(event) => setSavedLoraProvider(event.target.value)}>
						<option value="all">All providers</option><option value="mlx">MLX</option><option value="tinker">Tinker</option><option value="river">River</option><option value="synth">Synth</option><option value="imported">Imported</option>
					</select>
					<select aria-label="Checkpoint algorithm" value={savedLoraAlgorithm} onChange={(event) => setSavedLoraAlgorithm(event.target.value)}>
						<option value="all">All algorithms</option><option value="sft">SFT</option><option value="cispo">CISPO</option><option value="ppo">PPO</option>
					</select>
					<select aria-label="Checkpoint kind" value={savedLoraKind} onChange={(event) => setSavedLoraKind(event.target.value)}>
						<option value="all">All checkpoint kinds</option><option value="inference">Inference LoRA</option><option value="training">Training state</option>
					</select>
					<button className="secondary-button" type="button" disabled={savedLoraBusy} onClick={() => void refreshSavedLoras()}>{savedLoraBusy ? "Loading…" : "Refresh"}</button>
					<button className="secondary-button" type="button" disabled={savedLoraBusy} onClick={() => void importSavedLora()}>Import folder</button>
				</div>
				<label className="optimizer-search"><span>Prompt</span><input aria-label="Checkpoint inference prompt" placeholder="Prompt for Chat Completions or Responses" value={inferPrompt} onChange={(event) => setInferPrompt(event.target.value)} data-testid="checkpoint-infer-prompt" /></label>
				{inferResult !== null ? <pre className="optimizer-eval-evidence" data-testid="checkpoint-infer-result">{inferResult}</pre> : null}
				<div className="optimizer-checkpoint-grid">
					{savedLoras.map((checkpoint) => (
						<article className="optimizer-checkpoint-card" key={checkpoint.checkpointId} data-testid={`saved-lora-${checkpoint.checkpointId}`}>
							<div className="optimizer-recipe-top"><span className="optimizer-recipe-mark">LR</span><span className={`optimizer-status ${checkpoint.status}`}>{checkpoint.visibility === "private" ? "Private" : "Organization"}</span></div>
							<input className="optimizer-checkpoint-name" aria-label="Checkpoint name" defaultValue={checkpoint.name} data-testid={`saved-lora-name-${checkpoint.checkpointId}`} key={`${checkpoint.checkpointId}-name-${checkpoint.updatedAt ?? checkpoint.name}`} onBlur={(event) => { const name = event.target.value.trim(); if (name && name !== checkpoint.name) void patchSavedLora(checkpoint, { name }); }} />
							<p>{checkpoint.baseModel}</p>
							<label className="optimizer-search"><span>Notes</span><input aria-label="Checkpoint notes" defaultValue={checkpoint.description} key={`${checkpoint.checkpointId}-notes-${checkpoint.updatedAt ?? ""}`} onBlur={(event) => { const description = event.target.value; if (description !== checkpoint.description) void patchSavedLora(checkpoint, { description }); }} /></label>
							<label className="optimizer-search"><span>Tags</span><input aria-label="Checkpoint tags" defaultValue={checkpoint.tags.join(", ")} placeholder="comma-separated tags" key={`${checkpoint.checkpointId}-tags-${checkpoint.tags.join(",")}`} onBlur={(event) => { const tags = event.target.value.split(",").map((tag) => tag.trim()).filter(Boolean); if (tags.join(",") !== checkpoint.tags.join(",")) void patchSavedLora(checkpoint, { tags }); }} /></label>
							<dl><dt>Placement</dt><dd>{checkpoint.placement === "this_mac" ? "This Mac" : "Hosted"}</dd><dt>Base</dt><dd>{checkpoint.baseModel}</dd><dt>Algorithm</dt><dd>{checkpoint.lineage?.optimizerAlgorithm ?? checkpoint.optimizerAlgorithm ?? "Imported"}</dd><dt>Run</dt><dd>{checkpoint.lineage?.runId ?? checkpoint.runId ?? "—"}</dd><dt>Attempt</dt><dd>{checkpoint.lineage?.attemptId ?? checkpoint.attemptId ?? "—"}</dd><dt>Source</dt><dd>{checkpoint.lineage?.sourceCheckpointId ?? checkpoint.sourceCheckpointId ?? "—"}</dd><dt>Provider</dt><dd>{checkpoint.provider} · {checkpoint.checkpointKind}</dd><dt>Rank / step</dt><dd>{checkpoint.loraRank ?? "—"} / {checkpoint.step ?? "—"}</dd><dt>Storage</dt><dd>{checkpoint.storage.backend} · {formatBytes(checkpoint.storage.sizeBytes)}</dd><dt>Saved</dt><dd>{checkpoint.updatedAt ? formatWhen(checkpoint.updatedAt) : "—"}</dd></dl>
							{checkpoint.tags.length > 0 ? <div className="optimizer-checkpoint-tags">{checkpoint.tags.map((tag) => <span key={tag}>{tag}</span>)}</div> : null}
							<div className="optimizer-checkpoint-actions">{hostedCispoAdmitted && hostedSftWarmStarts.some((candidate) => candidate.checkpointId === checkpoint.checkpointId) ? <button className="primary-button" type="button" onClick={() => { setTrainingTask("banking77"); setTrainingModel(checkpoint.baseModel); setTrainingWarmStartCheckpointId(checkpoint.checkpointId); revealSection("launch", "[data-testid='optimizer-training-launch']"); }} data-testid={`use-for-cispo-${checkpoint.checkpointId}`}>Use for hosted CISPO</button> : null}{checkpoint.lineage?.runId || checkpoint.runId ? <button className="secondary-button" type="button" onClick={() => void openCheckpointRun(checkpoint)}>Open run</button> : null}{checkpoint.inferenceChatCompletions ? <button className="secondary-button" type="button" disabled={inferringId !== null} onClick={() => void inferSavedLora(checkpoint, "chat_completions")}>{inferringId === `${checkpoint.checkpointId}:chat_completions` ? "Sampling…" : "Chat Completions"}</button> : null}{checkpoint.inferenceResponses ? <button className="secondary-button" type="button" disabled={inferringId !== null} onClick={() => void inferSavedLora(checkpoint, "responses")}>{inferringId === `${checkpoint.checkpointId}:responses` ? "Sampling…" : "Responses"}</button> : null}{isLagunaCompatibleAdapter(checkpoint) ? <button className="secondary-button" type="button" disabled={savedLoraBusy} onClick={() => void useInComposer(checkpoint)} data-testid={`use-in-composer-${checkpoint.checkpointId}`}>Use in Composer</button> : null}{checkpoint.placement === "this_mac" ? <button className="secondary-button" type="button" disabled={savedLoraBusy} onClick={() => void publishSavedLora(checkpoint)}>Publish</button> : null}<button className="secondary-button" type="button" disabled={savedLoraBusy} onClick={() => void downloadSavedLora(checkpoint)}>Download</button><button className="secondary-button optimizer-danger-button" type="button" disabled={savedLoraBusy} onClick={() => void archiveSavedLora(checkpoint)}>Archive</button></div>
						</article>
					))}
					{savedLoras.length === 0 && !savedLoraBusy ? <div className="optimizer-empty"><span className="optimizer-empty-icon" aria-hidden>◇</span><strong>No checkpoints match</strong><p>Local MLX adapters appear when a This Mac recipe emits them, or when you import an mlx-lora.v1 folder. Hosted SFT/CISPO LoRAs appear after object-storage verification.</p></div> : null}
				</div>
			</section>
			) : null}

			{tab === "launch" && (evalRecipes.length > 0 || selectedStarter) ? (
				<section className="optimizer-recipes optimizer-eval-catalog" aria-labelledby="optimizer-eval-title">
					<div className="optimizer-recipes-head">
						<div><span className="optimizer-eyebrow">Eval · local</span><h2 id="optimizer-eval-title">{selectedStarter ? selectedStarter.title : "Score staged policies"}</h2></div>
						<p>Fixed recipes run against pinned local container targets and do not install the Optimizers plugin.</p>
					</div>
					{selectedStarter ? (
						<div className="optimizer-starter-summary" data-testid="optimizer-starter-summary">
							<strong>{selectedStarter.description}</strong>
							<span>{selectedStarter.flow.join(" → ")} · maximum ${selectedStarter.maxCostUsd.toFixed(2)}</span>
							<details><summary>Starter prompt</summary><pre>{selectedStarter.prompt}</pre></details>
						</div>
					) : null}
					{selectedStarter && !evalRecipes.some((recipe) => recipe.id === selectedStarter.recipeId) ? (
						<div className="optimizer-empty" role="status" data-testid="optimizer-starter-unavailable">
							<strong>The selected starter is not available in this workspace yet.</strong>
							<p>Expected recipe <code>{selectedStarter.recipeId}</code>. Add its source project or choose another admitted recipe below.</p>
						</div>
					) : null}
					<div className="optimizer-recipe-grid">
						{orderedEvalRecipes.map((recipe) => {
							// Only fields a producer actually writes: `limits.trials`,
							// `budget.max_usd`, `models`, task/source/semantics, and the
							// admission booleans projected by eval_recipes.rs. The old
							// screening/confirmation/selection keys were never produced.
							const limits = (recipe.limits ?? {}) as Record<string, unknown>;
							const budget = (recipe.budget ?? {}) as Record<string, unknown>;
							const models = (recipe.models ?? [])
								.map((model) => (typeof model.id === "string" ? model.id : null))
								.filter((id): id is string => id != null);
							const available = recipe.availability === "available";
							const admissionError = recipe.admissionError && typeof recipe.admissionError === "object"
								? recipe.admissionError as Record<string, unknown>
								: null;
							const admissionReason = [admissionError?.message, admissionError?.error, admissionError?.detail]
								.find((candidate): candidate is string => typeof candidate === "string" && candidate.trim().length > 0)
								?? recipe.availabilityReason
								?? null;
							const admissionFlags = [
								{ id: "recipe-discovered", label: "Recipe", ok: recipe.recipeDiscovered },
								{ id: "execution-supported", label: "Execution", ok: recipe.executionSupported },
								{ id: "target-present", label: "Target", ok: recipe.targetPresent },
								{ id: "target-digest", label: "Digest", ok: recipe.targetDigestMatches },
								{ id: "target-admitted", label: "Admitted", ok: recipe.targetAdmitted }
							].filter((flag): flag is { id: string; label: string; ok: boolean } => typeof flag.ok === "boolean");
							return (
								<article className={`optimizer-recipe-card${recipe.id === selectedStarter?.recipeId ? " is-starter" : ""}`} aria-labelledby={`optimizer-eval-${recipe.id}`} data-testid={`optimizer-eval-recipe-${recipe.id}`} key={recipe.id}>
									<div className="optimizer-recipe-top">
										<span className="optimizer-recipe-mark">EV</span>
										<span className="optimizer-availability" data-available={available} data-testid={`optimizer-eval-availability-${recipe.id}`}>{recipe.availability}</span>
									</div>
									<h3 id={`optimizer-eval-${recipe.id}`}>{recipe.title}</h3>
									<code className="optimizer-eval-id">{recipe.id}</code>
									<dl className="optimizer-eval-limits">
										<dt>Trials</dt><dd>{limits.trials != null ? String(limits.trials) : "—"}</dd>
										<dt>Budget</dt><dd>{typeof budget.max_usd === "number" ? `$${budget.max_usd.toFixed(2)}` : "—"}</dd>
										<dt>Models</dt><dd>{models.join(", ") || "—"}</dd>
										<dt>Task</dt><dd>{recipe.task ?? "—"}</dd>
										<dt>Source</dt><dd>{[recipe.source, recipe.semantics].filter(Boolean).join(" · ") || "—"}</dd>
									</dl>
									{admissionFlags.length > 0 ? (
										<ul className="optimizer-admission" aria-label={`${recipe.title} admission checks`} data-testid={`optimizer-eval-admission-${recipe.id}`}>
											{admissionFlags.map((flag) => (
												<li
													key={flag.id}
													className="optimizer-admission-flag"
													data-ok={flag.ok}
													data-testid={`optimizer-eval-admission-${recipe.id}-${flag.id}`}
													title={flag.ok ? undefined : admissionReason ?? undefined}
												>
													<span aria-hidden>{flag.ok ? "✓" : "✕"}</span>
													{flag.label}
													<span className="sr-only">{flag.ok ? " passed" : admissionReason ? `: ${admissionReason}` : " failed"}</span>
												</li>
											))}
										</ul>
									) : null}
									{!available && admissionReason ? <small data-testid={`optimizer-eval-blocked-${recipe.id}`}>{admissionReason}</small> : null}
									<button
										className="secondary-button"
										type="button"
										disabled={!available || startingAgent !== null || busy}
										data-testid={`start-eval-${recipe.id}`}
										onClick={() => {
											if (isWorkspaceBaselineEval(recipe)) {
												void startBoundedRecipe(recipe.id, setBusy);
												return;
											}
											void startAgent({
												id: "eval",
												label: "EV",
												name: recipe.title,
												description: recipe.description ?? "",
												flow: ["Stage", "Score", "Select"],
										prompt: starterPromptForRecipe(
											selectedStarter,
											recipe.id,
											`Run the Workshop eval recipe ${recipe.id} on policy variants in this project. Stage the policy files with optimizer_stage_eval_candidates using workspace-relative paths, kind python-code.v1, entrypoint policy:Policy, one labelled candidate each, marking the baseline; then call optimizer_start_recipe with the recipe id and returned candidate_set_id. Never replace a policy on your own. Report the run status and selection status separately, the per-candidate scorecard, and the evidence directory.`
										)
											});
										}}
									>
										{isWorkspaceBaselineEval(recipe)
											? (busy ? "Starting…" : "Start eval")
											: (startingAgent === "eval" ? "Opening agent…" : "Set up run")}
									</button>
								</article>
							);
						})}
					</div>
				</section>
			) : null}

			{tab === "runs" ? (<>
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
					<select aria-label="Recipe filter" value={recipeFilter} onChange={(e) => setRecipeFilter(e.target.value)} data-testid="optimizer-filter-recipe">
						<option value="all">All recipes</option>
						{facetOptions.recipes.map((id) => <option key={id} value={id}>{id}</option>)}
					</select>
					<select aria-label="Container filter" value={containerFilter} onChange={(e) => setContainerFilter(e.target.value)} data-testid="optimizer-filter-container">
						<option value="all">All containers</option>
						{facetOptions.containers.map((id) => <option key={id} value={id}>{id}</option>)}
					</select>
					<select aria-label="Model filter" value={modelFilter} onChange={(e) => setModelFilter(e.target.value)} data-testid="optimizer-filter-model">
						<option value="all">All models</option>
						{facetOptions.models.map((id) => <option key={id} value={id}>{id}</option>)}
					</select>
					<label className="optimizer-date-filter"><span>From</span><input type="date" aria-label="Runs from date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} data-testid="optimizer-filter-date-from" /></label>
					<label className="optimizer-date-filter"><span>To</span><input type="date" aria-label="Runs to date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} data-testid="optimizer-filter-date-to" /></label>
					{clientFiltersActive ? (
						<button className="secondary-button" type="button" onClick={clearClientFilters} data-testid="optimizer-clear-filters">Clear</button>
					) : null}
				</div>
			</div>

			<div className="optimizer-workbench">
				<section className="optimizer-runs" aria-label="Optimizer runs">
					<div className="optimizer-section-head"><div><span className="optimizer-eyebrow">Runs</span><strong data-testid="optimizer-run-count">{clientFiltersActive ? `${visibleRuns.length} of ${runs.length}` : `${runs.length} total`}</strong></div></div>
					<ul className="inventory-list optimizer-list">
						{visibleRuns.map((run) => {
							// The mini-fraction comes from the sealed terminal manifest the
							// list payload already carries. Live runs report their counts
							// through the event log, not the list record, so they show the
							// usage rollout floor when one exists and nothing otherwise —
							// never a fabricated zero.
							const counts = sealedWorkCounts(run);
							const fraction = counts
								? workFractionLabel(counts)
								: run.usage?.rollouts
									? `${run.usage.rollouts} rollouts`
									: null;
							return (
								<li key={run.id}>
									<button
										type="button"
										className={`inventory-row${selectedId === run.id ? " active" : ""}`}
										data-testid={`optimizer-run-${run.id}`}
										onClick={() => setSelectedId(run.id)}
									>
										<span className="optimizer-run-main">
											<span className="optimizer-algorithm">{algorithmLabel(run.algorithmId)}</span>
											<strong>{runTitle(run)}</strong>
											<small>
												<code className="optimizer-run-id-inline" title={run.id}>{truncateMiddle(run.id)}</code>
												{" · "}
												{run.finishedAt ? `finished ${formatWhen(run.finishedAt)}` : formatWhen(run.startedAt ?? run.createdAt)}
											</small>
										</span>
										<span className="optimizer-run-meta">
											<span className={statusChipClass(run.status)}>{statusText(run.status)}</span>
											<small data-testid={`optimizer-run-facts-${run.id}`}>
												{fraction ? `${fraction} · ` : ""}
												{run.source} · {run.usage?.costUsd == null ? "—" : `$${run.usage.costUsd.toFixed(2)}`}
											</small>
										</span>
									</button>
								</li>
							);
						})}
						{runs.length === 0 ? (
							<li className="optimizer-empty" data-testid="optimizer-runs-empty">
								<span className="optimizer-empty-icon" aria-hidden>↗</span>
								<strong>No optimizer runs yet</strong>
								<p>Plan one on the Launch tab, import an existing run, or sync cloud history.</p>
								<button className="secondary-button" type="button" onClick={() => setTab("launch")} data-testid="optimizer-runs-empty-launch">Open Launch</button>
							</li>
						) : visibleRuns.length === 0 ? (
							<li className="optimizer-empty" data-testid="optimizer-runs-filtered-empty">
								<span className="optimizer-empty-icon" aria-hidden>⌕</span>
								<strong>No runs match these filters</strong>
								<p>
									{runs.length} loaded run{runs.length === 1 ? "" : "s"} were filtered out.
									Recipe, container, and model are read from each run record; a run whose
									producer never recorded that fact cannot match its filter.
								</p>
								<button className="secondary-button" type="button" onClick={clearClientFilters} data-testid="optimizer-runs-clear-filters">Clear filters</button>
							</li>
						) : null}
					</ul>
				</section>

				<section id="optimizer-run-inspector" className="optimizer-inspector" aria-label="Optimizer inspector" tabIndex={-1}>
					{selected ? (
						<RunInspector run={selected} executionLabel={selectedExecution}>
							{trainingProjection ? (
								<section className="optimizer-training-progress" data-testid="optimizer-training-progress">
									<div className="optimizer-training-title">
										<span className="optimizer-eyebrow">Hosted training</span>
										<span className={statusChipClass(trainingProjection.lifecycle)}>{statusText(trainingProjection.lifecycle)}</span>
									</div>
									<dl>
										<dt>Phase</dt><dd>{trainingProjection.phase ?? "—"}</dd>
										<dt>Training cursor</dt><dd>{trainingProjection.last_sequence}</dd>
										<dt>Attempt</dt><dd>{trainingProjection.attempt_id ?? "—"}</dd>
										<dt>Tunnel</dt><dd data-testid="optimizer-training-tunnel">{trainingProjection.tunnel_health?.status ?? "No disruption reported"}</dd>
										<dt>Estimated cost</dt><dd>{typeof selectedTrainingUsage?.estimated_cost_usd === "number" ? `$${selectedTrainingUsage.estimated_cost_usd.toFixed(4)}` : "—"}</dd>
										<dt>Reconciliation</dt><dd>{String(objectValue(selectedTrainingUsage?.coverage).provider_cost_usd ?? "pending")}</dd>
									</dl>
									{Object.keys(trainingProjection.metrics).length > 0 ? (
										<div className="optimizer-training-metrics" data-testid="optimizer-training-metrics">
											{Object.entries(trainingProjection.metrics).map(([name, value]) => <span key={name}><small>{name}</small><strong>{value.toFixed(4)}</strong></span>)}
										</div>
									) : null}
									{selectedTrainingEvaluations.length > 0 ? <TrainingEvaluationCurve evaluations={selectedTrainingEvaluations} testId="optimizer-training-evaluations" /> : null}
									{selectedTrainingCheckpoints.length > 0 ? (
										<div className="optimizer-training-checkpoints" data-testid="optimizer-training-checkpoints">
											<strong>Ready checkpoints</strong>
											{selectedTrainingCheckpoints.map((checkpoint, index) => (
												<article key={String(checkpoint.checkpoint_id ?? checkpoint.artifact_id ?? index)}>
													<code>{String(checkpoint.checkpoint_id ?? checkpoint.artifact_id ?? `checkpoint ${index + 1}`)}</code>
													<span>step {String(checkpoint.step ?? "—")} · {String(checkpoint.state ?? "unknown")}</span>
													<small>{checkpoint.evaluation_eligible === true ? "Evaluation ready" : "Evaluation unavailable"} · {checkpoint.resume_eligible === true ? "Resume ready" : "Resume unavailable"}</small>
													{checkpoint.state === "ready" ? (
														<div className="optimizer-checkpoint-actions">
															{checkpoint.evaluation_eligible === true ? <button type="button" className="secondary-button" disabled={startingAgent !== null} onClick={() => void startCheckpointWorkflow("evaluate", checkpoint)}>Evaluate</button> : null}
															{checkpoint.resume_eligible === true && ["paused", "infrastructure_lost", "cap_reached", "failed", "degraded", "completed"].includes(trainingProjection.lifecycle) ? <button type="button" className="secondary-button" disabled={startingAgent !== null} onClick={() => void startCheckpointWorkflow("resume", checkpoint)}>Resume</button> : null}
															{checkpoint.evaluation_eligible === true ? <button type="button" className="secondary-button" disabled={startingAgent !== null} onClick={() => void startCheckpointWorkflow("compare_report", checkpoint)}>Compare &amp; report</button> : null}
														</div>
													) : null}
												</article>
											))}
										</div>
									) : null}
								</section>
							) : null}
							{selected.source === "cloud" ? (
								<section className="optimizer-training-progress" data-testid="optimizer-run-outputs">
									<div className="optimizer-training-title"><span className="optimizer-eyebrow">Automatic outputs</span><strong>{selectedRunOutputs?.counts.artifacts ?? 0} artifacts · {selectedCheckpointCounts.total} model checkpoints</strong></div>
									{selectedRunOutputs?.result ? <details open className="optimizer-run-files"><summary>Final result</summary><dl>{Object.entries(selectedRunOutputs.result).slice(0, 8).map(([name, value]) => <Fragment key={name}><dt>{name.replaceAll("_", " ")}</dt><dd>{typeof value === "object" ? JSON.stringify(value) : String(value)}</dd></Fragment>)}</dl></details> : <p>The final result will appear here when the run seals it.</p>}
									{selectedRunOutputs?.artifacts.map((artifact) => <article key={artifact.artifactId} className="optimizer-run-output"><strong>{artifact.artifactName}</strong><small>{artifact.contentType ?? "artifact"} · {formatBytes(artifact.sizeBytes)} · {artifact.storageBackend}</small><code>{artifact.sha256 ?? artifact.uri}</code></article>)}
									{["sft", "cispo", "ppo"].includes(selected.algorithmId) ? <p>{selectedCheckpointCounts.inference} inference LoRA · {selectedCheckpointCounts.training} resumable training state</p> : null}
									{selectedRunCheckpoints.map((checkpoint) => <article key={checkpoint.checkpointId} className="optimizer-run-output"><strong>{checkpoint.name}</strong><small>{checkpoint.checkpointKind} · step {checkpoint.step ?? "—"} · {checkpoint.storage.backend}</small><code>{checkpoint.lineage?.sourceCheckpointId ?? checkpoint.sourceCheckpointId ?? checkpoint.checkpointId}</code><div className="optimizer-checkpoint-actions"><button type="button" className="secondary-button" onClick={() => showCheckpointInCatalog(checkpoint)}>View in catalog</button><button type="button" className="secondary-button" disabled={savedLoraBusy} onClick={() => void downloadSavedLora(checkpoint)}>Download</button></div></article>)}
									{selectedRunOutputs && selectedRunOutputs.counts.artifacts === 0 && selectedRunCheckpoints.length === 0 ? <p>No persisted outputs have been published yet. Results and checkpoints appear automatically as the run reaches publication boundaries.</p> : null}
								</section>
							) : null}
							{evalState ? (
								<section className="optimizer-eval-scorecard" data-testid="optimizer-eval-scorecard">
									<span className="optimizer-eyebrow">Canonical aggregate</span>
									<table>
										<thead>
											<tr><th>Run</th><th>Revision</th><th>Scored</th><th>Failed</th><th>Mean reward</th><th>Evidence</th></tr>
										</thead>
										<tbody>
											<tr data-testid="eval-scorecard-row-aggregate">
												<td>{evalState.aggregate.runId}</td>
												<td>{evalState.aggregate.projectionRevision}</td>
												<td>{evalState.aggregate.scoredTrials}</td>
												<td>{evalState.aggregate.work.failed ?? "—"}</td>
												<td>{formatMetric(evalState.aggregate.meanReward)}</td>
												<td>{evalState.aggregate.evidence.completeness}</td>
											</tr>
										</tbody>
									</table>
									<dl className="optimizer-eval-selection" data-testid="optimizer-eval-selection">
										<dt>Selection</dt><dd>{evalState.aggregate.selection}</dd>
										<dt>Sequence</dt><dd>{evalState.aggregate.asOfSequence}</dd>
										<dt>Why</dt><dd>{evalSelectionReason(evalState.aggregate.selection)}</dd>
									</dl>
									<code className="optimizer-eval-evidence" data-testid="optimizer-eval-evidence">
										{evalState.aggregate.evidence.reason ?? `${evalState.aggregate.evidenceRefCount} immutable references`}
									</code>
								</section>
							) : null}
							<div className="optimizer-inspector-actions">
								<button className="primary-button" type="button" disabled={busy} onClick={() => void openSelectedVisual()} data-testid="open-optimizer-visual">Open visual</button>
								<button className="secondary-button" type="button" disabled={busy} onClick={() => void refreshSelected()} data-testid="refresh-optimizer-run">Refresh</button>
								{selected.capabilities?.pause && selected.status === "running" ? <button className="secondary-button" type="button" disabled={busy} onClick={() => void controlSelected("pause")} data-testid="pause-optimizer-run">Pause</button> : null}
								{selected.capabilities?.resume && selected.status === "paused" ? <button className="secondary-button" type="button" disabled={busy} onClick={() => void controlSelected("resume")} data-testid="resume-optimizer-run">Resume</button> : null}
								{selected.capabilities?.cancel && !isTerminalRunStatus(selected.status) ? <button className="secondary-button optimizer-danger-button" type="button" disabled={busy} onClick={() => void controlSelected("cancel")} data-testid="cancel-optimizer-run">Cancel</button> : null}
							</div>
						</RunInspector>
					) : (
						<div className="optimizer-empty optimizer-empty-inspector"><span className="optimizer-empty-icon" aria-hidden>◎</span><strong>Select a run</strong><p>Run details, usage, and linked visuals appear here.</p></div>
					)}
				</section>
			</div>
			</>) : null}
		</div>
	);
}
