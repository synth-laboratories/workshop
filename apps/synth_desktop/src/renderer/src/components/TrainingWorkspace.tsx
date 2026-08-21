import { useEffect, useMemo, useState } from "react";
import type { TrainingArtifact } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { inspectMlxReadiness, planModelInstall, trainingArtifacts } from "../runtime/trainingExperience";
import type { MlxReadiness, ModelInstallPlan } from "../runtime/trainingExperience";
import { TrainingEvaluationCurve } from "./TrainingEvaluationCurve";

type View = "setup" | "train" | "artifacts" | "run" | "inference" | "eval";
type Artifact = { id: string; kind: string; algorithm: string; baseModel: string; runId: string; datasetDigest: string; configDigest: string; sha256: string; size: string; integrity: string; backends: string[] };
type Evaluation = { phase?: string; step?: number | null; score?: number | null; loss?: number | null; delta?: number | null; checkpoint_id?: string | null; artifact_digest?: string | null; evaluator?: string | null; sample_count?: number | null; status?: string; detail?: unknown };
type Run = { id: string; status: string; algorithm: "sft" | "cispo"; error?: string; evaluations: Evaluation[] };
type TrainingLane = { recipeId: string; modelId: string; modelLabel: string; datasetLabel: string; datasetShard?: string; requiresParent: boolean };

const MODEL_ID = "Qwen/Qwen3.5-0.8B";
const MODEL_TITLE = "Qwen 3.5 0.8B";
const MODEL_REVISION = "2fc06364715b967f1860aea9cf38778875588b17";

function trainingLane(placement: "mlx" | "tinker", algorithm: "sft" | "cispo"): TrainingLane {
	if (placement === "mlx" && algorithm === "sft") return { recipeId: "sft.qwen35-0.8b.mlx.v1", modelId: MODEL_ID, modelLabel: MODEL_TITLE, datasetLabel: "Pinned local SFT cookbook", requiresParent: false };
	if (placement === "mlx") return { recipeId: "cispo.banking77.mlx.v1", modelId: MODEL_ID, modelLabel: MODEL_TITLE, datasetLabel: "Banking77 rollout target", requiresParent: true };
	if (algorithm === "sft") return { recipeId: "sft.banking77.nemotron-lightning.tinker.v1", modelId: "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16", modelLabel: "Nemotron 3.5 Lightning", datasetLabel: "Banking77 · train_a", datasetShard: "train_a", requiresParent: false };
	return { recipeId: "cispo.slime.hosted.v1", modelId: "openai/gpt-oss-20b", modelLabel: "GPT-OSS 20B", datasetLabel: "Banking77 rollout target", requiresParent: true };
}

function bytes(value?: number | null): string { return value == null ? "—" : value >= 1024 ** 3 ? `${(value / 1024 ** 3).toFixed(2)} GB` : `${(value / 1024 ** 2).toFixed(1)} MB`; }
function message(error: unknown): string { return error instanceof Error ? error.message : String(error); }
function Kv({ values }: { values: Array<[string, string]> }) { return <dl className="training-kv">{values.map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{value}</dd></div>)}</dl>; }
function present(item: TrainingArtifact): Artifact { return { id: item.id, kind: item.adapterKind, algorithm: item.producingAlgorithm.toUpperCase(), baseModel: item.baseModelId, runId: item.producingRunId, datasetDigest: item.datasetDigest ?? "—", configDigest: item.configDigest ?? "—", sha256: item.digest ?? "—", size: bytes(item.sizeBytes), integrity: item.integrity[0].toUpperCase() + item.integrity.slice(1), backends: item.compatibleInference }; }
function evaluations(events: unknown[]): Evaluation[] { return events.flatMap((raw) => { if (typeof raw !== "object" || raw == null) return []; const event = raw as Record<string, unknown>; const delta = typeof event.delta === "object" && event.delta != null ? event.delta as Record<string, unknown> : {}; return event.type === "training.evaluation.completed" && typeof delta.evaluation === "object" && delta.evaluation != null ? [delta.evaluation as Evaluation] : []; }); }

