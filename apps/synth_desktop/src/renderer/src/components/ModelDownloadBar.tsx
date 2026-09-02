import type { LandingState } from "../types/landing";

type Props = {
	state: LandingState;
	onPauseToggle: () => void;
};

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

export function ModelDownloadBar({ state, onPauseToggle }: Props) {
	const { model } = state;

	// Local-runtime health is useful while the local target is selected, but it
	// must not become a global application warning during a Synth Cloud or
	// remote session. Keep an explicit download visible because that is a
	// user-initiated background operation with progress worth preserving.
	if (state.selectedTargetId !== "local-laguna" && model.status !== "downloading") {
		return null;
	}

	if (model.status === "not_installed") {
		return (
			<div className="download-bar is-warning" data-testid="model-status-missing">
				<div className="download-label">
					<span>{model.detail || `${model.name} not connected`}</span>
				</div>
			</div>
		);
	}

	if (model.status === "error") {
		return (
			<div className="download-bar is-error" data-testid="model-status-error">
				<div className="download-label">
					<span>{model.detail || `${model.name} failed to start`}</span>
				</div>
			</div>
		);
	}

	if (model.status === "ready") {
		// The residency card immediately above owns the ready/resident state.
		return null;
	}

	if (model.status === "starting" || model.status === "loading") {
		const label =
			model.detail ||
			(model.status === "starting"
				? `Starting ${model.name}…`
				: `Loading ${model.name}…`);
		return (
			<div
				className={`download-bar is-${model.status}`}
				data-testid={`model-status-${model.status}`}
			>
				<div className="progress-track is-indeterminate" role="progressbar" aria-busy="true">
					<div className="progress-fill progress-indeterminate" />
				</div>
				<div className="download-label">
					<span>{label}</span>
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
					Downloading {model.name}
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
