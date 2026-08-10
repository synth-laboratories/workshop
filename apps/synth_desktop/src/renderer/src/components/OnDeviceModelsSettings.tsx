import { useCallback, useEffect, useState } from "react";
import type { LagunaModelHit, LagunaStatus } from "../env";
import { ProviderMark } from "./ProviderMark";

function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
	const gib = bytes / 1024 ** 3;
	return gib >= 1 ? `${gib.toFixed(1)} GB` : `${(bytes / 1024 ** 2).toFixed(0)} MB`;
}

const LAGUNA_XS = {
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

	const installed = hits.find((hit) => hit.selected) ?? hits[0] ?? null;
	const modelsRoot = installed?.modelsRoot ?? hits[0]?.modelsRoot ?? "~/.synth/models";
	const alternates = hits.filter((hit) => hit.path !== installed?.path);

	const download = useCallback(async () => {
		setBusy(true);
		setError(null);
		try {
			await window.synthLaguna?.downloadModel();
			setHits((await window.synthLaguna?.listModels()) ?? []);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	}, []);

	const clear = useCallback(async () => {
		setBusy(true);
		setError(null);
		try {
			await window.synthLaguna?.clearModelDirectory();
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

			<div className="on-device-grid" data-testid="on-device-recommended">
				<article className={`on-device-card${installed ? " installed" : ""}`} data-testid="on-device-laguna-xs">
					<header className="on-device-card-top">
						<div className="on-device-card-identity">
							<span className="on-device-card-mark" aria-hidden>
								<ProviderMark kind="laguna" className="on-device-card-mark-img" />
							</span>
							<div>
								<div className="on-device-card-title-row">
									<strong>{LAGUNA_XS.title}</strong>
									<span className="on-device-fit">Fit: {LAGUNA_XS.fit}</span>
									{installed ? <span className="model-location-badge">In use</span> : null}
								</div>
								<p>{LAGUNA_XS.description}</p>
							</div>
						</div>
						{installed ? (
							<button
								type="button"
								className="on-device-delete"
								disabled={busy}
								onClick={() => void clear()}
								data-testid="clear-laguna-model"
							>
								Delete
							</button>
						) : (
							<button
								type="button"
								className="on-device-download"
								disabled={busy}
								onClick={() => void download()}
								data-testid="download-laguna-model"
							>
								{busy ? "Downloading…" : "Download"}
							</button>
						)}
					</header>

					<dl className="on-device-specs">
						<div>
							<dt>Provider</dt>
							<dd>{LAGUNA_XS.provider}</dd>
						</div>
						<div>
							<dt>Quantization</dt>
							<dd>{LAGUNA_XS.quantization}</dd>
						</div>
						<div>
							<dt>Context</dt>
							<dd>{LAGUNA_XS.context}</dd>
						</div>
						<div>
							<dt>Parameters</dt>
							<dd>{LAGUNA_XS.parameters}</dd>
						</div>
						<div>
							<dt>Est. memory</dt>
							<dd>{LAGUNA_XS.estMemory}</dd>
						</div>
						<div>
							<dt>Download size</dt>
							<dd>{installed ? formatBytes(installed.totalBytes) : LAGUNA_XS.downloadSize}</dd>
						</div>
					</dl>

					{installed ? (
						<p className="on-device-installed">
							Installed · {installed.shardCount} weight shards · <code>{installed.path}</code>
						</p>
					) : (
						<p className="on-device-installed muted">
							{busy
								? "Downloading and verifying Laguna XS…"
								: "Not installed yet. Download the weights or choose an existing folder."}
						</p>
					)}
				</article>
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

			<p className="model-locations-note">A newly selected location is used the next time Laguna starts.</p>
		</div>
	);
}
