import { useEffect, useMemo, useState } from "react";
import type { OptimizerRecipeInfo, TrainingArtifact } from "../bridge";
import type { ContainerDeployment } from "../generated/protocol";
import { useOptimizerRun, useRunCollection } from "../hooks/useRunRead";
import { runtimeStorage } from "../preferences/runtimeStorage";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";
import { inspectMlxReadiness, planModelInstall, trainingArtifacts } from "../runtime/trainingExperience";
import type { MlxReadiness, ModelInstallPlan } from "../runtime/trainingExperience";
import { TrainingEvaluationCurve } from "./TrainingEvaluationCurve";

type View = "setup" | "train" | "artifacts" | "run" | "inference" | "eval";
type Artifact = { id: string; kind: string; algorithm: string; baseModel: string; runId: string; datasetDigest: string; configDigest: string; sha256: string; size: string; integrity: string; backends: string[] };
type TrainingTarget = { id: string; title: string; taskFamily: string };
type Evaluation = { phase?: string; step?: number | null; score?: number | null; loss?: number | null; delta?: number | null; macro_f1?: number | null; ci_low?: number | null; ci_high?: number | null; confidence?: number | null; paired_n?: number | null; verdict?: string | null; claim_ready?: boolean | null; checkpoint_id?: string | null; artifact_digest?: string | null; evaluator?: string | null; sample_count?: number | null; status?: string; detail?: unknown };
/**
 * The launch record this view owns. Everything durable about the run —
 * status, evaluations, loss — is read through the shared run read model
 * (`useOptimizerRun` / `useRunCollection`), never re-derived here from a
 * polled event prefix. `"unstarted"` is the one local placeholder: the
 * launch threw, so no durable run exists to have a status at all.
 */
type Run = {
	id: string;
	status: string;
	algorithm: "sft" | "cispo";
	error?: string;
    capabilities?: { pause?: boolean; resume?: boolean; cancel?: boolean };
    recoverableMirror?: boolean;
};

function recoverableMirror(record: { source: string; algorithmId: string; status: string; error?: unknown }): boolean {
    const error = typeof record.error === "object" && record.error !== null ? record.error as Record<string, unknown> : {};
    return record.source === "hosted" && record.algorithmId === "sft" && record.status === "failed" && error.source === "sidecar-training"
        && typeof error.message === "string" && (error.message.startsWith("training event sequence gap") || error.message.includes("event polling stayed unavailable"));
}

const MODEL_ID = "Qwen/Qwen3.5-2B";
const MODEL_TITLE = "Qwen 3.5 2B";
const MODEL_REVISION = "15852e8c16360a2fea060d615a32b45270f8a8fc";
const LOCAL_SFT_RECIPE_ID = "sft.qwen35-2b.mlx.v1";
const HOSTED_SFT_RECIPE_ID = "sft.banking77.nemotron-lightning.tinker.v1";
const LOCAL_CISPO_RECIPE_ID = "cispo.mlx.v1";
const HOSTED_CISPO_CANONICAL_ID = "cispo.banking77.tinker.v1";
const HOSTED_CISPO_ALIAS_ID = "cispo.hosted.tinker.v1";

function hostedCispoRecipeId(recipes: readonly OptimizerRecipeInfo[]): string {
	return recipes.some((recipe) => recipe.id === HOSTED_CISPO_CANONICAL_ID)
		? HOSTED_CISPO_CANONICAL_ID
		: HOSTED_CISPO_ALIAS_ID;
}

function trainingRecipeId(algorithm: "sft" | "cispo", placement: "mlx" | "tinker", recipes: readonly OptimizerRecipeInfo[] = []): string {
	if (algorithm === "cispo") return placement === "mlx" ? LOCAL_CISPO_RECIPE_ID : hostedCispoRecipeId(recipes);
	return placement === "mlx" ? LOCAL_SFT_RECIPE_ID : HOSTED_SFT_RECIPE_ID;
}

