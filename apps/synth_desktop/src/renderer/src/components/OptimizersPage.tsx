import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import type { OptimizerAlgorithmInfo, OptimizerRunRecord } from "@synth/runtime-protocol";
import type { HostedTrainingModel, OptimizerRecipeInfo, OptimizerRunOutputs, PluginActionReceipt, PluginLifecycleOperation, PluginStatus, SavedLoraCheckpoint, TrainingArtifact, TrainingProjection } from "../bridge/types";
import { bridges } from "../runtime/desktopBridge";
import { findPluginStatus, pluginPresentation, type PluginPresentation } from "../runtime/pluginPresentation";
import { publicError } from "../runtime/publicError";

type OptimizerGuide = {
	id: "gepa" | "go-ex" | "sft" | "cispo" | "eval";
	label: string;
	name: string;
	description: string;
	flow: string[];
	prompt: string;
	kind: "optimizer" | "training";
};

const OPTIMIZER_GUIDES: OptimizerGuide[] = [
	{
		id: "gepa",
		label: "GE",
		name: "GEPA",
		kind: "optimizer",
		description: "Improve prompts by proposing candidates, evaluating them, and maintaining a quality frontier.",
		flow: ["Propose", "Evaluate", "Select"],
		prompt: "Help me set up a GEPA optimization in Workshop. Do not start compute yet. First ask what I want to optimize, then help me choose or create the evaluation Container, dataset splits, scoring contract, proposer model, budget, and stopping criteria. Verify the target and event-stream contracts before proposing a run. Explain tradeoffs and wait for my explicit approval before starting paid compute."
	},
	{
		id: "go-ex",
		label: "GX",
		name: "GELO",
		kind: "optimizer",
		description: "Explore prompt-policy variants from rollout evidence and branch from useful intermediate states.",
		flow: ["Explore", "Branch", "Verify"],
		prompt: "Help me set up a prompt-only GELO (GoEx) optimization in Workshop. Do not start compute yet. First ask what behavior I want to improve and which Container or evaluation target should measure it. Discover the target's actual capabilities, including streaming, rewards, prompt treatment, checkpoints, and restore support; fail the plan early if required affordances are missing. Then help me choose seeds, proposer policy, budget, heldout evaluation, and stopping criteria. Wait for my explicit approval before starting paid compute."
	},
	{
		id: "sft",
		label: "SF",
		name: "SFT",
		kind: "training",
		description: "Collect strong demonstrations, train checkpoints, and compare the adapted model against its baseline. This Mac (MLX) or hosted.",
		flow: ["Collect", "Train", "Compare"],
		prompt: "Help me set up an SFT optimization in Workshop. Do not start compute yet. Ask whether I want This Mac (recipe sft.qwen35-0.8b.mlx.v1) or hosted (sft.hosted.fixture.v1 / Tinker recipes). Never dial :8787 or name synth-mlx-rl. Wait for my explicit approval before starting paid compute."
	},
	{
		id: "cispo",
		label: "CI",
		name: "CISPO · slime reference",
		kind: "training",
		description: "Run on-policy training with the pinned slime CISPO objective. This Mac (MLX) or hosted after the clip canary.",
		flow: ["Preflight", "Roll out", "Train"],
		prompt: "Help me set up CISPO in Workshop. Do not start paid compute yet. Prefer recipe cispo.banking77.mlx.v1 on this Mac, or the bounded hosted recipe cispo.slime.hosted.v1 if hosted is admitted. Never draft a free-form HostedOptimizerClient.launch_training call. Wait for my explicit approval before launch."
	}
];

const SEARCH_GUIDES = OPTIMIZER_GUIDES.filter((guide) => guide.kind === "optimizer");
const TRAINING_GUIDES = OPTIMIZER_GUIDES.filter((guide) => guide.kind === "training");

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

