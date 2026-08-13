import { useEffect, useRef, useState } from "react";
import type { CodexOauthStatus } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { SettingsCard } from "./SettingsCard";
import { ProviderMark } from "./ProviderMark";

const EMPTY: CodexOauthStatus = { configured: false };

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
					if (next.configured && pollRef.current != null) {
						window.clearInterval(pollRef.current);
						pollRef.current = null;
						setBusy(false);
					}
				});
			}, 750);
		} catch (reason) {
			setBusy(false);
			setError(reason instanceof Error ? reason.message : String(reason));
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
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusy(false);
		}
	};

	const disconnect = async () => {
		setBusy(true);
		setError(null);
		try { publish(await bridges.codexOauth!.disconnect()); }
		catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
		finally { setBusy(false); }
	};

	const cancel = async () => {
		if (pollRef.current != null) window.clearInterval(pollRef.current);
		pollRef.current = null;
		await bridges.codexOauth?.cancel().catch(() => undefined);
		setBusy(false);
	};

	return (
		<SettingsCard title="ChatGPT subscription (Codex OAuth) — local personal use" testId="chatgpt-codex-subscription" className="settings-card-embed">
			<div className={`codex-subscription-status${status.configured ? " is-connected" : ""}`}>
				<div className="codex-subscription-status-head">
					<span className="codex-subscription-orb" aria-label="OpenAI"><ProviderMark kind="openai" className="codex-subscription-openai-mark" /></span>
					<div>
						<span className="finetune-kicker">ChatGPT plan</span>
						<strong data-testid="codex-oauth-status">{status.configured ? "Connected" : busy ? "Waiting for browser sign-in…" : "Not connected"}</strong>
						{status.accountHint ? <span className="codex-subscription-account">{status.accountHint}</span> : null}
					</div>
					<span className="codex-subscription-allowance">Plan allowance</span>
				</div>
				<p>Connect your ChatGPT subscription to use Codex in Workshop on this device. Your tokens remain stored locally on this Mac.</p>
				<p className="codex-subscription-note">Uses your Codex allowance — not API credits or Platform API access.</p>
				<div className="settings-inline-actions codex-subscription-actions">
					<button type="button" data-testid="codex-oauth-connect" disabled={busy} onClick={() => void connect()}>{status.configured ? "Re-authenticate" : "Connect"}</button>
					{busy ? <button type="button" data-testid="codex-oauth-cancel" onClick={() => void cancel()}>Cancel</button> : null}
					{status.configured ? <button type="button" data-testid="codex-oauth-disconnect" disabled={busy} onClick={() => void disconnect()}>Disconnect</button> : null}
					<button type="button" data-testid="codex-oauth-show-manual" onClick={() => setManual((value) => !value)}>Paste redirect URL</button>
				</div>
				{manual ? <div className="settings-inline-actions" data-testid="codex-oauth-manual">
					<input aria-label="ChatGPT OAuth redirect URL" value={redirectUrl} onChange={(event) => setRedirectUrl(event.target.value)} placeholder="http://localhost:1455/auth/callback?code=…&state=…" />
					<button type="button" disabled={busy || !redirectUrl.trim()} onClick={() => void completeManual()}>Complete sign-in</button>
				</div> : null}
				{error ? <div className="model-locations-error" role="alert">{error}</div> : null}
			</div>
			{status.configured ? <div className="codex-subscription-models" data-testid="codex-oauth-authorized-models">
				<div className="codex-subscription-models-head"><div><strong>Available in the composer</strong><span>ChatGPT subscription · plan allowance</span></div><span>3 models</span></div>
				<div className="codex-subscription-model-grid">
					{[["GPT-5.6 Sol", "gpt-5.6-sol", "Fast iteration"], ["GPT-5.6 Luna", "gpt-5.6-luna", "Everyday coding"], ["GPT-5.6 Terra", "gpt-5.6-terra", "Deep reasoning"]].map(([name, id, fit]) => <article className="codex-subscription-model" key={id}><span className="codex-subscription-model-dot" aria-hidden /><div><strong>{name}</strong><span>{fit}</span><code>{id}</code></div></article>)}
				</div>
			</div> : null}
		</SettingsCard>
	);
}
