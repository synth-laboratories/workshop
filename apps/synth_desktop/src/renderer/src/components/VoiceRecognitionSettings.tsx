import { useCallback, useEffect, useMemo, useState } from "react";
import type { WhisperDownloadProgress, WhisperModelHit } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";

function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
	const gib = bytes / 1024 ** 3;
	return gib >= 1 ? `${gib.toFixed(1)} GB` : `${(bytes / 1024 ** 2).toFixed(0)} MB`;
}

const DEFAULT_MODELS_ROOT = "~/.synth/whisper-models";

/** Static fallback catalog shown until `bridges.whisper.listModels()` reports real hits. */
const STATIC_CATALOG: WhisperModelHit[] = [
	{
		id: "tiny",
		title: "Whisper Tiny",
		description: "Fastest option, lowest accuracy. Good for quick drafts and short notes.",
		recommended: false,
		multilingual: true,
		downloadBytes: 75 * 1024 ** 2,
		selected: false,
		modelsRoot: DEFAULT_MODELS_ROOT
	},
	{
		id: "base",
		title: "Whisper Base",
		description: "Balanced speed and accuracy for everyday dictation.",
		recommended: true,
		multilingual: true,
		downloadBytes: 142 * 1024 ** 2,
		selected: false,
		modelsRoot: DEFAULT_MODELS_ROOT
	},
	{
		id: "small",
		title: "Whisper Small",
		description: "Higher accuracy for longer or noisier recordings.",
		recommended: false,
		multilingual: true,
		downloadBytes: 466 * 1024 ** 2,
		selected: false,
		modelsRoot: DEFAULT_MODELS_ROOT
	},
	{
		id: "large-v3-turbo",
		title: "Whisper Large v3 Turbo",
		description: "Best accuracy, optimized for near real-time transcription.",
		recommended: false,
		multilingual: true,
		downloadBytes: 1620 * 1024 ** 2,
		selected: false,
		modelsRoot: DEFAULT_MODELS_ROOT
	}
];