function formatBytes(value: number | null | undefined): string {
	if (value == null) return "—";
	if (value < 1024) return `${value} B`;
	if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`;
	if (value < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MB`;
	return `${(value / 1024 ** 3).toFixed(1)} GB`;
}

function algorithmLabel(id: string): string {
	if (id === "gepa") return "GEPA";
	if (id === "go-ex") return "GELO";
	if (id === "sft") return "SFT";
	if (id === "cispo") return "CISPO";
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
	pairedTrials?: number;
	eliminationReason?: string | null;
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
	// A metric no valid trial produced is unknown, not zero.
	return value == null ? "—" : value.toFixed(3);
}

function formatLift(value: number | null | undefined): string {
	if (value == null) return "—";
	return `${value > 0 ? "+" : ""}${value.toFixed(3)}`;
}

function objectValue(value: unknown): Record<string, unknown> {
	return value && typeof value === "object" ? value as Record<string, unknown> : {};
}

function checkpointValue(value: unknown): Record<string, unknown> {
	const payload = objectValue(value);
	return objectValue(payload.checkpoint ?? payload);
}

type OptimizerDiagnostic = {
	title: string;
	message: string;
	field?: string;
	raw?: string;
	logPath?: string;
};

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

function optimizerDiagnostic(error: unknown): OptimizerDiagnostic | null {
	if (!error) return null;
	const value = typeof error === "object" ? error as Record<string, unknown> : {};
	const message = typeof error === "string"
		? error
		: typeof value.message === "string" ? value.message : publicError(error);
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
	const [errorDetails, setErrorDetails] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const [startingAgent, setStartingAgent] = useState<OptimizerGuide["id"] | null>(null);
	const [startingSftFixture, setStartingSftFixture] = useState(false);
	const [startingLocalSft, setStartingLocalSft] = useState(false);
	const [startingLocalCispo, setStartingLocalCispo] = useState(false);
	const [evalRecipes, setEvalRecipes] = useState<OptimizerRecipeInfo[]>([]);
	const [evalState, setEvalState] = useState<EvalState | null>(null);
	const [trainingProjection, setTrainingProjection] = useState<TrainingProjection | null>(null);
	const [trainingAlgorithm, setTrainingAlgorithm] = useState<"cispo">("cispo");
	const [trainingModel, setTrainingModel] = useState("openai/gpt-oss-20b");
	const [trainingTask, setTrainingTask] = useState("banking77");
	const [trainingContainerUrl, setTrainingContainerUrl] = useState("http://127.0.0.1:8000");
	const [trainingSteps, setTrainingSteps] = useState(2);
	const [trainingWallSeconds, setTrainingWallSeconds] = useState(300);
	const [trainingCostUsd, setTrainingCostUsd] = useState(0.1);
	const [trainingCheckpointEvery, setTrainingCheckpointEvery] = useState(1);
	const [hostedTrainingModels, setHostedTrainingModels] = useState<HostedTrainingModel[]>([]);
	const [hostedModelCatalogRevision, setHostedModelCatalogRevision] = useState<string | null>(null);
	const [savedLoras, setSavedLoras] = useState<SavedLoraCheckpoint[]>([]);
	const [localArtifacts, setLocalArtifacts] = useState<TrainingArtifact[]>([]);
	const [localArtifactReply, setLocalArtifactReply] = useState<string | null>(null);
	const [savedLoraTotal, setSavedLoraTotal] = useState(0);
	const [savedLoraSearch, setSavedLoraSearch] = useState("");
	const [savedLoraScope, setSavedLoraScope] = useState<"all" | "mine" | "org">("all");
	const [savedLoraProvider, setSavedLoraProvider] = useState("all");
	const [savedLoraAlgorithm, setSavedLoraAlgorithm] = useState("all");
	const [savedLoraKind, setSavedLoraKind] = useState("all");
	const [savedLoraBusy, setSavedLoraBusy] = useState(false);
	const [selectedRunCheckpoints, setSelectedRunCheckpoints] = useState<SavedLoraCheckpoint[]>([]);
	const [selectedCheckpointCounts, setSelectedCheckpointCounts] = useState({ total: 0, inference: 0, training: 0 });
	const [selectedRunOutputs, setSelectedRunOutputs] = useState<OptimizerRunOutputs | null>(null);
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
		if (!selectedId && nextRuns[0]) setSelectedId(nextRuns[0].id);
		const artifacts = await bridges.trainingArtifacts?.list().catch(() => [] as TrainingArtifact[]);
		setLocalArtifacts(artifacts ?? []);
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
				const preferred = catalog.models.find((model) => model.algorithms[trainingAlgorithm]?.status !== "blocked");
				if (preferred) setTrainingModel(preferred.modelId);
			}
		}).catch((reason) => {
			if (live) setError(presentError(reason).message);
		});
		return () => { live = false; };
	}, [trainingAlgorithm]);

	const refreshSavedLoras = useCallback(async () => {
		if (!bridges.optimizers) return;
		setSavedLoraBusy(true);
		try {
			const page = await bridges.optimizers.searchSavedLoras({
				search: savedLoraSearch.trim() || undefined,
				scope: savedLoraScope,
				provider: savedLoraProvider === "all" ? undefined : savedLoraProvider,
				optimizerAlgorithm: savedLoraAlgorithm === "all" ? undefined : savedLoraAlgorithm as "sft" | "cispo" | "ppo",
				checkpointKind: savedLoraKind === "all" ? undefined : savedLoraKind,
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
	}, [savedLoraAlgorithm, savedLoraKind, savedLoraProvider, savedLoraScope, savedLoraSearch]);

	useEffect(() => {
		const timer = window.setTimeout(() => void refreshSavedLoras(), 250);
		return () => window.clearTimeout(timer);
	}, [refreshSavedLoras]);

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
		if (!window.confirm(`Archive “${checkpoint.name}”? The Wasabi object is retained.`)) return;
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

	const openCheckpointRun = async (checkpoint: SavedLoraCheckpoint) => {
		if (!bridges.optimizers) return;
		const runId = checkpoint.lineage.runId ?? checkpoint.runId;
		if (!runId) return;
		try {
			if (!runs.some((run) => run.id === runId)) {
				const run = await bridges.optimizers.get(runId);
				setRuns((current) => [run, ...current.filter((item) => item.id !== run.id)]);
			}
			setSelectedId(runId);
			window.setTimeout(() => document.getElementById("optimizer-run-inspector")?.scrollIntoView({ behavior: "smooth", block: "start" }), 0);
		} catch (reason) {
			setError(presentError(reason).message);
		}
	};

	const showCheckpointInCatalog = (checkpoint: SavedLoraCheckpoint) => {
		setSavedLoraSearch(checkpoint.name);
		window.setTimeout(() => document.getElementById("optimizer-checkpoint-library")?.scrollIntoView({ behavior: "smooth", block: "start" }), 0);
	};

	const selected = useMemo(
		() => runs.find((run) => run.id === selectedId) ?? null,
		[runs, selectedId]
	);

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
			if (live && !["completed", "cancelled", "failed", "infrastructure_lost", "cap_reached"].includes(selected.status)) {
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
		const guide = TRAINING_GUIDES.find((item) => item.id === "cispo");
		if (!guide) return;
		await startAgent({
			...guide,
			prompt: `${guide.prompt}\n\nLaunch the bounded hosted recipe cispo.slime.hosted.v1. Do not call HostedOptimizerClient.launch_training.\n\nTyped launch draft:\n- recipe: cispo.slime.hosted.v1\n- model: ${trainingModel}\n- task: ${trainingTask}\n- local container URL: ${trainingContainerUrl}\n- hard step cap: ${trainingSteps}\n- hard wall-clock cap: ${trainingWallSeconds} seconds\n- hard cost cap: $${trainingCostUsd}\n- checkpoint every: ${trainingCheckpointEvery} step(s)\n\nUse recipe admission so the sidecar performs provider preflight, container capability validation, SynthTunnel setup, and lease ownership. Echo the effective config and both capability hashes. If the recipe is not admitted, stop before spend.`
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
			prompt: `${actionPrompt}\n\nRun: ${selected.id}\nAlgorithm: ${selected.algorithmId}\nCheckpoint: ${checkpointId}\nTask: ${String(selected.summary?.taskId ?? "from the sealed run config")}\nDo not substitute another checkpoint. Verify ready/evaluation/resume eligibility from canonical backend evidence before acting.`
		});
	};

	const runLocalArtifactInference = async (artifact: TrainingArtifact) => {
		if (!bridges.trainingArtifacts) return;
		setBusy(true);
		setError(null);
		setLocalArtifactReply(null);
		try {
			const result = await bridges.trainingArtifacts.launchInference({
				id: artifact.id,
				confirm: true
			});
			setLocalArtifactReply(`${result.policySnapshotId}: ${result.reply}`);
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			setBusy(false);
		}
	};

	const evaluateLocalArtifact = async (artifact: TrainingArtifact) => {
		if (!bridges.optimizers) return;
		setBusy(true);
		setError(null);
		try {
			const run = await bridges.optimizers.startRecipe({
				recipeId: "eval.mlx.local-policy.smoke.v1",
				trainingArtifactId: artifact.id,
				openVisual: true
			});
			setSelectedId(run.id);
			await refresh();
			const visualId = run.visualRefs.find((ref) => ref.kind === "visual")?.id;
			if (visualId) onOpenVisual(visualId);
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			setBusy(false);
		}
	};

	const startBoundedRecipe = async (recipeId: string, setter: (busy: boolean) => void) => {
		if (!bridges.optimizers) return;
		setter(true);
		setError(null);
		try {
			const run = await bridges.optimizers.startRecipe({
				recipeId,
				openVisual: true
			});
			setSelectedId(run.id);
			await refresh();
			const visualId = run.visualRefs.find((ref) => ref.kind === "visual")?.id;
			if (visualId) onOpenVisual(visualId);
		} catch (reason) {
			setError(presentError(reason).message);
		} finally {
			setter(false);
		}
	};

	const startSftFixture = async () => {
		await startBoundedRecipe("sft.hosted.fixture.v1", setStartingSftFixture);
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
			const visualId = run.visualRefs.find((ref) => ref.kind === "visual")?.id;
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
	const selectedTrainingUsage = trainingProjection?.provider_usage ?? null;
	const selectedTrainingCheckpoints = trainingProjection?.checkpoints.map(checkpointValue) ?? [];
	const selectedHostedModel = hostedTrainingModels.find((model) => model.modelId === trainingModel);
	const selectedHostedSupport = selectedHostedModel?.algorithms[trainingAlgorithm];
	const hostedLaunchBlocked = !selectedHostedSupport || selectedHostedSupport.status === "blocked";

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
					{SEARCH_GUIDES.map((guide) => (
						<article className="optimizer-recipe-card" aria-labelledby={`optimizer-guide-${guide.id}`} data-testid={`optimizer-guide-${guide.id}`} key={guide.id}>
							<div className="optimizer-recipe-top"><span className="optimizer-recipe-mark">{guide.label}</span><span className="optimizer-recipe-runtime">Optimization algorithm</span></div>
							<h3 id={`optimizer-guide-${guide.id}`}>{guide.name}</h3>
							<p>{guide.description}</p>
							<div className="optimizer-recipe-flow" aria-label={`${guide.name} workflow`}>{guide.flow.map((step) => <span key={step}>{step}</span>)}</div>
							<button
								className="secondary-button"
								type="button"
								disabled={startingAgent !== null || (plugin != null && !presentation.isUsable)}
								title={plugin != null && !presentation.isUsable && presentation.label
									? `Optimizers: ${presentation.label}`
									: undefined}
								onClick={() => void startAgent(guide)}
								data-testid={`start-${guide.id}-agent`}
							>
								{startingAgent === guide.id ? "Opening agent…" : "Plan with agent"}
							</button>
						</article>
					))}
				</div>
			</section>

			<section className="optimizer-recipes" aria-labelledby="optimizer-training-title" data-testid="optimizer-training-guides">
				<div className="optimizer-recipes-head">
					<div><span className="optimizer-eyebrow">Training</span><h2 id="optimizer-training-title">Train a model</h2></div>
					<p>SFT and CISPO are training lanes, not search algorithms. They live here, not in the optimizer card grid.</p>
				</div>
				<div className="optimizer-recipe-grid">
					{TRAINING_GUIDES.map((guide) => (
						<article className="optimizer-recipe-card" aria-labelledby={`optimizer-guide-${guide.id}`} data-testid={`optimizer-guide-${guide.id}`} key={guide.id}>
							<div className="optimizer-recipe-top"><span className="optimizer-recipe-mark">{guide.label}</span><span className="optimizer-recipe-runtime">Training lane</span></div>
							<h3 id={`optimizer-guide-${guide.id}`}>{guide.name}</h3>
							<p>{guide.description}</p>
							<div className="optimizer-recipe-flow" aria-label={`${guide.name} workflow`}>{guide.flow.map((step) => <span key={step}>{step}</span>)}</div>
							<button
								className="secondary-button"
								type="button"
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
									<button className="secondary-button" type="button" disabled={startingLocalSft || (plugin != null && !presentation.isUsable)} onClick={() => void startBoundedRecipe("sft.qwen35-0.8b.mlx.v1", setStartingLocalSft)} data-testid="start-sft-mlx">
										{startingLocalSft ? "Starting…" : "This Mac · Qwen 0.8B MLX"}
									</button>
									<button className="secondary-button" type="button" disabled={startingSftFixture} onClick={() => void startSftFixture()} data-testid="start-sft-fixture">
										{startingSftFixture ? "Starting fixture…" : "Run hosted fixture"}
									</button>
									<small>Sidecar admits local MLX or hosted public SFT. Never dial :8787.</small>
								</>
							) : null}
							{guide.id === "cispo" ? (
								<>
									<button className="secondary-button" type="button" disabled={startingLocalCispo || (plugin != null && !presentation.isUsable)} onClick={() => void startBoundedRecipe("cispo.banking77.mlx.v1", setStartingLocalCispo)} data-testid="start-cispo-mlx">
										{startingLocalCispo ? "Starting…" : "This Mac · Banking77 CISPO"}
									</button>
									<small>Hosted CISPO stays fail-closed until the slime clip canary admits it. Use the bounded recipe below.</small>
								</>
							) : null}
						</article>
					))}
				</div>
			</section>

			<section className="optimizer-training-launch" aria-labelledby="optimizer-training-launch-title" data-testid="optimizer-training-launch">
				<div className="optimizer-recipes-head">
					<div><span className="optimizer-eyebrow">Hosted on-policy training</span><h2 id="optimizer-training-launch-title">Configure a bounded launch</h2></div>
					<p>The client validates provider and Container capabilities before opening paid compute.</p>
				</div>
				<div className="optimizer-training-form">
					<label><span>Algorithm</span><select value={trainingAlgorithm} onChange={(event) => {
						setTrainingAlgorithm(event.target.value as "cispo");
						setTrainingTask("banking77");
					}}><option value="cispo">CISPO · slime reference</option></select></label>
					<label><span>Model</span><select value={trainingModel} onChange={(event) => setTrainingModel(event.target.value)}>{hostedTrainingModels.map((model) => { const support = model.algorithms[trainingAlgorithm]; return <option key={model.modelId} value={model.modelId} disabled={support?.status === "blocked"}>{model.label} · {support?.status ?? "not validated"}</option>; })}</select></label>
					<label><span>Task</span><input value={trainingTask} onChange={(event) => setTrainingTask(event.target.value)} /></label>
					<label><span>Local Container URL</span><input value={trainingContainerUrl} onChange={(event) => setTrainingContainerUrl(event.target.value)} /></label>
					<label><span>Steps</span><input type="number" min={1} value={trainingSteps} onChange={(event) => setTrainingSteps(Math.max(1, Number(event.target.value)))} /></label>
					<label><span>Wall clock (seconds)</span><input type="number" min={1} value={trainingWallSeconds} onChange={(event) => setTrainingWallSeconds(Math.max(1, Number(event.target.value)))} /></label>
					<label><span>Cost cap (USD)</span><input type="number" min={0.01} step={0.01} value={trainingCostUsd} onChange={(event) => setTrainingCostUsd(Math.max(0.01, Number(event.target.value)))} /></label>
					<label><span>Checkpoint every</span><input type="number" min={1} value={trainingCheckpointEvery} onChange={(event) => setTrainingCheckpointEvery(Math.max(1, Number(event.target.value)))} /></label>
				</div>
				<div className="optimizer-training-launch-actions">
					<button className="primary-button" type="button" disabled={startingAgent !== null || hostedLaunchBlocked || !trainingModel.trim() || !trainingTask.trim() || !trainingContainerUrl.trim()} onClick={() => void reviewTrainingLaunch()} data-testid="review-hosted-training-launch">Review &amp; launch</button>
					<small>{hostedLaunchBlocked ? selectedHostedSupport?.block_reason ?? "This model and algorithm combination is not admitted by the hosted catalog." : "Bounded recipe: cispo.slime.hosted.v1. Default golden path: CISPO → Banking77."}{hostedModelCatalogRevision ? ` Catalog ${hostedModelCatalogRevision}; live provider preflight still required.` : ""}</small>
				</div>
			</section>

			<section id="optimizer-local-artifact-library" className="optimizer-checkpoint-library" aria-labelledby="optimizer-local-artifact-library-title" data-testid="optimizer-local-artifact-library">
				<div className="optimizer-recipes-head">
					<div><span className="optimizer-eyebrow">This Mac</span><h2 id="optimizer-local-artifact-library-title">Training artifacts</h2></div>
					<p>{localArtifacts.length} retained adapters. Inference and Eval must name one of these ids; they never fall back to ambient latest.</p>
				</div>
				{localArtifactReply ? <p data-testid="local-artifact-reply">{localArtifactReply}</p> : null}
				{localArtifacts.length === 0 ? (
					<p data-testid="local-artifact-empty">No local adapters yet. Finish a bounded SFT or CISPO run on this Mac.</p>
				) : (
					<ul className="optimizer-checkpoint-list">
						{localArtifacts.map((artifact) => (
							<li key={artifact.id} data-testid={`local-artifact-${artifact.id}`}>
								<div>
									<strong>{artifact.id}</strong>
									<p>{artifact.adapterKind} · {artifact.baseModelId} · run {artifact.producingRunId} · {artifact.producingAlgorithm}</p>
									<p>digest {artifact.digest ?? "none"} · dataset {artifact.datasetDigest ?? "none"} · config {artifact.configDigest ?? "none"} · {artifact.integrity}{artifact.sizeBytes != null ? ` · ${formatBytes(artifact.sizeBytes)}` : ""}</p>
								</div>
								<div>
									<button className="secondary-button" type="button" disabled={!artifact.integrity || artifact.integrity === "unavailable"} onClick={() => void runLocalArtifactInference(artifact)} data-testid={`local-artifact-infer-${artifact.id}`}>Run inference</button>
									<button className="secondary-button" type="button" onClick={() => void evaluateLocalArtifact(artifact)} data-testid={`local-artifact-eval-${artifact.id}`}>Evaluate</button>
								</div>
							</li>
						))}
					</ul>
				)}
			</section>

			<section id="optimizer-checkpoint-library" className="optimizer-checkpoint-library" aria-labelledby="optimizer-checkpoint-library-title" data-testid="optimizer-checkpoint-library">
				<div className="optimizer-recipes-head">
					<div><span className="optimizer-eyebrow">Automatic outputs · Wasabi hosted / MinIO local</span><h2 id="optimizer-checkpoint-library-title">Checkpoint catalog</h2></div>
					<p>{savedLoraTotal} visible across your private and organization libraries.</p>
				</div>
				<div className="optimizer-checkpoint-filters">
					<label className="optimizer-search"><span aria-hidden>⌕</span><input aria-label="Search saved LoRA checkpoints" placeholder="Search names, models, references, or tags" value={savedLoraSearch} onChange={(event) => setSavedLoraSearch(event.target.value)} data-testid="saved-lora-search" /></label>
					<select aria-label="Checkpoint ownership scope" value={savedLoraScope} onChange={(event) => setSavedLoraScope(event.target.value as "all" | "mine" | "org")}>
						<option value="all">Mine + organization</option><option value="mine">Mine</option><option value="org">Organization</option>
					</select>
					<select aria-label="Checkpoint provider" value={savedLoraProvider} onChange={(event) => setSavedLoraProvider(event.target.value)}>
						<option value="all">All providers</option><option value="tinker">Tinker</option><option value="river">River</option><option value="synth">Synth</option><option value="imported">Imported</option>
					</select>
					<select aria-label="Checkpoint algorithm" value={savedLoraAlgorithm} onChange={(event) => setSavedLoraAlgorithm(event.target.value)}>
						<option value="all">All algorithms</option><option value="sft">SFT</option><option value="cispo">CISPO</option><option value="ppo">PPO</option>
					</select>
					<select aria-label="Checkpoint kind" value={savedLoraKind} onChange={(event) => setSavedLoraKind(event.target.value)}>
						<option value="all">All checkpoint kinds</option><option value="inference">Inference LoRA</option><option value="training">Training state</option>
					</select>
					<button className="secondary-button" type="button" disabled={savedLoraBusy} onClick={() => void refreshSavedLoras()}>{savedLoraBusy ? "Loading…" : "Refresh"}</button>
				</div>
				<div className="optimizer-checkpoint-grid">
					{savedLoras.map((checkpoint) => (
						<article className="optimizer-checkpoint-card" key={checkpoint.checkpointId} data-testid={`saved-lora-${checkpoint.checkpointId}`}>
							<div className="optimizer-recipe-top"><span className="optimizer-recipe-mark">LR</span><span className={`optimizer-status ${checkpoint.status}`}>{checkpoint.visibility === "private" ? "Private" : "Organization"}</span></div>
							<h3>{checkpoint.name}</h3>
							<p>{checkpoint.description || checkpoint.baseModel}</p>
							<dl><dt>Base</dt><dd>{checkpoint.baseModel}</dd><dt>Algorithm</dt><dd>{checkpoint.lineage.optimizerAlgorithm ?? checkpoint.optimizerAlgorithm ?? "Imported"}</dd><dt>Run</dt><dd>{checkpoint.lineage.runId ?? checkpoint.runId ?? "—"}</dd><dt>Attempt</dt><dd>{checkpoint.lineage.attemptId ?? checkpoint.attemptId ?? "—"}</dd><dt>Source</dt><dd>{checkpoint.lineage.sourceCheckpointId ?? checkpoint.sourceCheckpointId ?? "—"}</dd><dt>Provider</dt><dd>{checkpoint.provider} · {checkpoint.checkpointKind}</dd><dt>Rank / step</dt><dd>{checkpoint.loraRank ?? "—"} / {checkpoint.step ?? "—"}</dd><dt>Storage</dt><dd>{checkpoint.storage.backend} · {formatBytes(checkpoint.storage.sizeBytes)}</dd><dt>Saved</dt><dd>{checkpoint.updatedAt ? formatWhen(checkpoint.updatedAt) : "—"}</dd></dl>
							{checkpoint.tags.length > 0 ? <div className="optimizer-checkpoint-tags">{checkpoint.tags.map((tag) => <span key={tag}>{tag}</span>)}</div> : null}
							<div className="optimizer-checkpoint-actions">{checkpoint.lineage.runId || checkpoint.runId ? <button className="secondary-button" type="button" onClick={() => void openCheckpointRun(checkpoint)}>Open run</button> : null}<button className="secondary-button" type="button" disabled={savedLoraBusy} onClick={() => void downloadSavedLora(checkpoint)}>Download</button><button className="secondary-button optimizer-danger-button" type="button" disabled={savedLoraBusy} onClick={() => void archiveSavedLora(checkpoint)}>Archive</button></div>
						</article>
					))}
					{savedLoras.length === 0 && !savedLoraBusy ? <div className="optimizer-empty"><span className="optimizer-empty-icon" aria-hidden>◇</span><strong>No checkpoints match</strong><p>Inference LoRAs and resumable training state appear automatically after object-storage verification.</p></div> : null}
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
							const limits = (recipe.limits ?? {}) as Record<string, unknown>;
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
											prompt: `Run the Workshop eval recipe ${recipe.id} on policy variants in this project. Stage the policy files with optimizer_stage_eval_candidates using workspace-relative paths, kind python-code.v1, entrypoint policy:Policy, one labelled candidate each, marking the baseline; then call optimizer_start_recipe with the recipe id and returned candidate_set_id. Never replace a policy on your own. Report the run status and selection status separately, the per-candidate scorecard, and the evidence directory.`
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

				<section id="optimizer-run-inspector" className="optimizer-inspector" aria-label="Optimizer inspector">
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
							{trainingProjection ? (
								<section className="optimizer-training-progress" data-testid="optimizer-training-progress">
									<div className="optimizer-training-title">
										<span className="optimizer-eyebrow">Hosted training</span>
										<span className={`optimizer-status ${trainingProjection.lifecycle}`}>{trainingProjection.lifecycle.replaceAll("_", " ")}</span>
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
									{selectedRunCheckpoints.map((checkpoint) => <article key={checkpoint.checkpointId} className="optimizer-run-output"><strong>{checkpoint.name}</strong><small>{checkpoint.checkpointKind} · step {checkpoint.step ?? "—"} · {checkpoint.storage.backend}</small><code>{checkpoint.lineage.sourceCheckpointId ?? checkpoint.sourceCheckpointId ?? checkpoint.checkpointId}</code><div className="optimizer-checkpoint-actions"><button type="button" className="secondary-button" onClick={() => showCheckpointInCatalog(checkpoint)}>View in catalog</button><button type="button" className="secondary-button" disabled={savedLoraBusy} onClick={() => void downloadSavedLora(checkpoint)}>Download</button></div></article>)}
									{selectedRunOutputs && selectedRunOutputs.counts.artifacts === 0 && selectedRunCheckpoints.length === 0 ? <p>No persisted outputs have been published yet. Results and checkpoints appear automatically as the run reaches publication boundaries.</p> : null}
								</section>
							) : null}
							{evalState ? (
								<section className="optimizer-eval-scorecard" data-testid="optimizer-eval-scorecard">
									<span className="optimizer-eyebrow">Scorecard</span>
									<table>
										<thead>
											<tr><th>Candidate</th><th>Stage</th><th>Valid</th><th>Failed</th><th>Primary</th><th>Lift</th></tr>
										</thead>
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
									{evalState.evidenceDir ? (
										<code className="optimizer-eval-evidence" data-testid="optimizer-eval-evidence">{evalState.evidenceDir}</code>
									) : null}
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
