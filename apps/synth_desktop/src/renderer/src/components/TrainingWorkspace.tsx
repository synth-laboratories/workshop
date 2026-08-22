import { useEffect, useMemo, useState } from "react";
import type { TrainingArtifact } from "../bridge";
import type { ContainerDeployment } from "../generated/protocol";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";
import { isTerminalRunStatus } from "../runtime/runProgress/types";
import { inspectMlxReadiness, planModelInstall, trainingArtifacts } from "../runtime/trainingExperience";
import type { MlxReadiness, ModelInstallPlan } from "../runtime/trainingExperience";
import { TrainingEvaluationCurve } from "./TrainingEvaluationCurve";

type View = "setup" | "train" | "artifacts" | "run" | "inference" | "eval";
type Artifact = { id: string; kind: string; algorithm: string; baseModel: string; runId: string; datasetDigest: string; configDigest: string; sha256: string; size: string; integrity: string; backends: string[] };
type TrainingTarget = { id: string; title: string; taskFamily: string };
type Evaluation = { phase?: string; step?: number | null; score?: number | null; loss?: number | null; delta?: number | null; checkpoint_id?: string | null; artifact_digest?: string | null; evaluator?: string | null; sample_count?: number | null; status?: string; detail?: unknown };
/**
 * `status` is exactly what the durable optimizer record says; the renderer
 * never writes it. `connection` is this view's own reading state — a poll that
 * cannot reach the host says nothing about whether the run is still training,
 * and writing `status: "failed"` there marked live runs dead.
 */
type Run = {
	id: string;
	/** `"unstarted"` is the one local placeholder: the launch threw, so no
	 * durable run exists to have a status at all. */
	status: string;
	connection?: "live" | "reconnecting";
	algorithm: "sft" | "cispo";
	error?: string;
	evaluations: Evaluation[];
};

const MODEL_ID = "Qwen/Qwen3.5-0.8B";
const MODEL_TITLE = "Qwen 3.5 0.8B";
const MODEL_REVISION = "2fc06364715b967f1860aea9cf38778875588b17";

function bytes(value?: number | null): string { return value == null ? "—" : value >= 1024 ** 3 ? `${(value / 1024 ** 3).toFixed(2)} GB` : `${(value / 1024 ** 2).toFixed(1)} MB`; }
function message(error: unknown): string { return publicError(error); }
function Kv({ values }: { values: Array<[string, string]> }) { return <dl className="training-kv">{values.map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{value}</dd></div>)}</dl>; }
function present(item: TrainingArtifact): Artifact { return { id: item.id, kind: item.adapterKind, algorithm: item.producingAlgorithm.toUpperCase(), baseModel: item.baseModelId, runId: item.producingRunId, datasetDigest: item.datasetDigest ?? "—", configDigest: item.configDigest ?? "—", sha256: item.digest ?? "—", size: bytes(item.sizeBytes), integrity: item.integrity[0].toUpperCase() + item.integrity.slice(1), backends: item.compatibleInference }; }
function presentTarget(item: ContainerDeployment): TrainingTarget | null {
	if (item.status !== "ready" || !item.taskFamily) return null;
	return { id: item.id, title: item.name || item.taskFamily, taskFamily: item.taskFamily };
}
function evaluations(events: unknown[]): Evaluation[] { return events.flatMap((raw) => { if (typeof raw !== "object" || raw == null) return []; const event = raw as Record<string, unknown>; const delta = typeof event.delta === "object" && event.delta != null ? event.delta as Record<string, unknown> : {}; return event.type === "training.evaluation.completed" && typeof delta.evaluation === "object" && delta.evaluation != null ? [delta.evaluation as Evaluation] : []; }); }

