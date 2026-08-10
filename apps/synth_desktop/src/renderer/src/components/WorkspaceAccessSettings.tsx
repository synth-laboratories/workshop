import { useCallback, useEffect, useState } from "react";

export function WorkspaceAccessSettings() {
	const [roots, setRoots] = useState<string[]>([]);
	const [savedRoots, setSavedRoots] = useState<string[]>([]);
	const [busy, setBusy] = useState(true);
	const [error, setError] = useState<string | null>(null);
	const [saved, setSaved] = useState(false);

	useEffect(() => {
		void window.synthConfig?.getWorkspaceAccess()
			.then((settings) => {
				setRoots(settings.allowedRoots);
				setSavedRoots(settings.allowedRoots);
			})
			.catch((reason) => setError(reason instanceof Error ? reason.message : String(reason)))
			.finally(() => setBusy(false));
	}, []);

	const choose = useCallback(async () => {
		setError(null);
		setSaved(false);
		try {
			const path = await window.synthDesktop.chooseWorkspaceDirectory();
			if (path) setRoots((current) => current.includes(path) ? current : [...current, path]);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		}
	}, []);

	const save = useCallback(async () => {
		setBusy(true);
		setError(null);
		setSaved(false);
		try {
			const settings = await window.synthConfig?.updateWorkspaceAccess({ allowedRoots: roots });
			if (settings) {
				setRoots(settings.allowedRoots);
				setSavedRoots(settings.allowedRoots);
				setSaved(true);
			}
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	}, [roots]);

	const dirty = roots.length !== savedRoots.length || roots.some((root, index) => root !== savedRoots[index]);

	return (
		<section className="workspace-access" data-testid="workspace-access-settings">
			<header className="workspace-access-head">
				<div>
					<h3>Agent workspace access</h3>
					<p>The first folder is the default working directory for new Codex conversations. Every listed folder is writable in <code>workspace-write</code> mode.</p>
				</div>
				<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void choose()} data-testid="add-workspace-root">Add folder…</button>
			</header>
			{error ? <p className="model-locations-error" role="alert">{error}</p> : null}
			{roots.length ? (
				<div className="workspace-access-list">
					{roots.map((root, index) => (
						<div className="workspace-access-row" key={root} data-testid={`workspace-root-${index}`}>
							<div className="workspace-access-path">
								<code>{root}</code>
								{index === 0 ? <span>Default start folder</span> : null}
							</div>
							<button type="button" disabled={busy} aria-label={`Remove ${root}`} onClick={() => {
								setRoots((current) => current.filter((candidate) => candidate !== root));
								setSaved(false);
							}}>Remove</button>
						</div>
					))}
				</div>
			) : <p className="workspace-access-empty">No configured root. New conversations use this app instance’s isolated workspace.</p>}
			<footer className="workspace-access-actions">
				<span>{saved ? "Saved. Applies when an agent session starts or restarts." : "Changes apply to newly started or restarted agent sessions."}</span>
				<button type="button" className="settings-secondary-btn" disabled={busy || !dirty} onClick={() => void save()} data-testid="save-workspace-roots">{busy ? "Saving…" : "Save access"}</button>
			</footer>
		</section>
	);
}
