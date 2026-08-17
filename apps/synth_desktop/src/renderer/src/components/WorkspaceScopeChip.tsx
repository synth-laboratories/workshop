import { useEffect, useRef, useState } from "react";
import type { ConversationWorkspaceScope, WorkspaceAccessMode, WorkspaceGrantRequest } from "../bridge";
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
		{!hideTrigger ? <button type="button" className="workspace-scope-chip" onClick={() => setOpen((value) => !value)} aria-label={`Workspace: ${workspaceLabel(workspace)}`} aria-expanded={open} aria-controls="workspace-scope-menu" aria-haspopup="menu" data-testid="workspace-scope-chip" title={workspace}>
			<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden><path d="M2.25 4.25h4l1.2 1.4h6.3v6.1a1 1 0 01-1 1h-9.5a1 1 0 01-1-1v-7.5z" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" /></svg>
			<span>{workspaceLabel(workspace)}</span><span aria-hidden>⌄</span>
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