function bytes(value?: number | null): string { return value == null ? "—" : value >= 1024 ** 3 ? `${(value / 1024 ** 3).toFixed(2)} GB` : `${(value / 1024 ** 2).toFixed(1)} MB`; }
function message(error: unknown): string { return publicError(error); }
function Kv({ values }: { values: Array<[string, string]> }) { return <dl className="training-kv">{values.map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{value}</dd></div>)}</dl>; }
function present(item: TrainingArtifact): Artifact { return { id: item.id, kind: item.adapterKind, algorithm: item.producingAlgorithm.toUpperCase(), baseModel: item.baseModelId, runId: item.producingRunId, datasetDigest: item.datasetDigest ?? "—", configDigest: item.configDigest ?? "—", sha256: item.digest ?? "—", size: bytes(item.sizeBytes), integrity: item.integrity[0].toUpperCase() + item.integrity.slice(1), backends: item.compatibleInference }; }
function presentTarget(item: ContainerDeployment): TrainingTarget | null {
	if (item.status !== "ready" || !item.taskFamily) return null;
	return { id: item.id, title: item.name || item.taskFamily, taskFamily: item.taskFamily };
}
/** A durable `evaluations` collection row → the curve's point shape. */
function evaluationFromRow(details: unknown): Evaluation | null {
	if (typeof details !== "object" || details == null) return null;
	const row = details as Record<string, unknown>;
	const number = (value: unknown) => (typeof value === "number" && Number.isFinite(value) ? value : null);
	const string = (value: unknown) => (typeof value === "string" ? value : null);
	return {
		phase: string(row.phase) ?? undefined,
		step: number(row.step),
		score: number(row.score),
		loss: number(row.loss),
		delta: number(row.delta),
		macro_f1: number(row.macroF1),
		ci_low: number(row.ciLow),
		ci_high: number(row.ciHigh),
		confidence: number(row.confidence),
		paired_n: number(row.pairedN),
		verdict: string(row.verdict),
		claim_ready: typeof row.claimReady === "boolean" ? row.claimReady : null,
		checkpoint_id: string(row.checkpointId),
		artifact_digest: string(row.artifactDigest),
		evaluator: string(row.evaluator),
		sample_count: number(row.sampleCount),
		status: string(row.status) ?? undefined,
		detail: { sequence: row.sequence, childRunId: row.childRunId, pairedN: row.pairedN, confidence: row.confidence, ciLow: row.ciLow, ciHigh: row.ciHigh, verdict: row.verdict, claimReady: row.claimReady }
	};
}

