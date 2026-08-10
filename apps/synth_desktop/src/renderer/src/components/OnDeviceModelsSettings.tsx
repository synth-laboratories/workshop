import { useCallback, useEffect, useState } from "react";
import type { LagunaDownloadProgress, LagunaModelHit, LagunaStatus } from "../env";
import { ProviderMark } from "./ProviderMark";

function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
	const gib = bytes / 1024 ** 3;
	return gib >= 1 ? `${gib.toFixed(1)} GB` : `${(bytes / 1024 ** 2).toFixed(0)} MB`;
}

const LAGUNA_XS = {
	modelId: "poolside/Laguna-XS-2.1-NVFP4-mlx",
	id: "laguna-xs-2.1",
	title: "Laguna XS 2.1 NVFP4",
	description: "Poolside Laguna XS 2.1 33B/3B-active coding MoE, NVFP4 quantized.",
	provider: "poolside",
	quantization: "NVFP4",
	context: "262k context",
	parameters: "33B / 3B active",
	estMemory: "30 GB",
	downloadSize: "20 GB",
	fit: "High" as const
};

const MUSE_GLIMMER = {
	modelId: "meta-models/Muse-Glimmer-30B-GGUF",
	id: "muse-glimmer-30b",
	title: "Muse Glimmer 30B",
	description: "Meta's dense agentic model with controllable reasoning, tool use, and image understanding.",
	provider: "Meta",
	quantization: "K-Quant 4-bit",
	context: "131k context",
	parameters: "29.6B dense",
	estMemory: "24–32 GB",
	downloadSize: "19.8 GB incl. vision + DFlash",
	fit: "High" as const
};

const MODEL_CATALOG = [LAGUNA_XS, MUSE_GLIMMER];

type Props = {
	lagunaPhase?: string | null;
	onReloadLaguna: () => Promise<LagunaStatus>;
};

