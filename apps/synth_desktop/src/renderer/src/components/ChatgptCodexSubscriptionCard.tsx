import { useEffect, useRef, useState } from "react";
import type { CodexOauthStatus } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { SettingsCard } from "./SettingsCard";
import { ProviderMark } from "./ProviderMark";

const EMPTY: CodexOauthStatus = { state: "disconnected", action: "connect", canUseModels: false, guidance: "Connect a ChatGPT subscription.", configured: false, accountHint: null, lastRefresh: null, expiresAt: null };

export function oauthErrorMessage(reason: unknown): string {
	if (reason instanceof Error && reason.message.trim()) return reason.message;
	if (reason && typeof reason === "object") {
		const value = reason as { message?: unknown; error?: unknown };
		if (typeof value.message === "string" && value.message.trim()) return value.message;
		if (value.error && typeof value.error === "object") {
			const nested = value.error as { message?: unknown };
			if (typeof nested.message === "string" && nested.message.trim()) return nested.message;
		}
	}
	return "ChatGPT sign-in failed. Start over and try again.";
}

export function ChatgptCodexSubscriptionCard() {
	const [status, setStatus] = useState<CodexOauthStatus>(EMPTY);
	const [busy, setBusy] = useState(false);
	const [manual, setManual] = useState(false);
	const [redirectUrl, setRedirectUrl] = useState("");
	const [error, setError] = useState<string | null>(null);
	const pollRef = useRef<number | null>(null);

	const publish = (next: CodexOauthStatus) => {
		setStatus(next);
		window.dispatchEvent(new CustomEvent("codex-oauth-changed", { detail: next }));
	};

	useEffect(() => {
		void bridges.codexOauth?.status().then(publish).catch(() => setStatus(EMPTY));
		return () => { if (pollRef.current != null) window.clearInterval(pollRef.current); };
	}, []);

	const connect = async () => {
		setBusy(true);
		setError(null);
		try {
			const begin = await bridges.codexOauth!.begin();
			setManual(begin.mode === "manual");
			if (pollRef.current != null) window.clearInterval(pollRef.current);
			pollRef.current = window.setInterval(() => {
				void bridges.codexOauth!.status().then((next) => {
					publish(next);
					if (next.state === "ready" && pollRef.current != null) {
						window.clearInterval(pollRef.current);
						pollRef.current = null;
						setBusy(false);
					} else if (next.state === "refresh_failed" && pollRef.current != null) {
						window.clearInterval(pollRef.current);
						pollRef.current = null;
						setBusy(false);
						setError(next.guidance);
					}
				});
			}, 750);
		} catch (reason) {
			setBusy(false);
			setError(oauthErrorMessage(reason));
		}
	};

	const completeManual = async () => {
		setBusy(true);
		setError(null);
		try {
			publish(await bridges.codexOauth!.completeManual(redirectUrl));
			setRedirectUrl("");
			setManual(false);
		} catch (reason) {
			setError(oauthErrorMessage(reason));
		} finally {
			setBusy(false);
		}
	};

	const disconnect = async () => {
		setBusy(true);
		setError(null);
		try { publish(await bridges.codexOauth!.disconnect()); }
		catch (reason) { setError(oauthErrorMessage(reason)); }
		finally { setBusy(false); }
	};

	const cancel = async () => {
		if (pollRef.current != null) window.clearInterval(pollRef.current);
		pollRef.current = null;
		await bridges.codexOauth?.cancel().catch(() => undefined);
		setBusy(false);
	};

	const restart = async () => {
		if (pollRef.current != null) window.clearInterval(pollRef.current);
		pollRef.current = null;
		await bridges.codexOauth?.cancel().catch(() => undefined);
		setBusy(false);
		setError(null);
		await connect();
	};

	return (
		<SettingsCard title="ChatGPT subscription (Codex OAuth) — local personal use" testId="chatgpt-codex-subscription" className="settings-card-embed">
			<div className={`codex-subscription-status${status.canUseModels ? " is-connected" : ""}`} data-auth-state={status.state}>
				<div className="codex-subscription-status-head">
					<span className="codex-subscription-orb" aria-label="OpenAI"><ProviderMark kind="openai" className="codex-subscription-openai-mark" /></span>
					<div>
						<span className="finetune-kicker">ChatGPT plan</span>
						<strong data-testid="codex-oauth-status">{busy ? "Waiting for browser sign-in…" : ({ disconnected: "Not connected", authenticating: "Authenticating", ready: "Connected", expiring: "Refresh required", expired: "Authorization expired", refresh_failed: "Refresh failed" } as const)[status.state]}</strong>
						{status.accountHint ? <span className="codex-subscription-account">{status.accountHint}</span> : null}
					</div>
					<span className="codex-subscription-allowance"><span aria-hidden />Plan allowance</span>
				</div>
				<div className="codex-subscription-guidance">
					<span className="codex-subscription-guidance-icon" aria-hidden />
					<div>
						<p data-testid="codex-oauth-guidance">{status.guidance}</p>
						<p className="codex-subscription-note">Uses your Codex allowance, not API credits or Platform API access.</p>
					</div>
				</div>
				<div className="settings-inline-actions codex-subscription-actions">
					<button className="codex-subscription-primary" type="button" data-testid="codex-oauth-connect" disabled={busy} onClick={() => void connect()}><span aria-hidden>{busy ? "···" : "↻"}</span>{status.action === "reauthenticate" || status.action === "retry" ? "Re-sync ChatGPT" : status.configured ? "Re-authenticate" : "Connect ChatGPT"}</button>
					{busy ? <button className="codex-subscription-secondary" type="button" data-testid="codex-oauth-restart" onClick={() => void restart()}>Start over</button> : null}
					{busy ? <button className="codex-subscription-tertiary" type="button" data-testid="codex-oauth-cancel" onClick={() => void cancel()}>Cancel</button> : null}
					{status.configured ? <button className="codex-subscription-secondary" type="button" data-testid="codex-oauth-disconnect" disabled={busy} onClick={() => void disconnect()}>Disconnect</button> : null}
					<button className="codex-subscription-tertiary" type="button" data-testid="codex-oauth-show-manual" aria-expanded={manual} onClick={() => setManual((value) => !value)}>{manual ? "Hide redirect URL" : "Paste redirect URL"}</button>
				</div>
				{manual ? <div className="settings-inline-actions codex-subscription-manual" data-testid="codex-oauth-manual">
					<input aria-label="ChatGPT OAuth redirect URL" value={redirectUrl} onChange={(event) => setRedirectUrl(event.target.value)} placeholder="http://localhost:1455/auth/callback?code=…&state=…" />
					<button className="codex-subscription-primary" type="button" disabled={busy || !redirectUrl.trim()} onClick={() => void completeManual()}>Complete sign-in</button>
				</div> : null}
				{error ? <div className="model-locations-error" role="alert"><strong>Couldn’t re-sync ChatGPT.</strong> {error} Use Start over to create a fresh authorization attempt.</div> : null}
			</div>
			{status.canUseModels ? <div className="codex-subscription-models" data-testid="codex-oauth-authorized-models">
				<div className="codex-subscription-models-head"><div><strong>Available in the composer</strong><span>ChatGPT subscription · plan allowance</span></div><span>3 models</span></div>
				<div className="codex-subscription-model-grid">
					{[["GPT-5.6 Sol", "gpt-5.6-sol", "Fast iteration"], ["GPT-5.6 Luna", "gpt-5.6-luna", "Everyday coding"], ["GPT-5.6 Terra", "gpt-5.6-terra", "Deep reasoning"]].map(([name, id, fit]) => <article className="codex-subscription-model" key={id}><span className="codex-subscription-model-dot" aria-hidden /><div><strong>{name}</strong><span>{fit}</span><code>{id}</code></div></article>)}
				</div>
			</div> : null}
		</SettingsCard>
	);
}