export function TrainingWorkspace({ onStartAgent }: { onStartAgent?: () => void }) {
	const [view, setView] = useState<View>("artifacts");
	const [algorithm, setAlgorithm] = useState<"sft" | "cispo">("sft");
	const [placement, setPlacement] = useState<"mlx" | "tinker">("mlx");
	const [selectedId, setSelectedId] = useState("");
	const [artifacts, setArtifacts] = useState<Artifact[]>([]);
	const [readiness, setReadiness] = useState<MlxReadiness | null>(null);
	const [plan, setPlan] = useState<ModelInstallPlan | null>(null);
	const [installing, setInstalling] = useState(false);
	const [run, setRun] = useState<Run | null>(null);
	const [error, setError] = useState<string | null>(null);
	const artifact = useMemo(() => artifacts.find((item) => item.id === selectedId) ?? null, [artifacts, selectedId]);
	const lane = trainingLane(placement, algorithm);
	const sftArtifacts = useMemo(() => artifacts.filter((item) => item.algorithm === "SFT"), [artifacts]);

	const loadArtifacts = async () => { const items = (await trainingArtifacts.list()).map(present); setArtifacts(items); setSelectedId((current) => items.some((item) => item.id === current) ? current : items[0]?.id ?? ""); };
	useEffect(() => { let live = true; void Promise.all([inspectMlxReadiness(), planModelInstall(), trainingArtifacts.list()]).then(([nextReadiness, nextPlan, items]) => { if (!live) return; setReadiness(nextReadiness); setPlan(nextPlan); const next = items.map(present); setArtifacts(next); setSelectedId(next[0]?.id ?? ""); }).catch((reason) => { if (live) setError(message(reason)); }); return () => { live = false; }; }, []);
	useEffect(() => {
		if (!run || ["starting", "failed-to-start"].includes(run.id) || ["completed", "failed", "cancelled"].includes(run.status)) return;
		const bridge = bridges.optimizers; if (!bridge) return;
		const poll = async () => { try { const [record, events] = await Promise.all([bridge.refresh(run.id), bridge.eventsAfter(run.id, 0, 2000)]); setRun((current) => current?.id === record.id ? { ...current, status: record.status, evaluations: evaluations(events) } : current); if (["completed", "failed", "cancelled"].includes(record.status)) await loadArtifacts(); } catch (reason) { setRun((current) => current ? { ...current, status: "failed", error: message(reason) } : current); } };
		void poll(); const timer = window.setInterval(() => void poll(), 1000); return () => window.clearInterval(timer);
	}, [run?.id, run?.status]);

	const start = async () => {
		setRun({ id: "starting", status: "starting", algorithm, evaluations: [] }); setView("run");
		try {
			if (!bridges.optimizers) throw new Error("Local optimizer runtime is unavailable");
			if (lane.requiresParent && !selectedId) throw new Error("Select an SFT parent artifact before starting CISPO");
			const record = await bridges.optimizers.startRecipe({
				recipeId: lane.recipeId,
				openVisual: false,
				baseModel: lane.modelId,
				datasetShard: lane.datasetShard,
				trainingArtifactId: lane.requiresParent ? selectedId : undefined
			});
			setRun({ id: record.id, status: record.status, algorithm, evaluations: [] });
		} catch (reason) { setRun({ id: "failed-to-start", status: "failed", algorithm, evaluations: [], error: message(reason) }); }
	};

	return <section className="training-workspace" aria-labelledby="training-title" data-testid="training-workspace">
		<div className="training-heading"><div><span className="optimizer-eyebrow">Local MLX</span><h2 id="training-title">Training</h2></div><span className="training-status" data-state={readiness?.runtimeHealth === "ready" ? "ready" : "failed"}>{readiness?.runtimeHealth ?? "Unavailable"}</span></div>
		<div className="training-launch-row" data-testid="optimizer-guide-sft"><div className="training-flow" aria-label="SFT workflow"><span>Collect</span><span>Train</span><span>Compare</span></div>{onStartAgent ? <button className="secondary-button" type="button" onClick={onStartAgent} data-testid="start-sft-agent">Plan with agent</button> : null}</div>
		<nav className="training-tabs" aria-label="Training sections">{(["setup", "train", "artifacts", "run"] as const).map((item) => <button key={item} type="button" aria-current={view === item ? "page" : undefined} onClick={() => setView(item)} data-testid={`training-tab-${item}`}>{item === "setup" ? "Setup" : item === "train" ? "New run" : item[0].toUpperCase() + item.slice(1)}</button>)}</nav>
		{error ? <div className="training-terminal" role="alert"><strong>Unavailable</strong><span>{error}</span></div> : null}

		{view === "setup" ? <div className="training-panel" data-testid="training-setup" data-install-state={installing ? "installing" : plan?.alreadyPresent ? "ready" : "absent"}><div className="training-section-head"><h3>MLX readiness</h3><span className="training-status" data-state={readiness?.compatibility === "compatible" ? "ready" : "failed"}>{readiness?.compatibility ?? "unknown"}</span></div><Kv values={[["Platform", readiness?.platform ?? "unknown"], ["Runtime", readiness?.runtimeVersion ?? "—"], ["Runtime health", readiness?.runtimeHealth ?? "unknown"], ["Memory", bytes(readiness?.availableMemoryBytes)], ["Disk", bytes(readiness?.availableDiskBytes)]]} /><div className="training-model"><div className="training-section-head"><h3>{plan?.title ?? MODEL_TITLE}</h3><span className="training-status" data-state={plan?.alreadyPresent ? "ready" : "absent"}>{plan?.alreadyPresent ? "Ready" : "Absent"}</span></div><Kv values={[["Source", plan?.source ?? MODEL_ID], ["Revision", plan?.revision ?? MODEL_REVISION], ["Digest", plan?.digest ?? "—"], ["License", plan?.license ?? "—"], ["Download", bytes(plan?.downloadBytes)]]} /><button type="button" className="primary-button" disabled={!bridges.trainingModels || installing} onClick={() => { setInstalling(true); setError(null); void bridges.trainingModels?.downloadModel(plan?.modelId ?? MODEL_ID).then(() => planModelInstall()).then(setPlan).catch((reason) => setError(message(reason))).finally(() => setInstalling(false)); }}>Install managed copy</button></div></div> : null}

		{view === "train" ? <form className="training-panel training-form" data-testid="training-form" onSubmit={(event) => { event.preventDefault(); void start(); }}><h3>New training run</h3><div className="training-form-grid"><label><span>Base model</span><select value={lane.modelId} disabled><option value={lane.modelId}>{lane.modelLabel}</option></select></label><label><span>Dataset</span><select value={lane.datasetLabel} disabled><option value={lane.datasetLabel}>{lane.datasetLabel}</option></select></label><label><span>Recipe</span><select value={algorithm} onChange={(event) => setAlgorithm(event.target.value as "sft" | "cispo")}><option value="sft">SFT</option><option value="cispo">CISPO</option></select></label><label><span>Compute</span><select value={placement} onChange={(event) => setPlacement(event.target.value as "mlx" | "tinker")}><option value="mlx">This Mac · MLX</option><option value="tinker">Hosted · Tinker</option></select></label>{lane.requiresParent ? <label><span>Parent artifact</span><select required value={selectedId} onChange={(event) => setSelectedId(event.target.value)}><option value="" disabled>Select an SFT artifact</option>{sftArtifacts.map((item) => <option key={item.id} value={item.id}>{item.id}</option>)}</select></label> : null}</div><div className="training-preview" data-testid="training-resolved-config"><h4>Resolved configuration</h4><Kv values={[["Model", lane.modelLabel], ["Dataset", lane.datasetLabel], ["Recipe", lane.recipeId], ["Parent artifact", lane.requiresParent ? selectedId || "Required" : "Not applicable"], ["Compute", placement === "mlx" ? "This Mac · MLX" : "Hosted · Tinker"], ["Eval schedule", "Before · checkpoints · final"], ["Eval transport", "Container tunnel · exact checkpoint"], ["Output", "Managed artifacts"]]} /></div><button type="submit" className="primary-button" disabled={(placement === "mlx" && readiness?.runtimeHealth !== "ready") || (lane.requiresParent && !selectedId)}>Review run</button></form> : null}

		{view === "artifacts" ? <div className="training-panel" data-testid="training-artifact-library"><div className="training-section-head"><h3>Local artifacts</h3><span>{artifacts.length}</span></div><div className="training-artifact-grid">{artifacts.map((item) => <article className="training-artifact" key={item.id} data-testid={`training-artifact-${item.id}`}><div className="training-section-head"><span className="training-algorithm">{item.algorithm}</span><span className="training-status" data-state={item.integrity === "Verified" ? "ready" : "failed"}>{item.integrity}</span></div><button type="button" className="training-artifact-title" onClick={() => setSelectedId(item.id)}>{item.id}</button><Kv values={[["Base model", item.baseModel], ["Kind", item.kind], ["Producing run", item.runId], ["Dataset", item.datasetDigest], ["Config", item.configDigest], ["Size", item.size]]} /></article>)}</div>{artifact ? <div className="training-detail" data-testid="training-artifact-detail"><h3>{artifact.id}</h3><Kv values={[["Base model", artifact.baseModel], ["Adapter", `${artifact.kind} · ${artifact.sha256}`], ["Producing run", artifact.runId], ["Compatible", artifact.backends.join(" · ")]]} /><div className="training-actions"><button type="button" className="primary-button" onClick={() => setView("inference")}>Run inference</button><button type="button" className="secondary-button" onClick={() => setView("eval")}>Evaluate</button></div></div> : null}</div> : null}

		{view === "run" ? <div className="training-panel" data-testid="training-run-view">{run ? <><div className="training-section-head"><div><span className="training-algorithm">{run.algorithm.toUpperCase()}</span><h3>{run.id}</h3></div><span className="training-status" data-state={run.status === "failed" ? "failed" : run.status === "completed" ? "ready" : "installing"}>{run.status}</span></div>{run.error ? <div className="training-terminal" role="alert" data-testid="training-run-failure"><strong>Training failed</strong><span>{run.error}</span></div> : <Kv values={[["Recipe", run.algorithm.toUpperCase()], ["Status", run.status], ["Execution", placement === "mlx" ? "Local MLX" : "Hosted Tinker"], ["Evaluations", String(run.evaluations.length)]]} />}{run.evaluations.length ? <TrainingEvaluationCurve evaluations={run.evaluations} testId="training-evaluation-comparison" /> : null}</> : <div className="training-terminal"><strong>No run</strong></div>}</div> : null}

		{view === "inference" || view === "eval" ? <div className="training-panel" data-testid={`artifact-${view}`}><button type="button" className="training-back" onClick={() => setView("artifacts")}>← Artifacts</button>{artifact ? <><h3>{view === "eval" ? "Evaluate artifact" : "Run inference"}</h3><Kv values={[["Artifact", artifact.id], ["Base model", artifact.baseModel], ["Adapter", `${artifact.kind} · ${artifact.sha256}`]]} /><button type="button" className="primary-button" onClick={() => { const action = view === "eval" ? trainingArtifacts.launchEval(artifact.id, "eval.mlx.local-policy.smoke.v1") : trainingArtifacts.launchInference(artifact.id); void action.catch((reason) => setError(message(reason))); }}>{view === "eval" ? "Start Eval" : "Start inference"}</button></> : null}</div> : null}
	</section>;
}
