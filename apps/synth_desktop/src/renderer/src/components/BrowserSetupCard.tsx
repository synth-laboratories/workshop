import { FormEvent, useEffect, useState } from "react";
import type { BrowserRuntimeStatus } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { SettingsCard } from "./SettingsCard";

function message(reason: unknown): string {
	return reason instanceof Error ? reason.message : String(reason);
}

export function BrowserSetupCard() {
	const [status, setStatus] = useState<BrowserRuntimeStatus | null>(null);
	const [origin, setOrigin] = useState("");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const refresh = async () => {
		if (!bridges.browserAdmin) return;
		setBusy(true);
		setError(null);
		try { setStatus(await bridges.browserAdmin.status()); }
		catch (reason) { setError(message(reason)); }
		finally { setBusy(false); }
	};

	useEffect(() => { void refresh(); }, []);
	if (!bridges.browserAdmin) return null;

	const allow = async (event: FormEvent) => {
		event.preventDefault();
		setBusy(true);
		setError(null);
		try {
			setStatus(await bridges.browserAdmin!.allowOrigin(origin));
			setOrigin("");
		} catch (reason) { setError(message(reason)); }
		finally { setBusy(false); }
	};
	const revoke = async (approved: string) => {
		setBusy(true);
		setError(null);
		try { setStatus(await bridges.browserAdmin!.revokeOrigin(approved)); }
		catch (reason) { setError(message(reason)); }
		finally { setBusy(false); }
	};
	const restart = async () => {
		setBusy(true);
		setError(null);
		try { setStatus(await bridges.browserAdmin!.restart()); }
		catch (reason) { setError(message(reason)); }
		finally { setBusy(false); }
	};
	const chooseUploadRoot = async () => {
		setBusy(true);
		setError(null);
		try { setStatus(await bridges.browserAdmin!.chooseUploadRoot()); }
		catch (reason) { setError(message(reason)); }
		finally { setBusy(false); }
	};
	const revokeUploadRoot = async (path: string) => {
		setBusy(true);
		setError(null);
		try { setStatus(await bridges.browserAdmin!.revokeUploadRoot(path)); }
		catch (reason) { setError(message(reason)); }
		finally { setBusy(false); }
	};

	return <SettingsCard
		title="Managed Browser"
		description="Runtime readiness and origins new browser sessions may visit."
		testId="browser-setup"
		actions={<><button type="button" className="settings-secondary-btn" disabled={busy || !status?.serviceRunning} onClick={() => void restart()}>Restart service</button><button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void refresh()}>Refresh</button></>}
		className="context-compact-card browser-setup-card"
	>
		<div className="browser-runtime-summary" data-phase={status?.phase ?? "checking"}>
			<strong>{status?.phase === "ready" ? "Ready" : status ? "Setup required" : "Checking…"}</strong>
			<span>{status?.detail ?? "Checking the local Playwright and Chromium runtime."}</span>
		</div>
		{status ? <div className="browser-runtime-checks" aria-label="Managed browser runtime checks">
			{[["Backend", status.backendPresent], [status.nodeVersion ?? "Node", status.nodePresent], ["Playwright", status.playwrightPresent], ["Chromium", status.chromiumPresent]].map(([label, ready]) => <span key={String(label)} className={ready ? "ready" : "missing"}>{ready ? "✓" : "×"} {label}</span>)}
			<span className={status.serviceRunning ? "ready" : "missing"}>{status.serviceRunning ? "✓ Service running" : "Service stopped"}{status.crashCount ? ` · ${status.crashCount} crash${status.crashCount === 1 ? "" : "es"}` : ""}</span>
			<span className={status.chromeClaimEnabled ? "ready" : "missing"}>{status.chromeClaimEnabled ? "✓ Chrome claiming enabled" : "Chrome claiming disabled"}</span>
		</div> : null}
		<div className="browser-origin-policy">
			<div><strong>Approved website origins</strong><span>Pages cannot add origins themselves. Localhost is allowed for development.</span></div>
			{status?.allowedOrigins.length ? <ul>{status.allowedOrigins.map((approved) => <li key={approved}><code>{approved}</code><button type="button" disabled={busy} onClick={() => void revoke(approved)}>Revoke</button></li>)}</ul> : <p>No external origins approved.</p>}
			<form onSubmit={(event) => void allow(event)}>
				<input type="url" required placeholder="https://example.com" aria-label="Origin to approve" value={origin} onChange={(event) => setOrigin(event.target.value)} />
				<button type="submit" className="settings-secondary-btn" disabled={busy || !origin.trim()}>Approve origin</button>
			</form>
		</div>
		<div className="browser-origin-policy">
			<div><strong>Upload folders</strong><span>Only files under folders selected here can be attached. Every upload still requires exact confirmation.</span></div>
			{status?.uploadRoots.length ? <ul>{status.uploadRoots.map((path) => <li key={path}><code>{path}</code><button type="button" disabled={busy} onClick={() => void revokeUploadRoot(path)}>Revoke</button></li>)}</ul> : <p>No upload folders selected.</p>}
			<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void chooseUploadRoot()}>Choose upload folder</button>
		</div>
		{status ? <details className="browser-runtime-paths"><summary>Runtime paths</summary><code>{status.backendPath}</code><code>{status.profileRoot}</code></details> : null}
		{error ? <div className="model-locations-error" role="alert">{error}</div> : null}
	</SettingsCard>;
}
