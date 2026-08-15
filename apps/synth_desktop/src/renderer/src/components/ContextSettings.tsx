import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { ContextSkill, ContextSnapshot } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { SettingsCard } from "./SettingsCard";

function Toggle({ checked, disabled, label, onChange }: { checked: boolean; disabled?: boolean; label: string; onChange: (checked: boolean) => void }) {
	return <label className="context-toggle"><input type="checkbox" checked={checked} disabled={disabled} onChange={(event) => onChange(event.target.checked)} /><span aria-hidden /><em>{label}</em></label>;
}

type EditorTarget = { kind: "workspace" } | { kind: "workshop" } | { kind: "skill"; skillId: string };

function contextErrorMessage(reason: unknown): string {
	if (reason instanceof Error) return reason.message;
	if (reason && typeof reason === "object" && "message" in reason && typeof reason.message === "string") return reason.message;
	return typeof reason === "string" ? reason : "Context operation failed";
}

function ContextEditorDialog({ open, label, path, value, readOnly, dirty, busy, onChange, onClose, onSave, onCopy }: { open: boolean; label: string; path: string; value: string; readOnly?: boolean; dirty?: boolean; busy?: boolean; onChange?: (value: string) => void; onClose: () => void; onSave?: () => void; onCopy?: () => void }) {
	const dialog = useRef<HTMLDialogElement>(null);
	useEffect(() => {
		if (open && !dialog.current?.open) dialog.current?.showModal();
		if (!open && dialog.current?.open) dialog.current.close();
	}, [open]);
	return <dialog ref={dialog} className="context-editor-dialog" aria-label={label} onCancel={(event) => { event.preventDefault(); onClose(); }} onClose={onClose}>
		<header><div><span>Context document</span><h3>{label}</h3><code>{path}</code></div><button type="button" aria-label="Close" onClick={onClose}>×</button></header>
		<div className="context-editor-dialog-body"><textarea value={value} readOnly={readOnly} onChange={(event) => onChange?.(event.target.value)} aria-label={label} spellCheck={false} autoFocus={!readOnly} /></div>
		<footer><span>{readOnly ? "Bundled · read only" : dirty ? "Unsaved changes" : "Saved"}</span>{onCopy ? <button type="button" onClick={onCopy}>Copy</button> : null}<button type="button" onClick={onClose}>Close</button>{onSave ? <button type="button" className="primary" disabled={busy || !dirty} onClick={onSave}>{busy ? "Saving…" : "Save"}</button> : null}</footer>
	</dialog>;
}

