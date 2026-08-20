import { useMemo, useRef, useState } from "react";
import fixtures from "../fixtures/training.json";

type TrainingView = "setup" | "train" | "artifacts" | "run" | "inference" | "eval";
type Confirmation = "install" | "start" | "inference" | "eval" | "delete" | null;
type Artifact = { id: string; kind: string; algorithm: string; baseModel: string; runId: string; dataset: string; datasetDigest: string; configDigest: string; sha256: string; size: string; integrity: string; backends: string[]; parentArtifactId: string | null };

function formatBytes(bytes: number | null | undefined): string {
	if (bytes == null) return "—";
	return bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(2)} GB` : `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}

function errorMessage(error: unknown): string {
	if (error instanceof Error) return error.message;
	if (typeof error === "object" && error !== null && "message" in error && typeof error.message === "string") return error.message;
	return String(error);
}

function presentArtifact(item: TrainingArtifact): Artifact {
	return { id: item.id, kind: item.kind, algorithm: item.algorithm.toUpperCase(), baseModel: `${item.baseModel.id}${item.baseModel.revision ? ` · ${item.baseModel.revision}` : ""}`, runId: item.producingRunId, dataset: item.datasetDigest ?? "—", datasetDigest: item.datasetDigest ?? "—", configDigest: item.configDigest ?? "—", sha256: item.sha256 ?? "—", size: formatBytes(item.sizeBytes), integrity: item.integrity[0].toUpperCase() + item.integrity.slice(1), backends: item.compatibleBackends, parentArtifactId: item.parentArtifactId ?? null };
}

function Kv({ values }: { values: Array<[string, string]> }) {
	return <dl className="training-kv">{values.map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{value}</dd></div>)}</dl>;
}

function ConfirmationSheet({ kind, artifact, onClose, onConfirm }: { kind: Exclude<Confirmation, null>; artifact: Artifact; onClose: () => void; onConfirm: () => void }) {
	const title = { install: "Confirm model install", start: "Confirm training run", inference: "Confirm inference", eval: "Confirm Eval", delete: "Confirm artifact deletion" }[kind];
	const rows: Array<[string, string]> = kind === "install" ? [
		["Target", fixtures.model.name], ["Source", fixtures.model.source], ["Revision", fixtures.model.revision], ["Download", fixtures.model.size], ["Disk after", fixtures.model.diskAfter], ["License", fixtures.model.license]
	] : kind === "start" ? [
		["Recipe", "SFT · MLX LoRA"], ["Bounds", "120 steps · 20 min · 12 GB"], ["Dataset", "GSM8K · main@e53f"], ["Output", "Managed artifacts"], ["Runtime", fixtures.readiness.runtime]
	] : kind === "delete" ? [
		["Artifact", artifact.id], ["Kind", artifact.kind], ["References", "2 retained receipts"], ["Result", "Local files removed"]
	] : [
		["Artifact", artifact.id], ["Base model", artifact.baseModel], ["Backend", kind === "eval" ? "Local Eval" : "MLX inference"], [kind === "eval" ? "Recipe" : "Load", kind === "eval" ? "GSM8K exact-match" : "Base + adapter"], [kind === "eval" ? "Metric" : "Resources", kind === "eval" ? "exact_match · higher" : "8 GB memory"]
	];
	return <div className="training-sheet-backdrop"><section className="training-sheet" role="dialog" aria-modal="true" aria-labelledby="training-confirm-title" data-testid={`training-confirm-${kind}`}>
		<h2 id="training-confirm-title">{title}</h2><Kv values={rows} />
		{kind === "install" ? <label className="training-check"><input type="checkbox" required /> <span>License acknowledged</span></label> : null}
		<div className="training-actions"><button type="button" className="secondary-button" onClick={onClose}>Cancel</button><button type="button" className={kind === "delete" ? "secondary-button training-danger" : "primary-button"} onClick={onConfirm}>{kind === "delete" ? "Delete artifact" : "Confirm"}</button></div>
	</section></div>;
}

