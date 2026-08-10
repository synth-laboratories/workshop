import type { LandingState } from "../types/landing";

type Props = {
	state: LandingState;
	onPauseToggle: () => void;
};

function displayModelName(value: string): string {
	const name = value.split("/").at(-1)?.replace(/-(?:mlx|gguf)$/i, "") ?? value;
	if (/^muse-glimmer-30b$/i.test(name)) return "Muse Glimmer 30B";
	if (/^laguna-xs-2\.1-nvfp4$/i.test(name)) return "Laguna XS 2.1 NVFP4";
	return name.replace(/[-_]+/g, " ");
}

function IconPause() {
	return (
		<svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden>
			<rect x="2.5" y="2" width="2.2" height="8" rx="0.4" fill="currentColor" />
			<rect x="7.3" y="2" width="2.2" height="8" rx="0.4" fill="currentColor" />
		</svg>
	);
}

function IconPlay() {
	return (
		<svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden>
			<path d="M3.6 2.1v7.8L10.2 6 3.6 2.1z" fill="currentColor" />
		</svg>
	);
}

function IconModel({ spinning = false }: { spinning?: boolean }) {
	return (
		<svg className={`model-status-icon${spinning ? " is-spinning" : ""}`} viewBox="0 0 20 20" fill="none" aria-hidden>
			<path d="M6.2 4.5h8.3v10.2H6.2z" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" />
			<path d="M3.2 6.5H6m-2.8 3H6m-2.8 3H6M9 2v2.5m3 0V2m-3 12.7V18m3-3.3V18" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" />
			<path d="M9 7.4h2.8v3H9z" stroke="currentColor" strokeWidth="1.15" />
		</svg>
	);
}

export function ModelDownloadBar({ state, onPauseToggle }: Props) {
	const { model } = state;
	const modelName = displayModelName(model.name);

	if (model.status === "not_installed") {
		const detail = model.detail || `${modelName} not connected`;
		return (
			<div className="download-bar is-warning" data-testid="model-status-missing">
				<div className="download-label">
					{/* The title carries the whole message; the row clamps to
					    three lines rather than ellipsing away the fix. */}
					<span title={detail}>{detail}</span>
				</div>
			</div>
		);
	}

	if (model.status === "error") {
		const detail = model.detail || `${modelName} failed to start`;
		return (
			<div className="download-bar is-error" data-testid="model-status-error">
				<div className="download-label">
					<span title={detail}>{detail}</span>
				</div>
			</div>
		);
	}

	if (model.status === "ready") {
		return (
			<div className="download-bar model-status-row" data-testid="model-status-ready">
				<div className="download-label">
					<IconModel />
					<span>{modelName} ready</span>
				</div>
			</div>
		);
	}

	if (model.status === "unloaded") {
		return (
			<div className="download-bar model-status-row is-unloaded" data-testid="model-status-unloaded">
				<div className="download-label">
					<IconModel />
					<span title={model.detail}>{modelName} · Memory free</span>
				</div>
			</div>
		);
	}

	if (model.status === "starting" || model.status === "loading") {
		return (
			<div
				className={`download-bar model-status-row is-${model.status}`}
				data-testid={`model-status-${model.status}`}
				aria-label={model.detail || `Loading ${modelName}`}
			>
				<div className="download-label" role="status" aria-live="polite" aria-busy="true">
					<IconModel spinning />
					<span>Loading model…</span>
				</div>
			</div>
		);
	}

	if (model.status !== "downloading") return null;

	const pct = Math.round(model.downloadProgress ?? 0);

	return (
		<div className="download-bar" data-testid="model-download-bar">
			<div
				className="progress-track"
				role="progressbar"
				aria-valuenow={pct}
				aria-valuemin={0}
				aria-valuemax={100}
			>
				<div className="progress-fill" style={{ width: `${pct}%` }} />
			</div>
			<div className="download-label">
				<span>
					Downloading {modelName}
					{pct > 0 ? ` · ${pct}%` : ""}
				</span>
				<button
					type="button"
					className="pause-btn"
					onClick={onPauseToggle}
					aria-label={model.downloadPaused ? "Resume download" : "Pause download"}
				>
					{model.downloadPaused ? <IconPlay /> : <IconPause />}
				</button>
			</div>
		</div>
	);
}
