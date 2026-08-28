import { useEffect, useRef, useState } from "react";
import type { ConversationWorkspaceScope, ProjectSourceRequest, WorkspaceAccessMode, WorkspaceGrantRequest } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";

function compactPath(path: string): string {
	const home = "/Users/";
	if (path.startsWith(home)) {
		const parts = path.split("/");
		return parts.length > 3 ? `~/${parts.slice(3).join("/")}` : "~";
	}
	return path;
}

/**
 * What a project source approval actually authorizes, in the order the effects
 * escalate. Execution is named explicitly: a project source is the only grant
 * that lets declared container commands from a folder be started at all, and
 * an approval card that hid that behind "add folder" would be asking the wrong
 * question.
 */
export function projectSourceEffects(request: {
	containers: boolean;
	recipes: boolean;
	attachToConversation: boolean;
}): string[] {
	const effects: string[] = [];
	if (request.attachToConversation) effects.push("Let this conversation read and write files in the folder.");
	if (request.containers && request.recipes) effects.push("Discover container and optimizer recipe declarations here.");
	else if (request.containers) effects.push("Discover container declarations here.");
	else if (request.recipes) effects.push("Discover optimizer recipe declarations here.");
	if (request.containers) effects.push("Allow container commands declared here to be started, after the normal execution approvals.");
	effects.push("Remember this source for this Workshop instance until you remove it in Settings.");
	return effects;
}

export function workspaceLabel(path: string): string {
	if (path.includes("/.synth-desktop/instances/") || path.includes("/Synth Desktop/instances/")) return "Isolated workspace";
	const clean = path.replace(/\/$/, "");
	return clean.slice(clean.lastIndexOf("/") + 1) || clean;
}