export function TrainingWorkspace({ onStartFixture, fixtureBusy = false }: { onStartFixture?: () => void; fixtureBusy?: boolean }) {
	const [view, setView] = useState<TrainingView>("artifacts");
	const [algorithm, setAlgorithm] = useState<"sft" | "cispo">("sft");
	const [placement, setPlacement] = useState<"mlx" | "tinker">("mlx");
	const [selectedId, setSelectedId] = useState(fixtures.artifacts[1].id);
	const [confirmation, setConfirmation] = useState<Confirmation>(null);
	const [installState, setInstallState] = useState<"absent" | "installing" | "cancelled" | "failed" | "ready">("ready");
	const [failedLoad, setFailedLoad] = useState(false);
	const [artifacts, setArtifacts] = useState<Artifact[]>(fixtures.artifacts);
	const [readiness, setReadiness] = useState<MlxReadiness | null>(null);
	const [installPlan, setInstallPlan] = useState<ModelInstallPlan | null>(null);
	const [nativeRun, setNativeRun] = useState<{ id: string; status: string; algorithm: "sft" | "cispo"; error?: string } | null>(null);
	const dialogReturn = useRef<TrainingView>("artifacts");
	const artifact = useMemo(() => fixtures.artifacts.find((item) => item.id === selectedId) ?? fixtures.artifacts[0], [selectedId]);
	const openArtifactAction = (next: "inference" | "eval") => { dialogReturn.current = next; setView(next); setFailedLoad(false); };
	const confirm = (kind: Exclude<Confirmation, null>) => setConfirmation(kind);
	const executeConfirmation = async (kind: Exclude<Confirmation, null>) => {
		setConfirmation(null);
		if (kind === "install") {
			setInstallState("installing");
			try { await bridges.trainingModels?.downloadModel(installPlan?.modelId ?? "Qwen/Qwen3.5-0.8B"); setInstallState("ready"); }
			catch { setInstallState("failed"); }
			return;
		}
		if (kind === "delete") { await trainingArtifacts.delete(artifact.id); setArtifacts((items) => items.filter((item) => item.id !== artifact.id)); return; }
		if (kind === "inference") { await trainingArtifacts.launchInference(artifact.id, { mergeAdapter: false }); return; }
		if (kind === "eval") await trainingArtifacts.launchEval(artifact.id, "eval.gsm8k.mlx.v1");
		if (kind === "start") {
			setNativeRun({ id: "starting", status: "starting", algorithm });
			setView("run");
			try {
				if (!bridges.optimizers) throw new Error("Local optimizer runtime is unavailable");
				const recipeId = placement === "mlx"
					? algorithm === "cispo" ? "cispo.banking77.mlx.v1" : "sft.qwen35-0.8b.mlx.v1"
					: algorithm === "cispo" ? "cispo.slime.hosted.v1" : "sft.banking77.nemotron-lightning.tinker.v1";
				const run = await bridges.optimizers.startRecipe({
					recipeId,
					openVisual: false
				});
				setNativeRun({ id: run.id, status: run.status, algorithm });
			} catch (error) {
				setNativeRun({ id: "failed-to-start", status: "failed", algorithm, error: errorMessage(error) });
			}
		}
	};
	useEffect(() => {
		let live = true;
		void Promise.all([inspectMlxReadiness(), planModelInstall(), trainingArtifacts.list()]).then(([nextReadiness, nextPlan, nextArtifacts]) => {
			if (!live) return;
			setReadiness(nextReadiness);
			setInstallPlan(nextPlan);
			if (nextArtifacts.length > 0) {
				const presented = nextArtifacts.map(presentArtifact);
				setArtifacts(presented);
				setSelectedId((current) => presented.some((item) => item.id === current) ? current : presented[0].id);
			}
		}).catch(() => undefined);
		return () => { live = false; };
	}, []);
	useEffect(() => {
		if (!nativeRun || nativeRun.id === "starting" || nativeRun.id === "failed-to-start" || ["completed", "failed", "cancelled"].includes(nativeRun.status)) return;
		const optimizerBridge = bridges.optimizers;
		if (!optimizerBridge) return;
		const timer = window.setInterval(() => {
			void optimizerBridge.refresh(nativeRun.id).then((run) => {
				setNativeRun((current) => current?.id === run.id ? { ...current, status: run.status } : current);
				if (["completed", "failed", "cancelled"].includes(run.status)) {
					void trainingArtifacts.list().then((items) => {
						if (items.length > 0) setArtifacts(items.map(presentArtifact));
					});
				}
			}).catch((error) => setNativeRun((current) => current ? { ...current, status: "failed", error: errorMessage(error) } : current));
		}, 1000);
		return () => window.clearInterval(timer);
	}, [nativeRun?.id, nativeRun?.status]);

	return <section className="training-workspace" aria-labelledby="training-title" data-testid="training-workspace">
		<div className="training-heading"><div><span className="optimizer-eyebrow">Local MLX</span><h2 id="training-title">Training</h2></div><span className="training-status" data-state={fixtures.readiness.runtimeHealth.toLowerCase()}>{fixtures.readiness.runtimeHealth}</span></div>
		<div className="training-launch-row" data-testid="training-launch-row"><div className="training-flow" aria-label="SFT workflow"><span>Collect</span><span>Train</span><span>Compare</span></div>{onStartFixture ? <button className="secondary-button" type="button" disabled={fixtureBusy} onClick={onStartFixture} data-testid="training-start-sft-fixture">{fixtureBusy ? "Starting fixture…" : "Run free fixture"}</button> : null}</div>
		<nav className="training-tabs" aria-label="Training sections">
			{(["setup", "train", "artifacts", "run"] as const).map((item) => <button key={item} type="button" aria-current={view === item ? "page" : undefined} onClick={() => setView(item)} data-testid={`training-tab-${item}`}>{item === "setup" ? "Setup" : item === "train" ? "New run" : item[0].toUpperCase() + item.slice(1)}</button>)}
		</nav>

		{view === "setup" ? <div className="training-panel" data-testid="training-setup" data-install-state={installState}>
			<div className="training-section-head"><h3>MLX readiness</h3><span className="training-status" data-state="ready">Compatible</span></div>
			<Kv values={[["Platform", fixtures.readiness.platform], ["Runtime", fixtures.readiness.runtime], ["Runtime health", fixtures.readiness.runtimeHealth], ["Memory", fixtures.readiness.memory], ["Disk", fixtures.readiness.disk]]} />
			<div className="training-model" data-testid="on-device-training-catalog"><div className="training-section-head"><h3>{fixtures.model.name}</h3><span className="training-status" data-state={installState}>{installState === "failed" ? "Checksum failed" : installState[0].toUpperCase() + installState.slice(1)}</span></div>
				<Kv values={[["Source", fixtures.model.source], ["Revision", fixtures.model.revision], ["Digest", fixtures.model.digest], ["License", fixtures.model.license], ["Download", fixtures.model.size], ["Disk after", fixtures.model.diskAfter]]} />
				{installState === "installing" ? <div className="training-progress"><progress value={63} max={100} aria-label="Qwen model download" data-testid={`progress-${fixtures.model.id}`} /><span>706 MB / 1.12 GB · 63%</span><div className="training-actions"><button type="button" className="secondary-button" onClick={() => setInstallState("cancelled")}>Cancel download</button></div></div> : null}
				{installState === "failed" ? <div role="alert" className="training-terminal" data-testid="training-install-failure"><strong>Checksum failed</strong><span>Managed copy not installed</span></div> : null}
				<div className="training-actions"><button type="button" className="primary-button" onClick={() => confirm("install")}>Install managed copy</button><button type="button" className="secondary-button" onClick={() => setInstallState(installState === "installing" ? "cancelled" : "installing")}>{installState === "cancelled" ? "Resume download" : "Show progress"}</button><button type="button" className="secondary-button" onClick={() => setInstallState("failed")}>Checksum state</button></div>
			</div>
		</div> : null}

		{view === "train" ? <form className="training-panel training-form" data-testid="training-form" onSubmit={(event) => { event.preventDefault(); confirm("start"); }}>
			<h3>New training run</h3><div className="training-form-grid">
				<label><span>Base model</span><select defaultValue={fixtures.model.id}><option value={fixtures.model.id}>{fixtures.model.name}</option></select></label>
				<label><span>Dataset</span><select defaultValue="gsm8k"><option value="gsm8k">GSM8K · main@e53f</option></select></label>
				<label><span>Recipe</span><select value={algorithm} onChange={(event) => setAlgorithm(event.target.value as "sft" | "cispo")}><option value="sft">SFT</option><option value="cispo">CISPO</option></select></label>
				<label><span>Compute</span><select value={placement} onChange={(event) => setPlacement(event.target.value as "mlx" | "tinker")}><option value="mlx">This Mac · MLX</option><option value="tinker">Hosted · Tinker</option></select></label>
				{algorithm === "cispo" ? <label><span>Parent artifact</span><select value={selectedId} onChange={(event) => setSelectedId(event.target.value)}>{artifacts.filter((item) => item.algorithm === "SFT").map((item) => <option key={item.id} value={item.id}>{item.id}</option>)}</select></label> : null}
				<label><span>Steps</span><input type="number" min="1" max="240" defaultValue="120" /></label><label><span>Wall clock</span><select defaultValue="20"><option value="20">20 min</option></select></label><label><span>Memory cap</span><select defaultValue="12"><option value="12">12 GB</option></select></label>
			</div><div className="training-preview" data-testid="training-resolved-config"><h4>Resolved configuration</h4><Kv values={[["Model", `${fixtures.model.name} · ${fixtures.model.revision}`], ["Dataset", "GSM8K · sha256:e53f219a"], ["Recipe", algorithm.toUpperCase()], ["Compute", placement === "mlx" ? "This Mac · MLX" : "Hosted · Tinker"], ["LoRA", "rank 16 · alpha 32 · q/k/v/o_proj"], ["Bounds", "120 steps · 20 min · 12 GB"], ["Eval schedule", "Before · every 40 steps · final"], ["Eval transport", "Container tunnel · exact checkpoint"], ["Output", "Managed artifacts"], ["Runtime", placement === "mlx" ? fixtures.readiness.runtime : "Tinker"]]} /></div>
			<div className="training-actions"><button type="submit" className="primary-button">Review run</button></div>
		</form> : null}

		{view === "artifacts" ? <div className="training-panel" data-testid="training-artifact-library"><div className="training-section-head"><h3>Local artifacts</h3><span>{fixtures.artifacts.length}</span></div><div className="training-artifact-grid">{fixtures.artifacts.map((item) => <article className="training-artifact" key={item.id} data-testid={`training-artifact-${item.id}`}><div className="training-section-head"><span className="training-algorithm">{item.algorithm}</span><span className="training-status" data-state="ready">{item.integrity}</span></div><button type="button" className="training-artifact-title" onClick={() => setSelectedId(item.id)} aria-pressed={selectedId === item.id}>{item.id}</button><Kv values={[["Base model", item.baseModel], ["Kind", item.kind], ["Producing run", item.runId], ["Dataset", item.datasetDigest], ["Config", item.configDigest], ["Size", item.size]]} />{item.parentArtifactId ? <div className="training-lineage" data-testid="training-artifact-lineage"><span>Parent</span><button type="button" onClick={() => setSelectedId(String(item.parentArtifactId))}>{item.parentArtifactId}</button></div> : null}<div className="training-actions"><button type="button" className="primary-button" onClick={() => openArtifactAction("inference")}>Run inference</button><button type="button" className="secondary-button" onClick={() => openArtifactAction("eval")}>Evaluate</button></div></article>)}</div>
			<div className="training-detail" data-testid="training-artifact-detail"><div className="training-section-head"><div><span className="optimizer-eyebrow">Artifact detail</span><h3>{artifact.id}</h3></div><span className="training-status" data-state="ready">{artifact.integrity}</span></div><Kv values={[["Base model", artifact.baseModel], ["Adapter", `${artifact.kind} · ${artifact.sha256}`], ["Producing run", artifact.runId], ["Dataset", artifact.datasetDigest], ["Config", artifact.configDigest], ["Compatible", artifact.backends.join(" · ")]]} /><div className="training-actions"><button type="button" className="primary-button" onClick={() => openArtifactAction("inference")}>Run inference</button><button type="button" className="secondary-button" onClick={() => openArtifactAction("eval")}>Evaluate</button><button type="button" className="secondary-button">Export</button><button type="button" className="secondary-button training-danger" onClick={() => confirm("delete")}>Delete</button></div></div>
		</div> : null}

		{view === "run" ? <div className="training-panel" data-testid="training-run-view"><div className="training-section-head"><div><span className="training-algorithm">{(nativeRun?.algorithm ?? "cispo").toUpperCase()}</span><h3>{nativeRun?.id ?? "run-cispo-20260820-02"}</h3></div><span className="training-status" data-state={nativeRun?.status === "failed" ? "failed" : nativeRun?.status === "completed" ? "ready" : "installing"}>{nativeRun?.status ?? "Completed"}</span></div>{nativeRun?.error ? <div className="training-terminal" role="alert" data-testid="training-run-failure"><strong>Training failed</strong><span>{nativeRun.error}</span></div> : <Kv values={nativeRun ? [["Recipe", nativeRun.algorithm === "cispo" ? "CISPO" : "SFT"], ["Status", nativeRun.status], ["Execution", placement === "mlx" ? "Local MLX" : "Hosted Tinker"], ["Evaluations", "Before · checkpoints · final"], ["Output", "Managed artifacts"]] : [["Input artifact", "mlx-lora-sft-7f31"], ["Step", "120 / 120"], ["Elapsed", "18m 42s"], ["Output", "mlx-lora-cispo-b921"]]} />}{(!nativeRun || nativeRun.algorithm === "cispo") ? <div className="training-metrics" data-testid="training-cispo-diagnostics"><div><span>Policy objective</span><strong>0.184</strong></div><div><span>Reward</span><strong>0.742</strong></div><div><span>Clip fraction</span><strong>0.091</strong></div><div><span>Valid rollouts</span><strong>116 / 120</strong></div></div> : null}{!nativeRun ? <div className="training-evaluation-series" data-testid="training-evaluation-comparison" aria-label="Baseline checkpoint and final evaluations">{fixtures.evaluations.map((evaluation) => <article key={evaluation.phase + evaluation.step} data-phase={evaluation.phase}><span>{evaluation.phase}</span><strong>{evaluation.score.toFixed(3)}</strong><small>{evaluation.phase === "baseline" ? "reference" : `+${evaluation.delta.toFixed(3)} vs baseline`}</small><code>step {evaluation.step} · {evaluation.digest}</code></article>)}</div> : null}<div className="training-actions"><button type="button" className="primary-button" disabled={nativeRun != null && nativeRun.status !== "completed"} onClick={() => { const next = artifacts.at(-1); if (next) setSelectedId(next.id); setView("artifacts"); }}>Open artifact</button><button type="button" className="secondary-button">View metrics</button></div></div> : null}

		{view === "inference" || view === "eval" ? <div className="training-panel" data-testid={`artifact-${view}`}><button type="button" className="training-back" onClick={() => setView("artifacts")}>← Artifacts</button><div className="training-section-head"><div><span className="optimizer-eyebrow">{view === "eval" ? "Local Eval" : "MLX inference"}</span><h3>{view === "eval" ? "Evaluate artifact" : "Run inference"}</h3></div><span className="training-status" data-state={failedLoad ? "failed" : "ready"}>{failedLoad ? "Failed" : "Prefilled"}</span></div><Kv values={[["Artifact", artifact.id], ["Base model", artifact.baseModel], ["Adapter", `${artifact.kind} · ${artifact.sha256}`], ["Producing run", artifact.runId], ["Config", artifact.configDigest], ...(view === "eval" ? [["Recipe", "GSM8K exact-match"], ["Metric", "exact_match · higher"]] as Array<[string, string]> : [["Load", "Base + adapter"], ["Merge", "Off"]] as Array<[string, string]>)]} />{failedLoad ? <div className="training-terminal" role="alert" data-testid="artifact-failed-load"><strong>Adapter load failed</strong><span>Base model revision mismatch</span></div> : <div className="training-actions"><button type="button" className="primary-button" onClick={() => confirm(view)}>{view === "eval" ? "Review Eval" : "Review inference"}</button><button type="button" className="secondary-button" onClick={() => setFailedLoad(true)}>Failed-load state</button></div>}</div> : null}

		{confirmation ? <ConfirmationSheet kind={confirmation} artifact={artifact} onClose={() => setConfirmation(null)} onConfirm={() => { if (confirmation === "install") setInstallState("installing"); setConfirmation(null); }} /> : null}
	</section>;
}
