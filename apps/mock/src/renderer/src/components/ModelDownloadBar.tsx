import { useEffect, useRef, useState } from "react";
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
	const [animatedProgress, setAnimatedProgress] = useState(model.downloadProgress ?? 0);
	const intervalRef = useRef<number | null>(null);

	useEffect(() => {
		if (model.status !== "downloading") return;

		setAnimatedProgress(model.downloadProgress ?? 0);

		if (model.downloadPaused) {
			if (intervalRef.current) window.clearInterval(intervalRef.current);
			return;
		}

		intervalRef.current = window.setInterval(() => {
			setAnimatedProgress((value) => {
				if (value >= 98) return value;
				return value + 0.35;
			});
		}, 200);

		return () => {
			if (intervalRef.current) window.clearInterval(intervalRef.current);
		};
	}, [model.downloadPaused, model.downloadProgress, model.status]);

	if (model.status === "not_installed") return null;

	if (model.status === "ready") {
		return (
			<div className="download-bar" data-testid="model-status-ready">
				<div className="download-label">
					<span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
						<span className="ready-dot" aria-hidden />
						{model.name} ready
					</span>
				</div>
			</div>
		);
	}

	const pct = Math.round(animatedProgress);

	return (
		<div className="download-bar" data-testid="model-download-bar">
			<div
				className="progress-track"
				role="progressbar"
				aria-valuenow={pct}
				aria-valuemin={0}
				aria-valuemax={100}
			>
				<div className="progress-fill" style={{ width: `${animatedProgress}%` }} />
			</div>
			<div className="download-label">
				<span>
					Downloading {model.name}
					{animatedProgress > 0 ? ` · ${pct}%` : ""}
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