export function TrainingWorkspace({ onStartAgent, sessionRef, onEnsureApprovalSession }: { onStartAgent?: () => void; sessionRef?: string | null; onEnsureApprovalSession?: () => Promise<string> }) {
	const [view, setView] = useState<View>("artifacts");
	const [algorithm, setAlgorithm] = useState<"sft" | "cispo">("sft");
	const [placement, setPlacement] = useState<"mlx" | "tinker">("mlx");
	const [hostedModel, setHostedModel] = useState("openai/gpt-oss-20b");
	const [sftEvaluatorId, setSftEvaluatorId] = useState("");
	const [sftSteps, setSftSteps] = useState(2);
	const [sftCostCap, setSftCostCap] = useState(5);
	const [sftEvaluationMode, setSftEvaluationMode] = useState<"none" | "builtin" | "container" | "both">("none");
	const [selectedId, setSelectedId] = useState("");
	const [parentArtifactId, setParentArtifactId] = useState("");
	const [targetId, setTargetId] = useState("");
	const [targets, setTargets] = useState<TrainingTarget[]>([]);
	const [recipes, setRecipes] = useState<OptimizerRecipeInfo[]>([]);
	const [artifacts, setArtifacts] = useState<Artifact[]>([]);
	const [readiness, setReadiness] = useState<MlxReadiness | null>(null);
	const [plan, setPlan] = useState<ModelInstallPlan | null>(null);
	const [installing, setInstalling] = useState(false);
	const [checkingRuntime, setCheckingRuntime] = useState(false);
	const [installingRuntime, setInstallingRuntime] = useState(false);
	const [run, setRun] = useState<Run | null>(null);
    const [controlling, setControlling] = useState(false);
    useEffect(() => {
        const id = runtimeStorage.getItem("synth.training.lastRunId");
        if (!id || !bridges.optimizers) return;
        let live = true;
        void bridges.optimizers.get(id).then(record => {
            if (!live || !["sft", "cispo"].includes(record.algorithmId)) return;
            setRun({ id: record.id, status: record.status, algorithm: record.algorithmId as "sft" | "cispo", capabilities: record.capabilities, recoverableMirror: recoverableMirror(record) });
            setPlacement(runtimeStorage.getItem("synth.training.lastPlacement") === "mlx" ? "mlx" : "tinker");
            setView("run");
        }).catch(() => undefined);
        return () => { live = false; };
    }, []);
	const [error, setError] = useState<string | null>(null);
    const checkpointEvaluators = (recipes.find((recipe) => recipe.id === trainingRecipeId("sft", "tinker", recipes))?.limits?.checkpointEvaluators ?? []) as Array<{id: string; title: string; imageDigest?: string}>;
	const artifact = useMemo(() => artifacts.find((item) => item.id === selectedId) ?? null, [artifacts, selectedId]);
	const parentArtifact = useMemo(() => artifacts.find((item) => item.id === parentArtifactId && item.algorithm === "SFT") ?? null, [artifacts, parentArtifactId]);

	const loadArtifacts = async () => { const items = (await trainingArtifacts.list()).map(present); setArtifacts(items); setSelectedId((current) => items.some((item) => item.id === current) ? current : items[0]?.id ?? ""); };
	useEffect(() => { let live = true; void Promise.all([inspectMlxReadiness(), planModelInstall(), trainingArtifacts.list(), bridges.inventory?.listContainers() ?? Promise.resolve([]), bridges.optimizers?.listRecipes() ?? Promise.resolve([])]).then(([nextReadiness, nextPlan, items, containers, nextRecipes]) => { if (!live) return; setReadiness(nextReadiness); setPlan(nextPlan); setRecipes(nextRecipes); const next = items.map(present); const nextTargets = containers.map(presentTarget).filter((item): item is TrainingTarget => item != null); setArtifacts(next); setSelectedId(next[0]?.id ?? ""); setTargets(nextTargets); setTargetId(nextTargets[0]?.id ?? ""); }).catch((reason) => { if (live) setError(message(reason)); }); return () => { live = false; }; }, []);
	// The durable run: one bounded summary subscription, revalidated on
	// notification, plus one explicit page of the evaluations collection.
	// No timer, no `eventsAfter(run.id, 0, 2000)` — the read model owns both
	// the transport and the projection; this view only formats them.
	const durableRunId = run && !["starting", "failed-to-start"].includes(run.id) ? run.id : null;
	const summaryState = useOptimizerRun(durableRunId);
	const producerCondition = summaryState.summary?.condition;
    const durableStatus = producerCondition && ["evaluation_blocked", "budget_blocked", "operation_uncertain", "waiting_for_producer"].includes(producerCondition)
        ? producerCondition : summaryState.summary?.status ?? run?.status ?? "unstarted";
	const durableTerminal = summaryState.summary?.lifecycle === "terminal";
	const connection: "live" | "reconnecting" = summaryState.status === "stale" || summaryState.status === "error" ? "reconnecting" : "live";
	const evaluationPage = useRunCollection(durableRunId, "evaluations", { limit: 100, enabled: durableRunId != null });
	const runEvaluations = useMemo(
		() => (evaluationPage.page?.rows ?? []).flatMap((row) => { const point = evaluationFromRow(row.details); return point ? [point] : []; }),
		[evaluationPage.page]
	);
	const runError = run?.error ?? (summaryState.status === "error" ? summaryState.error : undefined);
	useEffect(() => { if (durableTerminal) void loadArtifacts().catch((reason) => setError(message(reason))); }, [durableTerminal]);

	const start = async () => {
		if (placement === "mlx" && readiness?.runtimeHealth !== "ready") {
			setError("Install the Synth MLX training runtime before starting local training.");
			setView("setup");
			return;
		}
		if (placement === "mlx" && !targetId) {
			setError("Register and probe a ready training container before starting this run.");
			return;
		}

		setRun({ id: "starting", status: "starting", algorithm }); setView("run");
		try {
            const approvalSession = (await onEnsureApprovalSession?.()) ?? sessionRef;
            if (!approvalSession) throw new Error("Training requires an approval context");
			if (!bridges.optimizers) throw new Error("Local optimizer runtime is unavailable");
			const recipeId = trainingRecipeId(algorithm, placement, recipes);
			const selectedRecipe = recipes.find((recipe) => recipe.id === recipeId);
			if (selectedRecipe?.availability !== "available") throw new Error(selectedRecipe?.availabilityReason ?? `Optimizer recipe ${recipeId} is not available`);
			if (algorithm === "cispo" && placement === "mlx" && !parentArtifact) throw new Error("CISPO requires an explicit SFT parent training artifact id");
			const record = await bridges.optimizers.startRecipe({
				recipeId,
                sessionRef: approvalSession,
				openVisual: false,
                ...(algorithm === "sft" && placement === "tinker" ? { baseModel: hostedModel, planOverride: { sft: { evaluationMode: sftEvaluationMode, evaluatorId: sftEvaluatorId, trainingSteps: sftSteps, maxCostUsd: sftCostCap } } } : {}),
				...(placement === "mlx" ? { containerId: targetId } : {}),
				...(algorithm === "cispo" && placement === "mlx" ? { trainingArtifactId: parentArtifact!.id } : {})
			});
			setRun({ id: record.id, status: record.status, algorithm, capabilities: record.capabilities, recoverableMirror: recoverableMirror(record) });
            runtimeStorage.setItem("synth.training.lastRunId", record.id);
            runtimeStorage.setItem("synth.training.lastPlacement", placement);
		} catch (reason) { setRun({ id: "failed-to-start", status: "unstarted", algorithm, error: message(reason) }); }
	};
    const control = async (action: "pause" | "resume" | "cancel") => {
        if (!durableRunId || !bridges.optimizers || controlling) return;
        setControlling(true);
        try {
            const record = await bridges.optimizers[action](durableRunId);
            setRun(current => current ? { ...current, id: record.id, status: record.status, capabilities: record.capabilities, recoverableMirror: recoverableMirror(record), error: undefined } : current);
            runtimeStorage.setItem("synth.training.lastRunId", record.id);
        } catch (reason) { setError(message(reason)); }
        finally { setControlling(false); }
    };
	const target = targets.find((item) => item.id === targetId) ?? null;
	const recipeAvailable = (id: string) => recipes.some((recipe) => recipe.id === id && recipe.availability === "available");
	const localAvailable = recipeAvailable(trainingRecipeId(algorithm, "mlx", recipes));
	const hostedAvailable = recipeAvailable(trainingRecipeId(algorithm, "tinker", recipes));
	const selectedPlacementAvailable = placement === "mlx" ? localAvailable : hostedAvailable;
	const selectedRecipeReason = recipes.find((recipe) => recipe.id === trainingRecipeId(algorithm, placement, recipes))?.availabilityReason;

	return <section className="training-workspace" aria-labelledby="training-title" data-testid="training-workspace">
		<div className="training-heading"><div><span className="optimizer-eyebrow">Model training</span><h2 id="training-title">Training</h2></div><span className="training-status" data-state={readiness?.runtimeHealth === "ready" ? "ready" : "failed"}>{placement === "tinker" ? "Hosted Tinker" : readiness?.runtimeHealth ?? "Unavailable"}</span></div>
		<div className="training-launch-row"><div className="training-flow" aria-label="SFT workflow"><span>Collect</span><span>Train</span><span>Compare</span></div>{onStartAgent ? <button className="secondary-button" type="button" onClick={onStartAgent}>Plan with agent</button> : null}</div>
		<nav className="training-tabs" aria-label="Training sections">{(["setup", "train", "artifacts", "run"] as const).map((item) => <button key={item} type="button" aria-current={view === item ? "page" : undefined} onClick={() => setView(item)} data-testid={`training-tab-${item}`}>{item === "setup" ? "Setup" : item === "train" ? "New run" : item[0].toUpperCase() + item.slice(1)}</button>)}</nav>
		{error ? <div className="training-terminal" role="alert"><strong>Unavailable</strong><span>{error}</span></div> : null}

		{view === "setup" ? <div className="training-panel" data-testid="training-setup" data-install-state={installing || installingRuntime ? "installing" : readiness?.runtimeHealth !== "ready" || !plan?.alreadyPresent ? "absent" : "ready"}><div className="training-section-head"><h3>MLX readiness</h3><span className="training-status" data-state={readiness?.runtimeHealth === "ready" ? "ready" : "absent"}>{readiness?.runtimeHealth ?? "unknown"}</span></div><Kv values={[["Platform", readiness?.platform ?? "unknown"], ["Runtime", readiness?.runtimeVersion ?? "—"], ["Runtime health", readiness?.runtimeHealth ?? "unknown"], ["Memory", bytes(readiness?.availableMemoryBytes)], ["Disk", bytes(readiness?.availableDiskBytes)]]} />{readiness?.runtimeHealth === "missing" ? <div className="training-terminal" role="alert" data-testid="mlx-runtime-missing"><strong>MLX training runtime not installed</strong><span>Workshop can install the signed, version-pinned synth-mlx-rl 0.6.0 distribution included with this app. Training will not start until verification succeeds.</span><div className="training-actions"><button type="button" className="primary-button" disabled={installingRuntime} onClick={() => { setInstallingRuntime(true); setError(null); void bridges.trainingModels?.installRuntime(true).then(() => inspectMlxReadiness()).then(setReadiness).catch((reason) => setError(message(reason))).finally(() => setInstallingRuntime(false)); }}>{installingRuntime ? "Installing verified runtime…" : "Install MLX runtime"}</button><button type="button" className="secondary-button" disabled={checkingRuntime || installingRuntime} onClick={() => { setCheckingRuntime(true); setError(null); void inspectMlxReadiness().then(setReadiness).catch((reason) => setError(message(reason))).finally(() => setCheckingRuntime(false)); }}>{checkingRuntime ? "Checking…" : "Check again"}</button></div></div> : null}<div className="training-model"><div className="training-section-head"><h3>{plan?.title ?? MODEL_TITLE}</h3><span className="training-status" data-state={plan?.alreadyPresent ? "ready" : "absent"}>{plan?.alreadyPresent ? "Ready" : "Absent"}</span></div><Kv values={[["Source", plan?.source ?? MODEL_ID], ["Revision", plan?.revision ?? MODEL_REVISION], ["Digest", plan?.digest ?? "—"], ["License", plan?.license ?? "—"], ["Download", bytes(plan?.downloadBytes)]]} /><button type="button" className="primary-button" disabled={!bridges.trainingModels || installing || plan?.alreadyPresent} onClick={() => { setInstalling(true); setError(null); void bridges.trainingModels?.downloadModel(plan?.modelId ?? MODEL_ID).then(() => planModelInstall()).then(setPlan).catch((reason) => setError(message(reason))).finally(() => setInstalling(false)); }}>{plan?.alreadyPresent ? "Managed copy installed" : installing ? "Installing…" : "Install managed copy"}</button></div></div> : null}

		{view === "train" ? <form className="training-panel training-form" data-testid="training-form" onSubmit={(event) => { event.preventDefault(); void start(); }}><h3>New training run</h3><div className="training-form-grid"><label><span>Base model</span><select value={placement === "mlx" ? MODEL_ID : hostedModel} disabled={placement === "mlx" || algorithm !== "sft"} onChange={(event) => setHostedModel(event.target.value)}>{placement === "mlx" ? <option value={MODEL_ID}>{MODEL_TITLE}</option> : <><option value="openai/gpt-oss-20b">GPT-OSS 20B</option><option value="nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16">Nemotron 3.5 Lightning 30B-A3B</option></>}</select></label><label><span>Dataset / workload</span><select disabled={placement === "tinker"} required={placement === "mlx"} value={targetId} onChange={(event) => setTargetId(event.target.value)}><option value="" disabled>{placement === "tinker" ? "Banking77 · registered corpus" : "Select a ready container…"}</option>{targets.map((item) => <option key={item.id} value={item.id}>{item.title} · {item.taskFamily}</option>)}</select></label><label><span>Recipe</span><select value={algorithm} onChange={(event) => { const next = event.target.value as "sft" | "cispo"; setAlgorithm(next); setParentArtifactId(""); const nextLocal = recipeAvailable(trainingRecipeId(next, "mlx", recipes)); setPlacement(nextLocal ? "mlx" : "tinker"); }}><option value="sft" disabled={!recipeAvailable(trainingRecipeId("sft", "mlx", recipes)) && !recipeAvailable(trainingRecipeId("sft", "tinker", recipes))}>SFT</option><option value="cispo" disabled={!recipeAvailable(trainingRecipeId("cispo", "mlx", recipes)) && !recipeAvailable(trainingRecipeId("cispo", "tinker", recipes))}>CISPO</option></select></label><label><span>Compute</span><select value={selectedPlacementAvailable ? placement : ""} onChange={(event) => setPlacement(event.target.value as "mlx" | "tinker")}><option value="" disabled>No admitted compute</option>{localAvailable ? <option value="mlx">This Mac · MLX</option> : null}{hostedAvailable ? <option value="tinker">Hosted · Tinker</option> : null}</select></label>{algorithm === "cispo" && placement === "mlx" ? <label><span>Parent artifact</span><select required value={parentArtifactId} onChange={(event) => setParentArtifactId(event.target.value)}><option value="" disabled>Select an SFT artifact…</option>{artifacts.filter((item) => item.algorithm === "SFT").map((item) => <option key={item.id} value={item.id}>{item.id} · {item.sha256}</option>)}</select></label> : null}</div>{algorithm === "sft" && placement === "tinker" ? <><label><span>Training steps</span><input type="number" min={1} max={100} value={sftSteps} onChange={(event) => setSftSteps(Number(event.target.value))} /></label><label><span>Maximum charge (USD)</span><input type="number" min={0.01} max={15} step={0.01} value={sftCostCap} onChange={(event) => setSftCostCap(Number(event.target.value))} /></label></> : null}{algorithm === "sft" && placement === "tinker" ? <label><span>Checkpoint evaluation</span><select value={sftEvaluationMode} onChange={(event) => setSftEvaluationMode(event.target.value as "none" | "builtin" | "container" | "both")}><option value="none">None · train and save checkpoints</option><option value="builtin">Built-in validation</option><option value="container">Live container evaluation</option><option value="both">Built-in and live container evaluation</option></select></label> : null}{algorithm === "sft" && placement === "tinker" && ["container", "both"].includes(sftEvaluationMode) ? <label><span>Checkpoint evaluator</span><select required value={sftEvaluatorId} onChange={(event) => setSftEvaluatorId(event.target.value)}><option value="" disabled>Select a pinned evaluator…</option>{checkpointEvaluators.map((item) => <option key={item.id} value={item.id} disabled={!item.imageDigest}>{item.title}{item.imageDigest ? " · pinned image" : " · image required"}</option>)}</select><small>Baseline, saved checkpoints and selected final · two trials per panel · frozen checkpoint provenance</small></label> : null}<div className="training-preview" data-testid="training-resolved-config"><h4>Resolved configuration</h4><Kv values={[["Model", placement === "mlx" ? `${MODEL_TITLE} · ${MODEL_REVISION}` : hostedModel], ["Dataset", placement === "tinker" ? "Banking77 · registered corpus" : target?.title ?? "No ready container"], ["Workload", placement === "tinker" ? "banking77" : target?.taskFamily ?? "—"], ["Container", placement === "tinker" && algorithm === "sft" ? (sftEvaluationMode === "container" || sftEvaluationMode === "both" ? "GSM8K · frozen checkpoint evaluator" : "No container evaluation") : target?.id ?? "—"], ["Recipe", algorithm.toUpperCase()], ...(algorithm === "cispo" && placement === "mlx" ? [["Parent artifact", parentArtifact?.id ?? "Explicit selection required"], ["Parent digest", parentArtifact?.sha256 ?? "—"]] as Array<[string, string]> : []), ["Compute", selectedPlacementAvailable ? placement === "mlx" ? "This Mac · MLX" : "Hosted · Tinker" : "Unavailable"], ["Eval schedule", algorithm === "sft" && placement === "tinker" && sftEvaluationMode === "none" ? "Disabled · checkpoints are unevaluated" : "Before · checkpoints · final"], ["Eval transport", placement === "mlx" ? "Container tunnel · exact checkpoint" : "Public Tinker service"], ["Output", "Managed artifacts"]]} /></div>{!selectedPlacementAvailable ? <div className="training-terminal" role="status" data-testid="training-recipe-unavailable"><strong>No admitted {algorithm.toUpperCase()} recipe</strong><span>{selectedRecipeReason ?? "Optimizers did not advertise a runnable placement for this algorithm."}</span></div> : null}{placement === "mlx" && readiness?.runtimeHealth !== "ready" ? <div className="training-terminal" role="alert"><strong>Setup required</strong><span>Install the Synth MLX training runtime in Setup before starting this run.</span></div> : null}{placement === "mlx" && !targets.length ? <div className="training-terminal" role="alert"><strong>No training workload</strong><span>Register and probe a container that advertises an SFT or CISPO training contract.</span></div> : null}<button type="submit" className="primary-button" disabled={!selectedPlacementAvailable || (placement === "mlx" && !targets.length) || (placement === "mlx" && readiness?.runtimeHealth !== "ready") || (algorithm === "cispo" && placement === "mlx" && !parentArtifact)}>Start bounded run</button></form> : null}

		{view === "artifacts" ? <div className="training-panel" data-testid="training-artifact-library"><div className="training-section-head"><h3>Local artifacts</h3><span>{artifacts.length}</span></div><div className="training-artifact-grid">{artifacts.map((item) => <article className="training-artifact" key={item.id} data-testid={`training-artifact-${item.id}`}><div className="training-section-head"><span className="training-algorithm">{item.algorithm}</span><span className="training-status" data-state={item.integrity === "Verified" ? "ready" : "failed"}>{item.integrity}</span></div><button type="button" className="training-artifact-title" onClick={() => setSelectedId(item.id)}>{item.id}</button><Kv values={[["Base model", item.baseModel], ["Kind", item.kind], ["Producing run", item.runId], ["Dataset", item.datasetDigest], ["Config", item.configDigest], ["Size", item.size]]} /></article>)}</div>{artifact ? <div className="training-detail" data-testid="training-artifact-detail"><h3>{artifact.id}</h3><Kv values={[["Base model", artifact.baseModel], ["Adapter", `${artifact.kind} · ${artifact.sha256}`], ["Producing run", artifact.runId], ["Compatible", artifact.backends.join(" · ")]]} /><div className="training-actions"><button type="button" className="primary-button" onClick={() => setView("inference")}>Run inference</button><button type="button" className="secondary-button" onClick={() => setView("eval")}>Evaluate</button><button type="button" className="secondary-button" onClick={() => { const destination = window.prompt("Export destination directory"); if (!destination) return; void trainingArtifacts.export(artifact.id, destination).catch((reason) => setError(message(reason))); }}>Export</button><button type="button" className="secondary-button training-danger" onClick={() => { if (!window.confirm(`Delete ${artifact.id}? This cannot be undone.`)) return; void trainingArtifacts.delete(artifact.id).then(loadArtifacts).catch((reason) => setError(message(reason))); }}>Delete</button></div></div> : null}</div> : null}

		{view === "run" ? <div className="training-panel" data-testid="training-run-view" data-read-model={summaryState.status} data-projection-revision={summaryState.revision}>{run ? <><div className="training-section-head"><div><span className="training-algorithm">{run.algorithm.toUpperCase()}</span><h3>{run.id}</h3></div><span className="training-status" data-state={durableStatus === "failed" || durableStatus === "unstarted" ? "failed" : durableStatus === "completed" ? "ready" : "installing"} data-connection={connection}>{durableStatus}</span></div>{runError ? <div className="training-terminal" role="alert" data-testid="training-run-failure"><strong>Training failed</strong><span>{runError}</span></div> : <Kv values={[["Recipe", run.algorithm.toUpperCase()], ["Status", durableStatus], ["Execution", placement === "mlx" ? "Local MLX" : "Hosted Tinker"], ["Step", summaryState.summary?.usage.steps != null ? String(summaryState.summary.usage.steps) : "—"], ["Evaluations", evaluationPage.page ? String(evaluationPage.page.total) : "—"]]} />}{["evaluation_blocked", "budget_blocked", "operation_uncertain"].includes(durableStatus) ? <p role="alert">{durableStatus === "evaluation_blocked" ? "Checkpoint evaluation is blocked. Retained child-job evidence must be reconciled before continuing." : durableStatus === "budget_blocked" ? "The frozen budget blocked further work. Existing charges and checkpoints are retained." : "A provider operation has an uncertain outcome. Automatic replay is blocked; its reserved charge is retained."}</p> : null}{run.recoverableMirror && durableStatus === "failed" ? <button type="button" className="secondary-button" disabled={controlling} onClick={() => void control("resume")}>Recover paused producer run</button> : null}{!durableTerminal ? <div className="training-actions">
                {run.capabilities?.pause && durableStatus === "running" ? <button type="button" className="secondary-button" disabled={controlling} onClick={() => void control("pause")}>Pause at checkpoint</button> : null}
                {run.capabilities?.resume && durableStatus === "paused" ? <button type="button" className="secondary-button" disabled={controlling} onClick={() => void control("resume")}>Resume</button> : null}
                {run.capabilities?.cancel && !["starting", "unstarted"].includes(durableStatus) ? <button type="button" className="secondary-button" disabled={controlling} onClick={() => void control("cancel")}>Cancel and drain</button> : null}
            </div> : null}{evaluationPage.stale ? <p className="training-stale" role="status" data-testid="training-evaluations-stale">Showing the last durable evaluations while the projection refreshes.</p> : null}{runEvaluations.length ? <TrainingEvaluationCurve evaluations={runEvaluations} testId="training-evaluation-comparison" /> : null}</> : <div className="training-terminal"><strong>No run</strong></div>}</div> : null}

		{view === "inference" || view === "eval" ? <div className="training-panel" data-testid={`artifact-${view}`}><button type="button" className="training-back" onClick={() => setView("artifacts")}>← Artifacts</button>{artifact ? <><h3>{view === "eval" ? "Evaluate artifact" : "Run inference"}</h3><Kv values={[["Artifact", artifact.id], ["Base model", artifact.baseModel], ["Adapter", `${artifact.kind} · ${artifact.sha256}`]]} /><button type="button" className="primary-button" onClick={() => { const action = view === "eval" ? trainingArtifacts.launchEval(artifact.id, "eval.mlx.local-policy.smoke.v1") : trainingArtifacts.launchInference(artifact.id); void action.catch((reason) => setError(message(reason))); }}>{view === "eval" ? "Start Eval" : "Start inference"}</button></> : null}</div> : null}
	</section>;
}
