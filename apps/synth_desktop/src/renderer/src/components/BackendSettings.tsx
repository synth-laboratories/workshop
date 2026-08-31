import { useEffect, useState } from "react";
import type { SynthBackendSettings } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";
import { useSynthConnection } from "../hooks/useSynthConnection";

function announceAccountChange(next: SynthBackendSettings) {
	window.dispatchEvent(new CustomEvent("synth:account-changed", {
		detail: { apiKeyConfigured: next.apiKeyConfigured }
	}));
}

/**
 * Browser sign-in for this device. Lives on its own so the Account page can put
 * it under Devices & security. Credentials are acquired by the native host;
 * the renderer never accepts key material.
 */
export function AccountSignIn() {
	const [settings, setSettings] = useState<SynthBackendSettings | null>(null);
	const [status, setStatus] = useState<string | null>(null);
	const [saving, setSaving] = useState(false);
	const connection = useSynthConnection();

	const load = () => {
		void bridges.config?.get().then(setSettings).catch(() => undefined);
	};
	useEffect(() => {
		load();
		const onChanged = () => load();
		window.addEventListener("synth:account-changed", onChanged);
		return () => window.removeEventListener("synth:account-changed", onChanged);
	}, []);

	const signOut = async () => {
		if (!bridges.account) return;
		setSaving(true);
		try {
			const next = await bridges.account.signOut();
			setSettings(next);
			announceAccountChange(next);
			setStatus("Signed out · cloud credentials removed");
		} catch (error) {
			setStatus(publicError(error));
		} finally {
			setSaving(false);
		}
	};
	return (
		<div className="backend-signin" data-testid="account-sign-in">
			{connection.state.kind === "opening_browser" || connection.state.kind === "awaiting_approval" ? (
				<>
					<span role="status" className="finetune-meta" data-testid="sign-in-status">
						Browser sign-in started. Finish signup or sign-in there — Workshop updates automatically when pairing completes.
					</span>
					<span className="finetune-meta backend-signin-note" data-testid="sign-in-browser-help">
						If no page appeared, check your browser tabs or choose Reopen browser. You can safely cancel and retry.
					</span>
					{connection.state.kind === "awaiting_approval" && connection.state.begin.userCode ? (
						<span className="finetune-meta backend-signin-code" data-testid="sign-in-user-code">
							Approve only if the browser shows pairing code{" "}
							<strong>{connection.state.begin.userCode}</strong>.
						</span>
					) : null}
					<div className="backend-signin-actions">
						<button type="button" className="settings-secondary-btn" onClick={() => void connection.reopenBrowser()}>Reopen browser</button>
						<button type="button" className="settings-secondary-btn" data-testid="sign-in-cancel" onClick={() => void connection.cancel()}>Cancel</button>
					</div>
				</>
			) : (
				<>
					{/* Steady-state copy stays here; a transient confirmation gets its own
					    line so the status never hides what the device's state is. */}
					<span role="status" className="finetune-meta" data-testid="sign-in-status">
						{connection.state.kind === "failed" || connection.state.kind === "expired"
							? connection.state.message
							: connection.state.kind === "connected"
								? "Connected to Synth · runtime reconnected"
							: settings?.apiKeyConfigured
								? "Connected to Synth. Sign in again to switch accounts."
								: "New here? Browser sign-in creates your Synth account and connects this device."}
					</span>
					<div className="backend-signin-actions">
						<button type="button" className="settings-secondary-btn" data-testid="sign-in-begin" onClick={() => void connection.start()}>
							Connect Synth in browser
						</button>
						{settings?.apiKeyConfigured ? (
							<button type="button" className="settings-secondary-btn" data-testid="account-sign-out" disabled={saving} onClick={() => void signOut()}>
								Sign out
							</button>
						) : null}
					</div>
					{status ? <span className="finetune-meta backend-signin-note" data-testid="account-sign-in-note">{status}</span> : null}
				</>
			)}
		</div>
	);
}

