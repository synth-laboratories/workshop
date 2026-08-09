import { useEffect, useRef, useState } from "react";
import type { SynthBackendSettings } from "../env";

type PairState =
	| { kind: "idle" }
	| { kind: "pairing"; verificationUri: string }
	| { kind: "error"; message: string };

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
	const [apiKey, setApiKey] = useState("");
	const [openrouterApiKey, setOpenrouterApiKey] = useState("");
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

	const apply = (next: SynthBackendSettings) => {
		setSettings(next);
		setProfile(next.profile);
		setBackendUrl(next.backendUrl);
		setEnvFile(next.envFile);
		setApiKeyEnv(next.apiKeyEnv);
	};

	useEffect(() => {
		void window.synthConfig?.get().then(apply).catch((error) => setStatus(String(error)));
	}, []);

	const [pair, setPair] = useState<PairState>({ kind: "idle" });
	const pollTimer = useRef<number | null>(null);
	const stopPolling = () => {
		if (pollTimer.current !== null) {
			window.clearInterval(pollTimer.current);
			pollTimer.current = null;
		}
	};
	useEffect(() => stopPolling, []);
	const beginSignIn = async () => {
		if (!window.synthAccount) return;
		try {
			const begin = await window.synthAccount.beginSignIn();
			setPair({ kind: "pairing", verificationUri: begin.verificationUri });
			stopPolling();
			pollTimer.current = window.setInterval(() => {
				void window.synthAccount?.pollSignIn().then((result) => {
					if (result.status === "active") {
						stopPolling();
						setPair({ kind: "idle" });
						setStatus("Signed in · runtime reconnected");
						void window.synthConfig?.get().then(apply);
					} else if (result.status === "expired") {
						stopPolling();
						setPair({ kind: "error", message: result.reason });
					}
				}).catch((error) => {
					stopPolling();
					setPair({ kind: "error", message: error instanceof Error ? error.message : String(error) });
				});
			}, 4000);
		} catch (error) {
			setPair({ kind: "error", message: error instanceof Error ? error.message : String(error) });
		}
	};
	const cancelSignIn = () => {
		stopPolling();
		setPair({ kind: "idle" });
		void window.synthAccount?.cancelSignIn();
	};

	const save = async () => {
		if (!window.synthConfig) return;
		setSaving(true);
		setStatus(null);
		try {
			const next = await window.synthConfig.update({
				profile, backendUrl, envFile, apiKeyEnv,
				apiKey: apiKey.trim() || undefined,
				openrouterApiKey: openrouterApiKey.trim() || undefined
			});
			apply(next);
			setApiKey("");
			setOpenrouterApiKey("");
			setStatus(next.apiKeySource === "process environment" && apiKey.trim()
				? "Saved · process environment still overrides the env file"
				: "Saved · runtime restarted with this backend");
		} catch (error) {
			setStatus(error instanceof Error ? error.message : String(error));
		} finally {
			setSaving(false);
		}
	};

	return (
		<div className="settings-finetunes backend-settings" data-testid="backend-settings">
			<header className="settings-section-head">
				<div><h2>Synth API</h2><p>Routing is stored in TOML. Credentials stay in a private env file read only by the native host.</p></div>
				<span className="finetune-badge">{settings?.apiKeyConfigured ? "Authenticated" : "API key required"}</span>
			</header>
			<div className="backend-signin" data-testid="account-sign-in">
				{pair.kind === "pairing" ? (
					<>
						<span role="status" className="finetune-meta" data-testid="sign-in-status">
							Finish sign-in in your browser — this page updates automatically.
						</span>
						<button type="button" className="settings-secondary-btn" onClick={() => void beginSignIn()}>Reopen browser</button>
						<button type="button" className="settings-secondary-btn" data-testid="sign-in-cancel" onClick={cancelSignIn}>Cancel</button>
					</>
				) : (
					<>
						<span role="status" className="finetune-meta" data-testid="sign-in-status">
							{pair.kind === "error"
								? pair.message
								: settings?.apiKeyConfigured
									? "Connected to Synth. Sign in again to switch accounts."
									: "New here? Browser sign-in creates your Synth account and connects this device."}
						</span>
						<button type="button" className="settings-secondary-btn" data-testid="sign-in-begin" onClick={() => void beginSignIn()}>
							{settings?.apiKeyConfigured ? "Sign in again" : "Sign in with browser"}
						</button>
					</>
				)}
			</div>
			<div className="backend-settings-grid">
				<label><span>Profile</span><select value={profile} onChange={(event) => selectProfile(event.target.value)}>
					<option value="prod">Production</option><option value="staging">Staging</option><option value="local">Local</option>
					{!["prod", "staging", "local"].includes(profile) ? <option value={profile}>{profile}</option> : null}
				</select></label>
				<label className="backend-settings-wide"><span>Backend API</span><input value={backendUrl} onChange={(event) => setBackendUrl(event.target.value)} placeholder="http://127.0.0.1:8000" spellCheck={false} /></label>
				<label className="backend-settings-wide"><span>Secrets env file</span><input value={envFile} onChange={(event) => setEnvFile(event.target.value)} placeholder="~/.synth-desktop/.env" spellCheck={false} /></label>
				<label><span>API key variable</span><input value={apiKeyEnv} onChange={(event) => setApiKeyEnv(event.target.value)} spellCheck={false} /></label>
				<label><span>{settings?.apiKeyConfigured ? "Replace API key" : "API key"}</span><input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={settings?.apiKeyConfigured ? "Leave blank to keep current key" : "Paste API key"} autoComplete="off" /></label>
				<label className="backend-settings-wide"><span>{settings?.openrouterApiKeyConfigured ? "Replace OpenRouter API key" : "OpenRouter API key"}</span><input type="password" value={openrouterApiKey} onChange={(event) => setOpenrouterApiKey(event.target.value)} placeholder={settings?.openrouterApiKeyConfigured ? "Configured — leave blank to keep current key" : "Required for GPT 5.6 Luna and Laguna S"} autoComplete="off" /></label>
			</div>
			<div className="backend-config-facts">
				<div><span>Config</span><code>{settings?.configPath ?? "Loading…"}</code></div>
				<div><span>Credential</span><code>{settings?.apiKeyConfigured ? `${settings.apiKeyFingerprint} · ${settings.apiKeySource}` : "Not configured"}</code></div>
				<div><span>Internal activity</span><code>{settings?.workerKeyConfigured ? "Worker credential available" : "Public mailbox only"}</code></div>
				<div><span>OpenRouter</span><code>{settings?.openrouterApiKeyConfigured ? `${settings.openrouterApiKeyFingerprint} · ${settings.openrouterApiKeySource}` : "Not configured — remote models disabled"}</code></div>
			</div>
			<div className="backend-settings-actions"><span role="status" className="finetune-meta">{status}</span><button type="button" className="settings-secondary-btn" disabled={saving || !backendUrl.trim() || !envFile.trim()} onClick={() => void save()}>{saving ? "Saving…" : "Save and reconnect"}</button></div>
		</div>
	);
}
