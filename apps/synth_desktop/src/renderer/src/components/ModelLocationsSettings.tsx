import { useCallback, useEffect, useState } from "react";
import type { LagunaModelHit } from "../bridge";

function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
	const gib = bytes / 1024 ** 3;
	return gib >= 1 ? `${gib.toFixed(1)} GB` : `${(bytes / 1024 ** 2).toFixed(0)} MB`;
}

export function ModelLocationsSettings() {
	const [hits, setHits] = useState<LagunaModelHit[]>([]);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const refresh = useCallback(async () => {
		setBusy(true);
		setError(null);
		try {
			setHits(await window.synthLaguna?.listModels() ?? []);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	}, []);

	useEffect(() => { void refresh(); }, [refresh]);

	const choose = useCallback(async () => {
		setBusy(true);
		setError(null);
		try {
			const path = await window.synthLaguna?.chooseModelDirectory();
			if (!path) return;
			await window.synthLaguna?.setModelDirectory(path);
			setHits(await window.synthLaguna?.listModels() ?? []);
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
			setHits(await window.synthLaguna?.listModels() ?? []);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	}, []);

	const selected = hits.find((hit) => hit.selected);
	return (
		<section className="model-locations" data-testid="laguna-model-locations">
			<div className="model-locations-head">
				<div>
					<h3>Local model files</h3>
					<p>Synth checks Poolside, Synth, and Hugging Face locations. Choose a folder if your model lives elsewhere.</p>
				</div>
				<div className="model-locations-actions">
					<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void refresh()}>Search again</button>
					<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void choose()}>Choose folder…</button>
				</div>
			</div>
			{error ? <p className="model-locations-error" role="alert">{error}</p> : null}
			{hits.length ? (
				<div className="model-locations-list">
					{hits.map((hit) => (
						<div className={`model-location-row${hit.selected ? " selected" : ""}`} key={hit.path}>
							<div className="model-location-copy">
								<strong>{hit.selected ? "Selected Laguna XS" : "Laguna XS found"}</strong>
								<code>{hit.path}</code>
								<span>{hit.shardCount} weight shards · {formatBytes(hit.totalBytes)}</span>
							</div>
							{hit.selected ? <span className="model-location-badge">In use</span> : (
								<button type="button" className="settings-secondary-btn" disabled={busy} onClick={async () => {
									setBusy(true); setError(null);
									try { await window.synthLaguna?.setModelDirectory(hit.path); await refresh(); }
									catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); setBusy(false); }
								}}>Use this copy</button>
							)}
						</div>
					))}
				</div>
			) : <p className="model-locations-empty">{busy ? "Searching this Mac…" : "No complete Laguna XS model was found."}</p>}
			{selected ? <button type="button" className="model-location-clear" disabled={busy} onClick={() => void clear()}>Use automatic discovery</button> : null}
			<p className="model-locations-note">A newly selected location is used the next time Laguna starts.</p>
		</section>
	);
}