export function VoiceRecognitionSettings() {
	const [hits, setHits] = useState<WhisperModelHit[]>([]);
	const [busyId, setBusyId] = useState<string | null>(null);
	const [downloadProgress, setDownloadProgress] = useState<WhisperDownloadProgress | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [loaded, setLoaded] = useState(false);

	const refresh = useCallback(async () => {
		setError(null);
		try {
			const listed = (await bridges.whisper?.listModels()) ?? [];
			setHits(listed);
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setLoaded(true);
		}
	}, []);

	useEffect(() => {
		void refresh();
	}, [refresh]);

	useEffect(() => bridges.whisper?.onDownloadProgress?.((progress) => {
		setDownloadProgress(progress);
	}) ?? undefined, []);

	const models = hits.length ? hits : STATIC_CATALOG;
	const modelsRoot = hits[0]?.modelsRoot ?? DEFAULT_MODELS_ROOT;
	const usingFallback = hits.length === 0;

	const download = useCallback(
		async (id: string) => {
			setBusyId(id);
			setDownloadProgress({ id, phase: "preparing", detail: "Preparing the private Whisper runtime…" });
			setError(null);
			try {
				await bridges.whisper?.downloadModel(id);
				await refresh();
			} catch (reason) {
				setError(publicError(reason));
			} finally {
				setBusyId(null);
				setDownloadProgress(null);
			}
		},
		[refresh]
	);

	const clear = useCallback(
		async (id: string) => {
			setBusyId(id);
			setError(null);
			try {
				await bridges.whisper?.clearModel(id);
				await refresh();
			} catch (reason) {
				setError(publicError(reason));
			} finally {
				setBusyId(null);
			}
		},
		[refresh]
	);

	const select = useCallback(
		async (id: string) => {
			setBusyId(id);
			setError(null);
			try {
				await bridges.whisper?.setSelected(id);
				await refresh();
			} catch (reason) {
				setError(publicError(reason));
			} finally {
				setBusyId(null);
			}
		},
		[refresh]
	);

	const isInstalled = useMemo(
		() => (hit: WhisperModelHit) => !usingFallback && (Boolean(hit.path) || Boolean(hit.installedBytes)),
		[usingFallback]
	);

	return (
		<div className="on-device-models voice-recognition-settings" data-testid="voice-recognition-settings">
			<div className="on-device-toolbar">
				<code className="on-device-path">Models downloaded to {modelsRoot}</code>
				<div className="on-device-toolbar-actions">
					<button
						type="button"
						className="settings-secondary-btn"
						disabled={busyId !== null}
						onClick={() => void refresh()}
					>
						Search again
					</button>
				</div>
			</div>

			{error ? (
				<p className="model-locations-error" role="alert">
					{error}
				</p>
			) : null}

			{!loaded ? <p className="model-locations-empty">Checking installed Whisper models…</p> : null}

			<div className="on-device-grid" data-testid="voice-recognition-catalog">
				{models.map((model) => {
					const installed = isInstalled(model);
					const busy = busyId === model.id;
					const progress = busy && downloadProgress?.id === model.id ? downloadProgress : null;
					const totalBytes = progress?.totalBytes ?? model.downloadBytes;
					const downloadedBytes = progress?.downloadedBytes ?? 0;
					const percent = progress?.phase === "downloading" && totalBytes > 0
						? Math.min(99, Math.round((downloadedBytes / totalBytes) * 100))
						: progress?.phase === "ready" ? 100 : null;
					return (
						<article
							key={model.id}
							className={`on-device-card${installed ? " installed" : ""}`}
							data-testid={`whisper-model-${model.id}`}
						>
							<header className="on-device-card-top">
								<div className="on-device-card-identity">
									<span className="voice-model-icon" aria-hidden>
										<svg viewBox="0 0 24 24"><path d="M12 15.5a4 4 0 0 0 4-4V6a4 4 0 1 0-8 0v5.5a4 4 0 0 0 4 4Zm-7-4a7 7 0 0 0 14 0M12 18.5V22m-4 0h8" /></svg>
									</span>
									<div>
										<div className="on-device-card-title-row">
											<strong>{model.title}</strong>
											{model.recommended ? <span className="on-device-fit">Recommended</span> : null}
											{model.multilingual ? <span className="voice-multilingual-tag">Multilingual</span> : null}
											<span className="voice-model-size">{formatBytes(model.installedBytes ?? model.downloadBytes)}</span>
											{model.selected ? <span className="model-location-badge">In use</span> : null}
										</div>
										{model.description ? <p>{model.description}</p> : null}
									</div>
								</div>
								{installed ? (
									<button
										type="button"
										className="on-device-delete"
										disabled={busy}
										onClick={() => void clear(model.id)}
										data-testid={`delete-whisper-${model.id}`}
									>
										Delete
									</button>
								) : (
									<button
										type="button"
										className="on-device-download"
										disabled={busy}
										onClick={() => void download(model.id)}
										data-testid={`download-whisper-${model.id}`}
									>
										{busy ? "Downloading…" : "Download"}
									</button>
								)}
							</header>

							<dl className="on-device-specs voice-model-specs">
								<div>
									<dt>Size</dt>
									<dd>{formatBytes(model.installedBytes ?? model.downloadBytes)}</dd>
								</div>
								<div>
									<dt>Languages</dt>
									<dd>{model.multilingual ? "Multilingual" : "English only"}</dd>
								</div>
							</dl>

							{installed ? (
								model.selected ? (
									<p className="on-device-installed">
										Installed and selected · <code>{model.path}</code>
									</p>
								) : (
									<button
										type="button"
										className="settings-secondary-btn voice-use-model"
										disabled={busy}
										onClick={() => void select(model.id)}
									>
										Use this model
									</button>
								)
							) : (
								busy && progress ? (
									<div className="voice-download-progress" aria-live="polite">
										<div
											className={`progress-track${percent === null ? " is-indeterminate" : ""}`}
											role="progressbar"
											aria-label={progress.detail}
											aria-valuenow={percent ?? undefined}
										>
											<div className={`progress-fill${percent === null ? " progress-indeterminate" : ""}`} style={percent === null ? undefined : { width: `${percent}%` }} />
										</div>
										<div className="voice-progress-label">
											<span>{progress.phase === "preparing" ? "Preparing" : "Downloading"}</span>
											<span>{percent !== null ? `${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)} · ${percent}%` : progress.detail}</span>
										</div>
									</div>
								) : (
									<p className="on-device-installed muted">Not installed yet.</p>
								)
							)}
						</article>
					);
				})}
			</div>

		</div>
	);
}
