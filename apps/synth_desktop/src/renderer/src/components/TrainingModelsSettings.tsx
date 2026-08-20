import { useCallback, useEffect, useState } from "react";
import type { TrainingModelDownloadProgress, TrainingModelHit } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";

const QWEN_TRAINING = {
	modelId: "Qwen/Qwen3.5-0.8B",
	id: "qwen-3.5-0.8b-training",
	title: "Qwen 3.5 0.8B (MLX training)",
	description: "Base weights for Optimizers local SFT and CISPO through mlx-rl.",
	parameters: "0.8B",
	downloadSize: "1.75 GB",
	framework: "mlx-rl"
};

function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
	const gib = bytes / 1024 ** 3;
	return gib >= 1 ? `${gib.toFixed(2)} GB` : `${(bytes / 1024 ** 2).toFixed(0)} MB`;
}

export function TrainingModelsSettings() {
	const [hits, setHits] = useState<TrainingModelHit[]>([]);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [downloadProgress, setDownloadProgress] = useState<TrainingModelDownloadProgress | null>(null);

	const refresh = useCallback(async () => {
		setBusy(true);
		setError(null);
		try {
			setHits((await bridges.trainingModels?.listModels()) ?? []);
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}, []);

	useEffect(() => {
		void refresh();
	}, [refresh]);

	useEffect(
		() => bridges.trainingModels?.onDownloadProgress(setDownloadProgress),
		[]
	);

	const installed = hits.find((hit) => hit.modelId === QWEN_TRAINING.modelId) ?? null;
	const modelsRoot = installed?.modelsRoot ?? "~/.synth-desktop/models/training";
	const progress = downloadProgress?.modelId === QWEN_TRAINING.modelId ? downloadProgress : null;
	const pct = progress?.totalBytes && progress.downloadedBytes != null
		? Math.min(100, Math.round(progress.downloadedBytes / progress.totalBytes * 100))
		: null;

	const download = async () => {
		setBusy(true);
		setError(null);
		try {
			await bridges.trainingModels?.downloadModel(QWEN_TRAINING.modelId);
			setHits((await bridges.trainingModels?.listModels()) ?? []);
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	const deleteModel = async () => {
		if (!window.confirm(`Delete ${QWEN_TRAINING.title} and its managed training weights from this Mac?`)) return;
		setBusy(true);
		setError(null);
		try {
			await bridges.trainingModels?.deleteModel(QWEN_TRAINING.modelId);
			setHits((await bridges.trainingModels?.listModels()) ?? []);
			setDownloadProgress(null);
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	return (
		<div className="on-device-models" data-testid="training-model-locations">
			<div className="on-device-toolbar">
				<code className="on-device-path">Training weights downloaded to {modelsRoot}</code>
				<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void refresh()}>
					Search again
				</button>
			</div>

			{error ? <p className="model-locations-error" role="alert">{error}</p> : null}

			<section className="model-download-guide" aria-labelledby="training-model-download-guide-title">
				<div>
					<strong id="training-model-download-guide-title">Managed training download</strong>
					<p>Workshop verifies and stores these weights for local Optimizers recipes. They are not used by chat or the Laguna policy daemon.</p>
				</div>
				<ul>
					<li>Qwen 3.5 0.8B: 1.75 GB download · Apple Silicon MLX training</li>
				</ul>
				<p className="model-download-guide-status">Live progress appears below. Interrupted downloads resume when you choose Download again.</p>
			</section>

			<div className="on-device-grid" data-testid="on-device-training-catalog">
				<article className={`on-device-card${installed ? " installed" : ""}`} data-testid={`on-device-${QWEN_TRAINING.id}`}>
					<header className="on-device-card-top">
						<div className="on-device-card-identity">
							<span className="on-device-card-mark" aria-hidden>Q</span>
							<div>
								<div className="on-device-card-title-row">
									<strong>{QWEN_TRAINING.title}</strong>
									{installed ? <span className="model-location-badge">Installed</span> : null}
								</div>
								<p>{QWEN_TRAINING.description}</p>
							</div>
						</div>
						{installed ? (
							<button type="button" className="on-device-delete" disabled={busy} onClick={() => void deleteModel()}>
								Delete
							</button>
						) : (
							<button
								type="button"
								className="on-device-download"
								disabled={busy}
								onClick={() => void download()}
								data-testid={`download-${QWEN_TRAINING.id}`}
							>
								{busy ? "Downloading…" : "Download"}
							</button>
						)}
					</header>

					<dl className="on-device-specs">
						<div><dt>Provider</dt><dd>Qwen</dd></div>
						<div><dt>Framework</dt><dd>{QWEN_TRAINING.framework}</dd></div>
						<div><dt>Purpose</dt><dd>SFT / CISPO</dd></div>
						<div><dt>Parameters</dt><dd>{QWEN_TRAINING.parameters}</dd></div>
						<div><dt>Inference</dt><dd>Not used</dd></div>
						<div><dt>Download size</dt><dd>{installed ? formatBytes(installed.totalBytes) : QWEN_TRAINING.downloadSize}</dd></div>
					</dl>

					{installed ? (
						<p className="on-device-installed">
							Installed · {installed.shardCount} weight shard{installed.shardCount === 1 ? "" : "s"} · <code>{installed.path}</code>
						</p>
					) : (
						<p className="on-device-installed muted">
							{busy ? (progress?.detail ?? `Preparing ${QWEN_TRAINING.title}…`) : "Not installed yet."}
						</p>
					)}
					{busy ? (
						<div className="model-managed-progress" data-testid={`progress-${QWEN_TRAINING.id}`}>
							<div
								className={`progress-track${pct == null ? " is-indeterminate" : ""}`}
								role="progressbar"
								aria-valuenow={pct ?? undefined}
								aria-valuemin={0}
								aria-valuemax={100}
								aria-label={progress?.detail ?? `Preparing ${QWEN_TRAINING.title}`}
							>
								<div className={`progress-fill${pct == null ? " progress-indeterminate" : ""}`} style={pct == null ? undefined : { width: `${pct}%` }} />
							</div>
							<span>
								{progress?.detail ?? `Preparing ${QWEN_TRAINING.title}…`}
								{pct != null ? ` · ${formatBytes(progress?.downloadedBytes ?? 0)} of ${formatBytes(progress?.totalBytes ?? 0)} · ${pct}%` : ""}
							</span>
						</div>
					) : null}
				</article>
			</div>
		</div>
	);
}
