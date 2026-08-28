import { useCallback, useEffect, useState } from "react";
import type { ProjectSourceCatalog, ProjectSourceRow } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";

const ORIGIN_LABEL: Record<ProjectSourceRow["origin"], string> = {
	configured: "Approved",
	environment: "Launcher override",
	remembered: "Previously discovered",
	development_fallback: "Development fallback"
};

function statusLabel(row: ProjectSourceRow): string {
	if (row.inspection.status === "valid") {
		const parts: string[] = [];
		if (row.containers) parts.push(`${row.inspection.containers.length} container${row.inspection.containers.length === 1 ? "" : "s"}`);
		if (row.recipes) parts.push(`${row.inspection.recipes.length} recipe${row.inspection.recipes.length === 1 ? "" : "s"}`);
		return parts.join(" · ") || "No declarations";
	}
	return row.inspection.message ?? row.inspection.status;
}

function SourceRow({ row, busy, onRemove }: { row: ProjectSourceRow; busy: boolean; onRemove?: (path: string) => void }) {
	const invalid = row.inspection.status !== "valid";
	return (
		<div className={`project-source-row${invalid ? " project-source-row-invalid" : ""}`} data-testid="project-source-row">
			<div className="project-source-identity">
				<code title={row.path}>{row.path}</code>
				<span className="project-source-capabilities">
					{row.containers ? <b>Containers</b> : null}
					{row.recipes ? <b>Recipes</b> : null}
					<em>{ORIGIN_LABEL[row.origin]}</em>
				</span>
			</div>
			<div className="project-source-state">
				<span className={invalid ? "project-source-invalid" : undefined} role={invalid ? "status" : undefined}>{statusLabel(row)}</span>
				{row.lastScannedAt ? <small>Last scan {row.lastScannedAt}</small> : <small>Not scanned yet</small>}
			</div>
			{onRemove ? (
				<button type="button" disabled={busy} aria-label={`Remove project source ${row.path}`} onClick={() => onRemove(row.path)}>Remove</button>
			) : null}
		</div>
	);
}

/**
 * Settings → Workspace → Project sources.
 *
 * The copy here has one job: keep project sources from reading as a second
 * name for workspace access. Attaching a folder to a conversation grants file
 * access. A project source is where Workshop is allowed to find container and
 * recipe declarations it may then be asked to run, which is why removal is
 * offered on every approved row and why invalid rows say what is wrong instead
 * of quietly disappearing from discovery.
 */
export function ProjectSourcesSettings() {
	const [catalog, setCatalog] = useState<ProjectSourceCatalog | null>(null);
	const [busy, setBusy] = useState(true);
	const [error, setError] = useState<string | null>(null);

	const load = useCallback(async (refresh: boolean) => {
		setBusy(true);
		setError(null);
		try {
			const next = refresh ? await bridges.projectSources?.refresh() : await bridges.projectSources?.get();
			if (next) setCatalog(next);
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}, []);

	useEffect(() => { void load(false); }, [load]);

	const add = useCallback(async () => {
		setBusy(true);
		setError(null);
		try {
			const next = await bridges.projectSources?.add(true, true);
			if (next) setCatalog(next);
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}, []);

	const remove = useCallback(async (path: string) => {
		setBusy(true);
		setError(null);
		try {
			const next = await bridges.projectSources?.remove(path);
			if (next) setCatalog(next);
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}, []);

	return (
		<section className="project-sources" data-testid="project-sources-settings">
			<header className="project-sources-head">
				<div>
					<h3>Project sources</h3>
					<p>
						Folders Workshop may read <code>workshop.containers.toml</code> and <code>workshop.recipe(s)</code> from.
						Container commands declared in these folders can be started, after the usual execution approvals.
						This is separate from agent workspace access, which only grants file access to a conversation.
					</p>
				</div>
				<div className="project-sources-actions">
					<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void load(true)} data-testid="rescan-project-sources">Rescan</button>
					<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void add()} data-testid="add-project-source">Add project source…</button>
				</div>
			</header>
			{error ? <p className="model-locations-error" role="alert">{error}</p> : null}
			{catalog?.sources.length ? (
				<div className="project-source-list">
					{catalog.sources.map((row) => <SourceRow key={row.path} row={row} busy={busy} onRemove={(path) => void remove(path)} />)}
				</div>
			) : <p className="project-sources-empty">No approved project source. An agent can ask for one, or add a repository folder here.</p>}
			{catalog?.implicitRoots.length ? (
				<div className="project-source-implicit">
					<small>Also in scope, not approved here</small>
					<p>These come from the launcher environment, earlier discoveries, or the development fallback. Approve one above to keep it across environments.</p>
					{catalog.implicitRoots.map((row) => <SourceRow key={row.path} row={row} busy={busy} />)}
				</div>
			) : null}
			{catalog?.configPath ? <footer className="project-sources-config"><code>{catalog.configPath}</code></footer> : null}
		</section>
	);
}