export function TrainingWorkspace({ onStartAgent }: { onStartAgent?: () => void }) {
	const [view, setView] = useState<View>("artifacts");
	const [algorithm, setAlgorithm] = useState<"sft" | "cispo">("sft");
	const [placement, setPlacement] = useState<"mlx" | "tinker">("mlx");
	const [selectedId, setSelectedId] = useState("");
	const [targetId, setTargetId] = useState("");
	const [targets, setTargets] = useState<TrainingTarget[]>([]);
	const [artifacts, setArtifacts] = useState<Artifact[]>([]);
	const [readiness, setReadiness] = useState<MlxReadiness | null>(null);
	const [plan, setPlan] = useState<ModelInstallPlan | null>(null);
	const [installing, setInstalling] = useState(false);
	const [checkingRuntime, setCheckingRuntime] = useState(false);
	const [installingRuntime, setInstallingRuntime] = useState(false);
	const [run, setRun] = useState<Run | null>(null);
	const [error, setError] = useState<string | null>(null);
	const artifact = useMemo(() => artifacts.find((item) => item.id === selectedId) ?? null, [artifacts, selectedId]);

	const loadArtifacts = async () => { const items = (await trainingArtifacts.list()).map(present); setArtifacts(items); setSelectedId((current) => items.some((item) => item.id === current) ? current : items[0]?.id ?? ""); };
	useEffect(() => { let live = true; void Promise.all([inspectMlxReadiness(), planModelInstall(), trainingArtifacts.list(), bridges.inventory?.listContainers() ?? Promise.resolve([])]).then(([nextReadiness, nextPlan, items, containers]) => { if (!live) return; setReadiness(nextReadiness); setPlan(nextPlan); const next = items.map(present); const nextTargets = containers.map(presentTarget).filter((item): item is TrainingTarget => item != null); setArtifacts(next); setSelectedId(next[0]?.id ?? ""); setTargets(nextTargets); setTargetId(nextTargets[0]?.id ?? ""); }).catch((reason) => { if (live) setError(message(reason)); }); return () => { live = false; }; }, []);
	useEffect(() => {
		if (!run || ["starting", "failed-to-start"].includes(run.id) || isTerminalRunStatus(run.status)) return;
		const bridge = bridges.optimizers; if (!bridge) return;
		const poll = async () => { try { const [record, events] = await Promise.all([bridge.refresh(run.id), bridge.eventsAfter(run.id, 0, 2000)]); setRun((current) => current?.id === record.id ? { ...current, status: record.status, connection: "live", error: undefined, evaluations: evaluations(events) } : current); if (isTerminalRunStatus(record.status)) await loadArtifacts(); } catch (reason) { setRun((current) => current ? { ...current, connection: "reconnecting", error: message(reason) } : current); } };
		void poll(); const timer = window.setInterval(() => void poll(), 1000); return () => window.clearInterval(timer);
	}, [run?.id, run?.status]);

	const start = async () => {
		if (placement === "mlx" && readiness?.runtimeHealth !== "ready") {
			setError("Install the Synth MLX training runtime before starting local training.");
			setView("setup");
			return;
		}
		if (!targetId) {
			setError("Register and probe a ready training container before starting this run.");
			return;
		}
		setRun({ id: "starting", status: "starting", connection: "live", algorithm, evaluations: [] }); setView("run");
		try { if (!bridges.optimizers) throw new Error("Local optimizer runtime is unavailable"); const recipeId = placement === "mlx" ? algorithm === "cispo" ? "cispo.mlx.v1" : "sft.qwen35-0.8b.mlx.v1" : algorithm === "cispo" ? "cispo.slime.hosted.v1" : "sft.banking77.nemotron-lightning.tinker.v1"; if (algorithm === "cispo" && !selectedId) throw new Error("CISPO requires an explicit parent training artifact id"); const record = await bridges.optimizers.startRecipe({ recipeId, containerId: targetId, openVisual: false, ...(algorithm === "cispo" ? { trainingArtifactId: selectedId } : {}) }); setRun({ id: record.id, status: record.status, connection: "live", algorithm, evaluations: [] }); } catch (reason) { setRun({ id: "failed-to-start", status: "unstarted", connection: "reconnecting", algorithm, evaluations: [], error: message(reason) }); }
	};
	const target = targets.find((item) => item.id === targetId) ?? null;

	return <section className="training-workspace" aria-labelledby="training-title" data-testid="training-workspace">
		<div className="training-heading"><div><span className="optimizer-eyebrow">Local MLX</span><h2 id="training-title">Training</h2></div><span className="training-status" data-state={readiness?.runtimeHealth === "ready" ? "ready" : "failed"}>{readiness?.runtimeHealth ?? "Unavailable"}</span></div>
		<div className="training-launch-row" data-testid="optimizer-guide-sft"><div className="training-flow" aria-label="SFT workflow"><span>Collect</span><span>Train</span><span>Compare</span></div>{onStartAgent ? <button className="secondary-button" type="button" onClick={onStartAgent} data-testid="start-sft-agent">Plan with agent</button> : null}</div>
		<nav className="training-tabs" aria-label="Training sections">{(["setup", "train", "artifacts", "run"] as const).map((item) => <button key={item} type="button" aria-current={view === item ? "page" : undefined} onClick={() => setView(item)} data-testid={`training-tab-${item}`}>{item === "setup" ? "Setup" : item === "train" ? "New run" : item[0].toUpperCase() + item.slice(1)}</button>)}</nav>
		{error ? <div className="training-terminal" role="alert"><strong>Unavailable</strong><span>{error}</span></div> : null}

		{view === "setup" ? <div className="training-panel" data-testid="training-setup" data-install-state={installing || installingRuntime ? "installing" : readiness?.runtimeHealth !== "ready" || !plan?.alreadyPresent ? "absent" : "ready"}><div className="training-section-head"><h3>MLX readiness</h3><span className="training-status" data-state={readiness?.runtimeHealth === "ready" ? "ready" : "absent"}>{readiness?.runtimeHealth ?? "unknown"}</span></div><Kv values={[["Platform", readiness?.platform ?? "unknown"], ["Runtime", readiness?.runtimeVersion ?? "—"], ["Runtime health", readiness?.runtimeHealth ?? "unknown"], ["Memory", bytes(readiness?.availableMemoryBytes)], ["Disk", bytes(readiness?.availableDiskBytes)]]} />{readiness?.runtimeHealth === "missing" ? <div className="training-terminal" role="alert" data-testid="mlx-runtime-missing"><strong>MLX training runtime not installed</strong><span>Workshop can install the signed, version-pinned synth-mlx-rl 0.6.0 distribution included with this app. Training will not start until verification succeeds.</span><div className="training-actions"><button type="button" className="primary-button" disabled={installingRuntime} onClick={() => { setInstallingRuntime(true); setError(null); void bridges.trainingModels?.installRuntime(true).then(() => inspectMlxReadiness()).then(setReadiness).catch((reason) => setError(message(reason))).finally(() => setInstallingRuntime(false)); }}>{installingRuntime ? "Installing verified runtime…" : "Install MLX runtime"}</button><button type="button" className="secondary-button" disabled={checkingRuntime || installingRuntime} onClick={() => { setCheckingRuntime(true); setError(null); void inspectMlxReadiness().then(setReadiness).catch((reason) => setError(message(reason))).finally(() => setCheckingRuntime(false)); }}>{checkingRuntime ? "Checking…" : "Check again"}</button></div></div> : null}<div className="training-model"><div className="training-section-head"><h3>{plan?.title ?? MODEL_TITLE}</h3><span className="training-status" data-state={plan?.alreadyPresent ? "ready" : "absent"}>{plan?.alreadyPresent ? "Ready" : "Absent"}</span></div><Kv values={[["Source", plan?.source ?? MODEL_ID], ["Revision", plan?.revision ?? MODEL_REVISION], ["Digest", plan?.digest ?? "—"], ["License", plan?.license ?? "—"], ["Download", bytes(plan?.downloadBytes)]]} /><button type="button" className="primary-button" disabled={!bridges.trainingModels || installing || plan?.alreadyPresent} onClick={() => { setInstalling(true); setError(null); void bridges.trainingModels?.downloadModel(plan?.modelId ?? MODEL_ID).then(() => planModelInstall()).then(setPlan).catch((reason) => setError(message(reason))).finally(() => setInstalling(false)); }}>{plan?.alreadyPresent ? "Managed copy installed" : installing ? "Installing…" : "Install managed copy"}</button></div></div> : null}

		{view === "train" ? <form className="training-panel training-form" data-testid="training-form" onSubmit={(event) => { event.preventDefault(); void start(); }}><h3>New training run</h3><div className="training-form-grid"><label><span>Base model</span><select value={MODEL_ID} disabled><option value={MODEL_ID}>{MODEL_TITLE}</option></select></label><label><span>Dataset / workload</span><select required value={targetId} onChange={(event) => setTargetId(event.target.value)}><option value="" disabled>Select a ready container…</option>{targets.map((item) => <option key={item.id} value={item.id}>{item.title} · {item.taskFamily}</option>)}</select></label><label><span>Recipe</span><select value={algorithm} onChange={(event) => setAlgorithm(event.target.value as "sft" | "cispo")}><option value="sft">SFT</option><option value="cispo">CISPO</option></select></label><label><span>Compute</span><select value={placement} onChange={(event) => setPlacement(event.target.value as "mlx" | "tinker")}><option value="mlx">This Mac · MLX</option><option value="tinker">Hosted · Tinker</option></select></label>{algorithm === "cispo" ? <label><span>Parent artifact</span><select required value={selectedId} onChange={(event) => setSelectedId(event.target.value)}>{artifacts.filter((item) => item.algorithm === "SFT").map((item) => <option key={item.id}>{item.id}</option>)}</select></label> : null}</div><div className="training-preview" data-testid="training-resolved-config"><h4>Resolved configuration</h4><Kv values={[["Model", `${MODEL_TITLE} · ${MODEL_REVISION}`], ["Dataset", target?.title ?? "No ready container"], ["Workload", target?.taskFamily ?? "—"], ["Container", target?.id ?? "—"], ["Recipe", algorithm.toUpperCase()], ["Compute", placement === "mlx" ? "This Mac · MLX" : "Hosted · Tinker"], ["Eval schedule", "Before · checkpoints · final"], ["Eval transport", "Container tunnel · exact checkpoint"], ["Output", "Managed artifacts"]]} /></div>{placement === "mlx" && readiness?.runtimeHealth !== "ready" ? <div className="training-terminal" role="alert"><strong>Setup required</strong><span>Install the Synth MLX training runtime in Setup before starting this run.</span></div> : null}{!targets.length ? <div className="training-terminal" role="alert"><strong>No training workload</strong><span>Register and probe a container that advertises an SFT or CISPO training contract.</span></div> : null}<button type="submit" className="primary-button" disabled={!targets.length || (placement === "mlx" && readiness?.runtimeHealth !== "ready")}>Start bounded run</button></form> : null}

		{view === "artifacts" ? <div className="training-panel" data-testid="training-artifact-library"><div className="training-section-head"><h3>Local artifacts</h3><span>{artifacts.length}</span></div><div className="training-artifact-grid">{artifacts.map((item) => <article className="training-artifact" key={item.id} data-testid={`training-artifact-${item.id}`}><div className="training-section-head"><span className="training-algorithm">{item.algorithm}</span><span className="training-status" data-state={item.integrity === "Verified" ? "ready" : "failed"}>{item.integrity}</span></div><button type="button" className="training-artifact-title" onClick={() => setSelectedId(item.id)}>{item.id}</button><Kv values={[["Base model", item.baseModel], ["Kind", item.kind], ["Producing run", item.runId], ["Dataset", item.datasetDigest], ["Config", item.configDigest], ["Size", item.size]]} /></article>)}</div>{artifact ? <div className="training-detail" data-testid="training-artifact-detail"><h3>{artifact.id}</h3><Kv values={[["Base model", artifact.baseModel], ["Adapter", `${artifact.kind} · ${artifact.sha256}`], ["Producing run", artifact.runId], ["Compatible", artifact.backends.join(" · ")]]} /><div className="training-actions"><button type="button" className="primary-button" onClick={() => setView("inference")}>Run inference</button><button type="button" className="secondary-button" onClick={() => setView("eval")}>Evaluate</button><button type="button" className="secondary-button" onClick={() => { const destination = window.prompt("Export destination directory"); if (!destination) return; void trainingArtifacts.export(artifact.id, destination).catch((reason) => setError(message(reason))); }}>Export</button><button type="button" className="secondary-button training-danger" onClick={() => { if (!window.confirm(`Delete ${artifact.id}? This cannot be undone.`)) return; void trainingArtifacts.delete(artifact.id).then(loadArtifacts).catch((reason) => setError(message(reason))); }}>Delete</button></div></div> : null}</div> : null}

		{view === "run" ? <div className="training-panel" data-testid="training-run-view">{run ? <><div className="training-section-head"><div><span className="training-algorithm">{run.algorithm.toUpperCase()}</span><h3>{run.id}</h3></div><span className="training-status" data-state={run.status === "failed" || run.status === "unstarted" ? "failed" : run.status === "completed" ? "ready" : "installing"} data-connection={run.connection ?? "live"}>{run.status}</span></div>{run.error ? <div className="training-terminal" role="alert" data-testid="training-run-failure"><strong>Training failed</strong><span>{run.error}</span></div> : <Kv values={[["Recipe", run.algorithm.toUpperCase()], ["Status", run.status], ["Execution", placement === "mlx" ? "Local MLX" : "Hosted Tinker"], ["Evaluations", String(run.evaluations.length)]]} />}{run.evaluations.length ? <TrainingEvaluationCurve evaluations={run.evaluations} testId="training-evaluation-comparison" /> : null}</> : <div className="training-terminal"><strong>No run</strong></div>}</div> : null}

		{view === "inference" || view === "eval" ? <div className="training-panel" data-testid={`artifact-${view}`}><button type="button" className="training-back" onClick={() => setView("artifacts")}>← Artifacts</button>{artifact ? <><h3>{view === "eval" ? "Evaluate artifact" : "Run inference"}</h3><Kv values={[["Artifact", artifact.id], ["Base model", artifact.baseModel], ["Adapter", `${artifact.kind} · ${artifact.sha256}`]]} /><button type="button" className="primary-button" onClick={() => { const action = view === "eval" ? trainingArtifacts.launchEval(artifact.id, "eval.mlx.local-policy.smoke.v1") : trainingArtifacts.launchInference(artifact.id); void action.catch((reason) => setError(message(reason))); }}>{view === "eval" ? "Start Eval" : "Start inference"}</button></> : null}</div> : null}
	</section>;
}