export function OnDeviceModelsSettings({ lagunaPhase, onReloadLaguna }: Props) {
	const [hits, setHits] = useState<LagunaModelHit[]>([]);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [reloadState, setReloadState] = useState<"idle" | "reloading" | "ready" | "error">("idle");
	const [reloadDetail, setReloadDetail] = useState<string | null>(null);
	const [downloadProgress, setDownloadProgress] = useState<LagunaDownloadProgress | null>(null);

	const refresh = useCallback(async () => {
		setBusy(true);
		setError(null);
		try {
			setHits((await window.synthLaguna?.listModels()) ?? []);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	}, []);

	useEffect(() => {
		void refresh();
	}, [refresh]);

	useEffect(() => window.synthLaguna?.onDownloadProgress?.(setDownloadProgress), []);

	const selected = hits.find((hit) => hit.selected) ?? null;
	const modelsRoot = selected?.modelsRoot ?? hits[0]?.modelsRoot ?? "~/.synth-desktop/models";
	const catalogPaths = new Set(MODEL_CATALOG.map((model) => model.modelId));
	const alternates = hits.filter((hit) => !catalogPaths.has(hit.modelId) || (hit.modelId === selected?.modelId && hit.path !== selected.path));

	const download = useCallback(async (modelId: string) => {
		setBusy(true);
		setError(null);
		try {
			await window.synthLaguna?.downloadModel(modelId);
			setHits((await window.synthLaguna?.listModels()) ?? []);
			setReloadState("reloading");
			setReloadDetail("Starting the selected model…");
			const status = await onReloadLaguna();
			setReloadState("ready");
			setReloadDetail(status.detail ?? "Model is ready.");
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	}, [onReloadLaguna]);

	const deleteModel = useCallback(async (modelId: string) => {
		const model = MODEL_CATALOG.find((candidate) => candidate.modelId === modelId);
		if (!window.confirm(`Delete ${model?.title ?? "this model"} and its managed weights from this Mac?`)) return;
		setBusy(true);
		setError(null);
		try {
			await window.synthLaguna?.deleteModel(modelId);
			setHits((await window.synthLaguna?.listModels()) ?? []);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	}, []);

	const choose = useCallback(async () => {
		setBusy(true);
		setError(null);
		try {
			const path = await window.synthLaguna?.chooseModelDirectory();
			if (!path) return;
			await window.synthLaguna?.setModelDirectory(path);
			setHits((await window.synthLaguna?.listModels()) ?? []);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	}, []);

	const selectHit = useCallback(async (path: string) => {
		setBusy(true);
		setError(null);
		try {
			await window.synthLaguna?.setModelDirectory(path);
			setHits((await window.synthLaguna?.listModels()) ?? []);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	}, []);

	const reloadLaguna = async () => {
		setReloadState("reloading");
		setReloadDetail("Reloading Laguna XS…");
		try {
			const status = await onReloadLaguna();
			setReloadState("ready");
			setReloadDetail(status.detail ?? "Laguna XS is ready.");
		} catch (reason) {
			setReloadState("error");
			setReloadDetail(reason instanceof Error ? reason.message : String(reason));
		}
	};

	return (
		<div className="on-device-models" data-testid="laguna-model-locations">
			<div className="on-device-toolbar">
				<code className="on-device-path">Models downloaded to {modelsRoot}</code>
				<div className="on-device-toolbar-actions">
					<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void refresh()}>
						Search again
					</button>
					<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void choose()}>
						Choose folder…
					</button>
					<button
						type="button"
						className="settings-secondary-btn"
						onClick={() => void reloadLaguna()}
						disabled={reloadState === "reloading"}
					>
						{reloadState === "reloading" ? "Reloading…" : "Reload"}
					</button>
					{reloadState !== "idle" ? (
						<p
							data-testid="laguna-reload-status"
							role={reloadState === "error" ? "alert" : "status"}
							data-state={reloadState}
						>
							{reloadState === "reloading" && lagunaPhase ? `Laguna ${lagunaPhase}…` : reloadDetail}
						</p>
					) : null}
				</div>
			</div>

			{error ? (
				<p className="model-locations-error" role="alert">
					{error}
				</p>
			) : null}

			<section className="model-download-guide" aria-labelledby="model-download-guide-title">
				<div>
					<strong id="model-download-guide-title">Managed downloads</strong>
					<p>Choose Download and keep Workshop open. Runtime setup, verified model artifacts, selection, and startup happen automatically.</p>
				</div>
				<ul>
					<li>Laguna XS: 20.1 GB download · about 30 GB memory</li>
					<li>Muse Glimmer: 19.8 GB download · 24–32 GB+ memory · includes vision and DFlash</li>
				</ul>
				<p className="model-download-guide-status">Live progress appears on the model card. Interrupted downloads resume when you choose Download again.</p>
			</section>

			<div className="on-device-grid" data-testid="on-device-recommended">
				{MODEL_CATALOG.map((model) => {
					const installed = hits.find((hit) => hit.modelId === model.modelId) ?? null;
					const inUse = installed?.selected ?? false;
					const progress = downloadProgress?.modelId === model.modelId ? downloadProgress : null;
					const modelBusy = busy && (!downloadProgress || progress !== null);
					const pct = progress?.totalBytes && progress.downloadedBytes != null
						? Math.min(100, Math.round(progress.downloadedBytes / progress.totalBytes * 100))
						: null;
					return <article key={model.modelId} className={`on-device-card${installed ? " installed" : ""}`} data-testid={`on-device-${model.id}`}>
					<header className="on-device-card-top">
						<div className="on-device-card-identity">
							<span className="on-device-card-mark" aria-hidden>
								<ProviderMark kind="laguna" className="on-device-card-mark-img" />
							</span>
							<div>
								<div className="on-device-card-title-row">
									<strong>{model.title}</strong>
									<span className="on-device-fit">Fit: {model.fit}</span>
									{inUse ? <span className="model-location-badge">In use</span> : null}
								</div>
								<p>{model.description}</p>
							</div>
						</div>
						{inUse ? (
							<button
								type="button"
								className="on-device-delete"
								disabled={modelBusy}
								onClick={() => void deleteModel(model.modelId)}
								data-testid={`delete-${model.id}`}
							>
								Delete
							</button>
						) : installed ? (
							<button type="button" className="on-device-download" disabled={modelBusy} onClick={() => void selectHit(installed.path)}>
								Use
							</button>
						) : (
							<button
								type="button"
								className="on-device-download"
								disabled={modelBusy}
								onClick={() => void download(model.modelId)}
								data-testid={`download-${model.id}`}
							>
								{modelBusy ? (progress?.phase === "provisioning" ? "Installing…" : "Downloading…") : "Download"}
							</button>
						)}
					</header>

					<dl className="on-device-specs">
						<div>
							<dt>Provider</dt>
							<dd>{model.provider}</dd>
						</div>
						<div>
							<dt>Quantization</dt>
							<dd>{model.quantization}</dd>
						</div>
						<div>
							<dt>Context</dt>
							<dd>{model.context}</dd>
						</div>
						<div>
							<dt>Parameters</dt>
							<dd>{model.parameters}</dd>
						</div>
						<div>
							<dt>Est. memory</dt>
							<dd>{model.estMemory}</dd>
						</div>
						<div>
							<dt>Download size</dt>
							<dd>{installed ? formatBytes(installed.totalBytes) : model.downloadSize}</dd>
						</div>
					</dl>

					{installed ? (
						<p className="on-device-installed">
							Installed · {installed.shardCount} weight shards
							{installed.companionBytes > 0 ? ` · DFlash ${formatBytes(installed.companionBytes)}` : ""}
							{!installed.runtimeReady ? " · Runtime needs repair" : ""} · <code>{installed.path}</code>
						</p>
					) : (
						<p className="on-device-installed muted">
							{modelBusy
								? (progress?.detail ?? `Preparing ${model.title}…`)
								: "Not installed yet. Download the weights or choose an existing folder."}
						</p>
					)}
					{modelBusy ? (
						<div className="model-managed-progress" data-testid={`progress-${model.id}`}>
							<div className={`progress-track${pct == null ? " is-indeterminate" : ""}`} role="progressbar" aria-valuenow={pct ?? undefined} aria-valuemin={0} aria-valuemax={100} aria-label={progress?.detail ?? `Preparing ${model.title}`}>
								<div className={`progress-fill${pct == null ? " progress-indeterminate" : ""}`} style={pct == null ? undefined : { width: `${pct}%` }} />
							</div>
							<span>
								{progress?.detail ?? `Preparing ${model.title}…`}
								{pct != null ? ` · ${formatBytes(progress?.downloadedBytes ?? 0)} of ${formatBytes(progress?.totalBytes ?? 0)} · ${pct}%` : ""}
							</span>
						</div>
					) : null}
				</article>;
				})}
			</div>

			{alternates.length ? (
				<div className="on-device-alternates">
					<p className="on-device-alternates-label">Other copies on disk</p>
					{alternates.map((hit) => (
						<div className="model-location-row" key={hit.path}>
							<div className="model-location-copy">
								<strong>Laguna XS</strong>
								<code>{hit.path}</code>
								<span>
									{hit.shardCount} weight shards · {formatBytes(hit.totalBytes)}
								</span>
							</div>
							<button
								type="button"
								className="settings-secondary-btn"
								disabled={busy}
								onClick={() => void selectHit(hit.path)}
							>
								Use this copy
							</button>
						</div>
					))}
				</div>
			) : null}

			<p className="model-locations-note">Downloads, runtime setup, selection, startup, and removal are managed here. Muse includes its 17 GB 4-bit K-quant, vision projector, and DFlash speculator.</p>
		</div>
	);
}
