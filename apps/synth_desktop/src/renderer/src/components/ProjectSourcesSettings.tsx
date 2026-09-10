import { useCallback, useEffect, useRef, useState } from "react";
import type { ProjectSourceCatalog, ProjectSourceRequest, ProjectSourceRow } from "../generated/protocol";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";
import "./ProjectSourcesSettings.css";

function SourceRow({ row, busy, remove }: { row: ProjectSourceRow; busy: boolean; remove?: (path: string) => void }) {
	const counts = [row.containers ? `${row.inspection.containers.length} container(s)` : null, row.recipes ? `${row.inspection.recipes.length} recipe(s)` : null].filter(Boolean).join(" · ");
	return <div className="project-source-row" data-testid="project-source-row">
		<div><code>{row.path}</code><p>
			{row.containers ? "Containers " : ""}{row.recipes ? "Recipes " : ""}
			· {row.origin === "configured" ? "Approved" : "Launcher environment"}
		</p><p>{row.inspection.status === "valid"
			? `Last successful scan: ${counts}`
			: row.inspection.message ?? row.inspection.status}</p></div>
		{remove ? <button type="button" className="settings-secondary-btn" disabled={busy} aria-label={`Remove project source ${row.path}`} onClick={() => remove(row.path)}>Remove</button> : null}
	</div>;
}

export function ProjectSourcesSettings() {
	const [catalog, setCatalog] = useState<ProjectSourceCatalog | null>(null);
	const [requests, setRequests] = useState<ProjectSourceRequest[] | null>(null);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [notice, setNotice] = useState<string | null>(null);
	const [containers, setContainers] = useState(true);
	const [recipes, setRecipes] = useState(true);
	const active = useRef(false);
	const mounted = useRef(true);
	const bridge = () => {
		if (!bridges.projectSources) throw new Error("Project source controls are unavailable");
		return bridges.projectSources;
	};
	const reload = useCallback(async () => {
		const [next, pending] = await Promise.all([bridge().refresh(), bridge().requests()]);
		if (mounted.current) { setCatalog(next); setRequests(pending.filter((request) => request.status === "pending")); }
	}, []);
	const run = useCallback(async (action: () => Promise<void>, quiet = false) => {
		if (active.current) return;
		active.current = true;
		if (mounted.current) { setBusy(true); if (!quiet) { setError(null); setNotice(null); } }
		try { await action(); }
		catch (reason) { if (mounted.current) setError(publicError(reason)); }
		finally { active.current = false; if (mounted.current) setBusy(false); }
	}, []);
	useEffect(() => {
		mounted.current = true;
		void run(reload);
		const timer = window.setInterval(() => void run(reload, true), 5000);
		return () => { mounted.current = false; window.clearInterval(timer); };
	}, [reload, run]);
	const add = () => run(async () => {
		const next = await bridge().add(containers, recipes);
		if (next && mounted.current) { setCatalog(next); setNotice("Project source approved."); }
	});
	const remove = (path: string) => void run(async () => {
		try { const next = await bridge().remove(path); if (mounted.current) setCatalog(next); }
		catch (reason) { await reload().catch(() => undefined); throw reason; } // Revocation may have succeeded before an audit failure.
	});
	const approve = (id: string) => void run(async () => {
		try {
			const result = await bridge().approve(id);
			if (!result) return; // Native picker cancellation grants nothing.
			if (mounted.current) {
				setCatalog(result.catalog);
				setNotice(result.attachmentError ?? "Project source approved.");
			}
			await reload();
		} catch (reason) { await reload().catch(() => undefined); throw reason; }
	});
	const deny = (id: string) => void run(async () => { await bridge().deny(id); await reload(); });
	return <section className="project-sources" data-testid="project-sources-settings" aria-busy={busy}>
		<h3>Project sources</h3>
		<p>Approve folders containing container and optimizer recipe declarations. These grants are separate from conversation file access; execution approvals still apply.</p>
		<div className="project-source-actions">
			<label><input type="checkbox" checked={containers} disabled={busy} onChange={(event) => setContainers(event.target.checked)} /> Containers</label>
			<label><input type="checkbox" checked={recipes} disabled={busy} onChange={(event) => setRecipes(event.target.checked)} /> Recipes</label>
			<button type="button" className="settings-secondary-btn" disabled={busy || (!containers && !recipes)} onClick={() => void add()} data-testid="add-project-source">Add project source…</button>
			<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void run(reload)} data-testid="rescan-project-sources">Rescan</button>
		</div>
		{error ? <p role="alert">{error}</p> : null}
		{notice ? <p role="status">{notice}</p> : null}
		{catalog?.sources.map((row) => <SourceRow key={row.path} row={row} busy={busy} remove={remove} />)}
		{catalog && !catalog.sources.length ? <p>No approved project source. Add a repository folder or review an agent request below.</p> : null}
		{catalog?.implicitRoots.length ? <div><h4>Launcher-managed sources</h4><p>These permissions come from the launcher environment. Removing an approved row above does not remove an environment grant.</p>
			{catalog.implicitRoots.map((row) => <SourceRow key={row.path} row={row} busy={busy} />)}</div> : null}
		<h4>Pending source requests</h4>
		{requests === null ? <p>{busy ? "Loading source requests…" : "Source requests are unavailable."}</p> : !requests.length ? <p>No pending source requests.</p> : requests.map((request) => <div className="project-source-request" key={request.id} data-testid="project-source-request">
			<code>{request.canonicalPath}</code><p>{request.reason}</p>
			<p>Requested: {request.containers ? "containers " : ""}{request.recipes ? "recipes" : ""}.
				{request.attachToConversation ? ` Also attach with read/write access to conversation ${request.sessionId}.` : " No conversation file access requested."}</p>
			<p>Approval requires selecting this exact folder, not its parent.</p>
			<div className="project-source-actions"><button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => approve(request.id)}>Choose exact folder and approve…</button><button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => deny(request.id)}>Deny</button></div>
		</div>)}
		{catalog?.configPath ? <small><code>{catalog.configPath}</code></small> : null}
	</section>;
}