/** Advanced connection: endpoint and native-host credential references. */
export function BackendSettings() {
	const PROFILE_ENDPOINTS: Record<string, string> = {
		prod: "https://api.usesynth.ai",
		staging: "https://api-dev.usesynth.ai",
		local: "http://127.0.0.1:8000"
	};
	const [settings, setSettings] = useState<SynthBackendSettings | null>(null);
	const [profile, setProfile] = useState("prod");
	const [backendUrl, setBackendUrl] = useState("");
	const [envFile, setEnvFile] = useState("");
	const [apiKeyEnv, setApiKeyEnv] = useState("SYNTH_API_KEY");
	const [status, setStatus] = useState<string | null>(null);
	const [saving, setSaving] = useState(false);
	const selectProfile = (nextProfile: string) => {
		const currentDefault = PROFILE_ENDPOINTS[profile];
		setProfile(nextProfile);
		// Preserve explicitly customized endpoints. Only follow the selected
		// profile when the current URL is empty or still a known default.
		if (!backendUrl.trim() || backendUrl === currentDefault) {
			setBackendUrl(PROFILE_ENDPOINTS[nextProfile] ?? backendUrl);
		}
	};

	// Reading settings must never announce a change: this panel also listens for
	// that event, and re-broadcasting on load would loop.
	const apply = (next: SynthBackendSettings) => {
		setSettings(next);
		setProfile(next.profile);
		setBackendUrl(next.backendUrl);
		setEnvFile(next.envFile);
		setApiKeyEnv(next.apiKeyEnv);
	};
	useEffect(() => {
		const load = () => {
			void bridges.config?.get().then(apply).catch((error) => setStatus(publicError(error)));
		};
		load();
		// Sign-in and sign-out now happen in Devices & security; this panel must
		// still show the resulting credential state rather than a stale one.
		const onChanged = () => load();
		window.addEventListener("synth:account-changed", onChanged);
		return () => window.removeEventListener("synth:account-changed", onChanged);
	}, []);

	const save = async () => {
		if (!bridges.config) return;
		setSaving(true);
		setStatus(null);
		try {
			const next = await bridges.config.update({
				profile, backendUrl, envFile, apiKeyEnv
			});
			apply(next);
			announceAccountChange(next);
			setStatus("Saved · runtime restarted with this backend");
		} catch (error) {
			setStatus(publicError(error));
		} finally {
			setSaving(false);
		}
	};

	return (
		<div className="settings-finetunes backend-settings" data-testid="backend-settings">
			<header className="settings-section-head">
				<div><h2>Synth API</h2><p>Routing is stored in TOML. Credentials must already exist in a private env file read only by the native host.</p></div>
				<span className="finetune-badge">{settings?.apiKeyConfigured ? "Authenticated" : "API key required"}</span>
			</header>
			<div className="backend-settings-grid">
				<label><span>Profile</span><select value={profile} onChange={(event) => selectProfile(event.target.value)}>
					<option value="prod">Production</option><option value="staging">Staging</option><option value="local">Local</option>
					{!["prod", "staging", "local"].includes(profile) ? <option value={profile}>{profile}</option> : null}
				</select></label>
				<label className="backend-settings-wide"><span>Backend API</span><input value={backendUrl} onChange={(event) => setBackendUrl(event.target.value)} placeholder="http://127.0.0.1:8000" spellCheck={false} /></label>
				<label className="backend-settings-wide"><span>Secrets env file</span><input value={envFile} onChange={(event) => setEnvFile(event.target.value)} placeholder="~/.synth-desktop/.env" spellCheck={false} /></label>
				<label><span>API key variable</span><input value={apiKeyEnv} onChange={(event) => setApiKeyEnv(event.target.value)} spellCheck={false} /></label>
			</div>
			<div className="backend-config-facts">
				<div><span>Config</span><code>{settings?.configPath ?? "Loading…"}</code></div>
				<div><span>Credential</span><code>{settings?.apiKeyConfigured ? `${settings.apiKeyFingerprint} · ${settings.apiKeySource}` : `Set ${apiKeyEnv} in the secrets env file`}</code></div>
				<div><span>Internal activity</span><code>{settings?.workerKeyConfigured ? "Worker credential available" : "Public mailbox only"}</code></div>
				<div><span>OpenRouter</span><code>{settings?.openrouterApiKeyConfigured ? `${settings.openrouterApiKeyFingerprint} · ${settings.openrouterApiKeySource}` : "Set OPENROUTER_API_KEY in the secrets env file"}</code></div>
			</div>
			<div className="backend-settings-actions"><span role="status" className="finetune-meta">{status}</span><button type="button" className="settings-secondary-btn" disabled={saving || !backendUrl.trim() || !envFile.trim()} onClick={() => void save()}>{saving ? "Saving…" : "Save and reconnect"}</button></div>
		</div>
	);
}