export function ContextSettings({ subagents }: { subagents: ReactNode }) {
	const [workspace, setWorkspace] = useState<string | null>(null);
	const [snapshot, setSnapshot] = useState<ContextSnapshot | null>(null);
	const [workspaceDraft, setWorkspaceDraft] = useState("");
	const [editor, setEditor] = useState<EditorTarget | null>(null);
	const [skillDraft, setSkillDraft] = useState("");
	const [busy, setBusy] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);

	const accept = (next: ContextSnapshot) => { setSnapshot(next); setWorkspaceDraft(next.workspaceAgents.content); };
	useEffect(() => {
		let cancelled = false;
		void (async () => {
			try {
				const path = await bridges.codex?.defaultWorkspace();
				if (!path || cancelled) return;
				setWorkspace(path);
				const next = await bridges.context!.snapshot(path);
				if (!cancelled) accept(next);
			} catch (reason) { if (!cancelled) setError(contextErrorMessage(reason)); }
		})();
		return () => { cancelled = true; };
	}, []);

	const run = async (id: string, operation: () => Promise<ContextSnapshot | undefined>) => {
		setBusy(id); setError(null);
		try { const next = await operation(); if (next) accept(next); }
		catch (reason) {
			const message = contextErrorMessage(reason);
			if (!message.toLowerCase().includes("cancelled")) setError(message);
		}
		finally { setBusy(null); }
	};

	if (!workspace || !snapshot) return <div className="settings-sections" data-testid="settings-context"><p className="settings-runtime-copy">{error ?? "Loading context…"}</p></div>;

	const cookbookSkill = snapshot.skills.find((skill) => skill.source === "cookbook");
	const editableSkills = snapshot.skills.filter((skill) => skill.source !== "cookbook");
	const enabledSkills = editableSkills.filter((skill) => skill.enabled).length;
	const configuredGroups = snapshot.mcpGroups.filter((group) => group.servers.length > 0);
	const enabledGroups = configuredGroups.filter((group) => group.enabled).length;
	const activeSkill = editor?.kind === "skill" ? snapshot.skills.find((skill) => skill.id === editor.skillId) : undefined;
	const editorLabel = editor?.kind === "workspace" ? "Your AGENTS.md" : editor?.kind === "workshop" ? "Workshop defaults" : activeSkill ? `${activeSkill.name}/SKILL.md` : "Context";
	const editorPath = editor?.kind === "workspace" ? snapshot.workspaceAgents.path : editor?.kind === "workshop" ? snapshot.workshopAgents.path : activeSkill?.path ?? (activeSkill ? `User copy · ${activeSkill.id}` : "");
	const editorValue = editor?.kind === "workspace" ? workspaceDraft : editor?.kind === "workshop" ? snapshot.workshopAgents.content : skillDraft;
	const editorDirty = editor?.kind === "workspace" ? workspaceDraft !== snapshot.workspaceAgents.content : Boolean(activeSkill && skillDraft !== activeSkill.content);
	const cookbookBusy = busy === "cookbooks";
	const cancelCookbooks = () => {
		setError(null);
		void bridges.context!.cancelCookbooks(workspace).then(accept).catch((reason) => {
			setError(contextErrorMessage(reason));
		});
	};

	const openSkill = (skill: ContextSkill) => { setSkillDraft(skill.content); setEditor({ kind: "skill", skillId: skill.id }); };
	const saveEditor = editor?.kind === "workspace"
		? () => void run("agents", async () => { const next = await bridges.context!.updateWorkspaceAgents(workspace, workspaceDraft); setEditor(null); return next; })
		: activeSkill
			? () => void run(`skill:${activeSkill.id}`, async () => { const next = await bridges.context!.updateSkill(workspace, activeSkill.id, activeSkill.enabled, skillDraft); setEditor(null); return next; })
			: undefined;

	return <div className="settings-sections context-settings context-settings-v2" data-testid="settings-context">
		<header className="context-hero context-hero-compact"><div><span className="context-eyebrow">Next session</span><h3>Agent context</h3><p>What Workshop will load when a new agent session starts.</p></div><div className="context-health"><i /><span>Ready</span></div></header>

		<div className="context-overview" aria-label="Context overview">
			<div><span>Instructions</span><strong>{snapshot.workspaceAgents.state === "absent" ? "Workshop defaults" : "Workspace overlay"}</strong></div>
			<div><span>Capabilities</span><strong>{enabledSkills} skills · {enabledGroups} MCP group</strong></div>
			<div><span>Cookbook</span><strong>{snapshot.cookbooks.enabled ? "Included" : snapshot.cookbooks.installed ? "Installed, excluded" : "Not installed"}</strong></div>
		</div>
		{error ? <div className="model-locations-error" role="alert">{error}</div> : null}

		<SettingsCard title="Instructions" description="Workshop defaults plus an optional workspace overlay." testId="context-instructions" className="context-compact-card">
			<div className="context-document-list">
				<article><div><strong>Your AGENTS.md</strong><span>{snapshot.workspaceAgents.state === "absent" ? "No workspace overlay. Workshop defaults apply." : "Workspace-specific instructions override the defaults."}</span><code>{snapshot.workspaceAgents.path}</code></div><button type="button" className="settings-secondary-btn" onClick={() => { setWorkspaceDraft(snapshot.workspaceAgents.content); setEditor({ kind: "workspace" }); }}>{snapshot.workspaceAgents.state === "absent" ? "Add instructions" : "Edit"}</button></article>
				<article><div><strong>Workshop defaults</strong><span>Bundled product policy · v{snapshot.workshopAgents.version ?? "dev"}</span></div><button type="button" className="settings-secondary-btn" onClick={() => setEditor({ kind: "workshop" })}>View</button></article>
			</div>
		</SettingsCard>

		<SettingsCard title="Capabilities" description="Skills and MCP servers available to new sessions." testId="context-capabilities" actions={<span className="context-quiet-count">{enabledSkills + enabledGroups} active</span>} className="context-compact-card context-capabilities">
			<div className="context-capability-section"><h4>Skills</h4><div className="context-capability-list">{editableSkills.map((skill) => <article key={skill.id}><div><strong>{skill.name}</strong><span>{skill.description}</span></div><div className="context-capability-actions"><Toggle checked={skill.enabled} label={skill.enabled ? "On" : "Off"} onChange={(enabled) => void run(`skill:${skill.id}`, () => bridges.context!.updateSkill(workspace, skill.id, enabled))} /><button type="button" onClick={() => openSkill(skill)}>Edit</button></div></article>)}</div></div>
			<div className="context-capability-section"><h4>MCP groups</h4><div className="context-capability-list">{snapshot.mcpGroups.map((group) => { const configured = group.servers.length > 0; return <article key={group.id} className={!configured ? "disabled" : ""}><div><strong>{group.label}</strong><span>{configured ? group.servers.join(" · ") : "Not configured"}</span></div>{configured ? <Toggle checked={group.enabled} label={group.enabled ? "On" : "Off"} onChange={(enabled) => void run(`mcp:${group.id}`, () => bridges.context!.updateMcpGroup(workspace, group.id, enabled))} /> : <span className="context-not-configured">Unavailable</span>}</article>; })}</div></div>
		</SettingsCard>

		<SettingsCard title="Cookbooks" description="Pinned public recipes; runs/ is never checked out." testId="context-cookbooks" actions={<span className="context-quiet-count">{cookbookBusy ? "Installing" : snapshot.cookbooks.enabled ? "Included" : snapshot.cookbooks.installed ? "Excluded" : "Not installed"}</span>} className="context-compact-card">
			<div className="context-cookbook-row"><div><strong>{snapshot.cookbooks.pin ? `Public cookbook · ${snapshot.cookbooks.pin.slice(0, 12)}` : "Public cookbook"}</strong><span>{cookbookBusy ? "Fetching the public cookbook pin" : snapshot.cookbooks.installed ? snapshot.cookbooks.digest : "Install a pinned checkout without runs/ or private overlays."}</span>{cookbookSkill && !snapshot.cookbooks.installed ? <small>The cookbook skill becomes available after installation.</small> : null}</div><div className="context-capability-actions">{snapshot.cookbooks.installed ? <Toggle checked={snapshot.cookbooks.enabled} disabled={cookbookBusy} label={snapshot.cookbooks.enabled ? "Included" : "Excluded"} onChange={(enabled) => void run("cookbooks", () => bridges.context!.setCookbooksEnabled(workspace, enabled))} /> : null}{cookbookBusy ? <button type="button" onClick={cancelCookbooks}>Cancel</button> : <button type="button" onClick={() => void run("cookbooks", () => bridges.context!.installCookbooks(workspace))}>{snapshot.cookbooks.installed ? "Update" : "Install"}</button>}{snapshot.cookbooks.installed ? <button type="button" className="danger" disabled={cookbookBusy} onClick={() => void run("cookbooks", () => bridges.context!.uninstallCookbooks(workspace))}>Uninstall</button> : null}</div></div>
		</SettingsCard>

		<details className="context-advanced" data-testid="context-advanced"><summary><div><strong>Advanced</strong><span>Subagent compatibility and generated model settings</span></div><span aria-hidden>›</span></summary><div className="context-advanced-body">{subagents}</div></details>

		<ContextEditorDialog open={Boolean(editor)} label={editorLabel} path={editorPath} value={editorValue} readOnly={editor?.kind === "workshop"} dirty={editorDirty} busy={busy === "agents" || Boolean(activeSkill && busy === `skill:${activeSkill.id}`)} onChange={editor?.kind === "workspace" ? setWorkspaceDraft : editor?.kind === "skill" ? setSkillDraft : undefined} onClose={() => setEditor(null)} onSave={saveEditor} onCopy={editor?.kind === "workshop" ? () => void navigator.clipboard.writeText(snapshot.workshopAgents.content) : undefined} />
	</div>;
}