export function WorkspaceScopeChip({ sessionId, ensureSession, fallbackWorkspace, scope, onScopeChange, onError, hideTrigger = false, openSignal = 0 }: {
	sessionId: string | null;
	ensureSession?: () => Promise<string | null>;
	fallbackWorkspace: string | null;
	scope: ConversationWorkspaceScope | null;
	onScopeChange: (scope: ConversationWorkspaceScope) => void;
	onError: (message: string) => void;
	hideTrigger?: boolean;
	openSignal?: number;
}) {
	const [open, setOpen] = useState(false);
	const [busy, setBusy] = useState(false);
	const [grants, setGrants] = useState<WorkspaceGrantRequest[]>([]);
	const [sourceRequests, setSourceRequests] = useState<ProjectSourceRequest[]>([]);
	const [recentFolders, setRecentFolders] = useState<string[]>([]);
	const ref = useRef<HTMLDivElement>(null);
	const workspace = scope?.workspace ?? fallbackWorkspace;
	useEffect(() => { if (openSignal > 0) setOpen(true); }, [openSignal]);
	useEffect(() => {
		if (!open) return;
		const close = (event: MouseEvent) => { if (!ref.current?.contains(event.target as Node)) setOpen(false); };
		const escape = (event: KeyboardEvent) => { if (event.key === "Escape") setOpen(false); };
		document.addEventListener("mousedown", close);
		document.addEventListener("keydown", escape);
		return () => { document.removeEventListener("mousedown", close); document.removeEventListener("keydown", escape); };
	}, [open]);
	useEffect(() => { if (sessionId) void bridges.workspaceScope?.listGrants(sessionId).then(setGrants).catch(() => undefined); else setGrants([]); }, [sessionId, open]);
	useEffect(() => {
		if (!sessionId || !bridges.projectSources) { setSourceRequests([]); return; }
		let disposed = false;
		const refresh = () => void bridges.projectSources?.listRequests(sessionId).then((requests) => {
			if (!disposed) setSourceRequests(requests);
		}).catch(() => undefined);
		refresh();
		const timer = window.setInterval(refresh, 1500);
		return () => { disposed = true; window.clearInterval(timer); };
	}, [sessionId]);
	useEffect(() => {
		if (!open) return;
		void bridges.workspaceScope?.listRecentFolders().then(setRecentFolders).catch(() => setRecentFolders([]));
	}, [open]);

	const add = async (access: WorkspaceAccessMode) => {
		if (!bridges.workspaceScope) return;
		setBusy(true);
		try {
			const targetSessionId = sessionId ?? await ensureSession?.();
			if (!targetSessionId) return;
			const next = await bridges.workspaceScope.chooseAndAttach(targetSessionId, access);
			if (next) onScopeChange(next);
		} catch (reason) { onError(publicError(reason)); }
		finally { setBusy(false); }
	};
	const addRecent = async (path: string) => {
		if (!bridges.workspaceScope) return;
		setBusy(true);
		try {
			const targetSessionId = sessionId ?? await ensureSession?.();
			if (!targetSessionId) return;
			onScopeChange(await bridges.workspaceScope.attachRecent(targetSessionId, path));
		} catch (reason) { onError(publicError(reason)); }
		finally { setBusy(false); }
	};
	const remove = async (path: string) => {
		if (!sessionId || !bridges.workspaceScope) return;
		setBusy(true);
		try { onScopeChange(await bridges.workspaceScope.removeAttachment(sessionId, path)); }
		catch (reason) { onError(publicError(reason)); }
		finally { setBusy(false); }
	};
	const resolveSourceRequest = async (request: ProjectSourceRequest, approve: boolean) => {
		if (!bridges.projectSources) return;
		setBusy(true);
		try {
			if (approve) {
				const approval = await bridges.projectSources.approveRequest(request.id);
				// A request that also asked to attach the folder returns the new
				// conversation scope; one that only admitted a source does not.
				if (approval?.scope) onScopeChange(approval.scope);
			} else {
				await bridges.projectSources.denyRequest(request.id);
			}
			if (sessionId) setSourceRequests(await bridges.projectSources.listRequests(sessionId));
		} catch (reason) { onError(publicError(reason)); }
		finally { setBusy(false); }
	};
	const resolveGrant = async (request: WorkspaceGrantRequest, approve: boolean) => {
		if (!bridges.workspaceScope) return;
		setBusy(true);
		try {
			if (approve) { const next=await bridges.workspaceScope.approveRequest(request.id); if(next) onScopeChange(next); }
			else await bridges.workspaceScope.denyRequest(request.id);
			if(sessionId) setGrants(await bridges.workspaceScope.listGrants(sessionId));
		} catch(reason){onError(publicError(reason));} finally{setBusy(false);}
	};

	if (!workspace) return null;
	return <div className={`workspace-scope-wrap${hideTrigger ? " workspace-scope-slash-only" : ""}`} ref={ref}>
		{!hideTrigger ? <button type="button" className="workspace-scope-chip" onClick={() => setOpen((value) => !value)} aria-label={`Workspace: ${workspaceLabel(workspace)}${sourceRequests.some((request) => request.status === "pending") ? "; project source approval pending" : ""}`} aria-expanded={open} aria-controls="workspace-scope-menu" aria-haspopup="menu" data-testid="workspace-scope-chip" title={workspace}>
			<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden><path d="M2.25 4.25h4l1.2 1.4h6.3v6.1a1 1 0 01-1 1h-9.5a1 1 0 01-1-1v-7.5z" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" /></svg>
			<span>{workspaceLabel(workspace)}</span>{sourceRequests.some((request) => request.status === "pending") ? <span className="workspace-scope-pending-badge" aria-hidden>!</span> : null}<span aria-hidden>⌄</span>
		</button> : null}
		{open ? <div id="workspace-scope-menu" className="workspace-scope-menu" role="menu" aria-label="Workspace and attached folders" data-testid="workspace-scope-menu">
			<section aria-label="Working workspace"><small>Workspace</small><div className="workspace-scope-row"><code title={workspace}>{compactPath(workspace)}</code><b>Read/write</b></div></section>
			<section aria-label="Attached folders"><small>Attached folders</small>
				{scope?.attachments.length ? scope.attachments.map((item) => <div className="workspace-scope-row" key={item.path} data-testid="workspace-attachment">
					<span><code title={item.path}>{compactPath(item.path)}</code><b>{item.access === "read_write" ? "Read/write" : "Read-only"}</b></span>
					<button type="button" disabled={busy} onClick={() => void remove(item.path)} aria-label={`Remove attached folder ${item.path}`}>Remove</button>
				</div>) : <p>No additional folders</p>}
			</section>
			{scope?.bindingStatus === "pending" && scope.revision > scope.boundRevision ? <p className="workspace-scope-pending" role="status">The local agent will resume with this scope on the next message.</p> : null}
			{scope?.bindingStatus === "failed" ? <p className="workspace-scope-failed" role="alert">Access change failed. The previous scope remains active.</p> : null}
			{sourceRequests.filter((request) => request.status === "pending").map((request) => <section className="workspace-grant-card project-source-card" key={request.id} aria-label="Pending project source request" data-testid="project-source-pending">
				<small>Add project source</small>
				<strong>{workspaceLabel(request.canonicalPath)}</strong>
				<code title={request.canonicalPath}>{compactPath(request.canonicalPath)}</code>
				<p>{request.reason}</p>
				<p className="project-source-effects-title">Workshop will:</p>
				<ul className="project-source-effects">{projectSourceEffects(request).map((effect) => <li key={effect}>{effect}</li>)}</ul>
				<div>
					<button type="button" disabled={busy} onClick={() => void resolveSourceRequest(request, true)}>Approve…</button>
					<button type="button" disabled={busy} onClick={() => void resolveSourceRequest(request, false)}>Deny</button>
				</div>
			</section>)}
			{grants.filter((request)=>request.status==="pending").map((request)=><section className="workspace-grant-card" key={request.id} aria-label="Pending folder access request" data-testid="workspace-grant-pending"><small>Agent requested {request.access==="read_write"?"read/write":"read-only"} access</small><code title={request.path}>{compactPath(request.path)}</code><p>{request.reason}</p><div><button type="button" disabled={busy} onClick={()=>void resolveGrant(request,true)}>Approve…</button><button type="button" disabled={busy} onClick={()=>void resolveGrant(request,false)}>Deny</button></div></section>)}
			<div className="workspace-scope-actions">
				<button type="button" role="menuitem" disabled={busy || (!sessionId && !ensureSession)} onClick={() => void add("read_write")}>Add folder…</button>
				{recentFolders.filter((path) => path !== workspace && !scope?.attachments.some((item) => item.path === path)).length ? <section className="workspace-recent-folders" aria-label="Recent folders">
					<small>Recent folders</small>
					{recentFolders.filter((path) => path !== workspace && !scope?.attachments.some((item) => item.path === path)).map((path) => <button key={path} type="button" role="menuitem" disabled={busy || (!sessionId && !ensureSession)} onClick={() => void addRecent(path)} title={path}>
						<span>{compactPath(path)}</span><b>Add</b>
					</button>)}
				</section> : null}
				<button type="button" role="menuitem" disabled title="Read-only requires enforceable sandbox support">Add read-only folder…</button>
			</div>
		</div> : null}
	</div>;
}
