import { useEffect, useRef, useState } from "react";
import type { SynthBackendSettings } from "../bridge";
import { bridges } from "../runtime/desktopBridge";

type PairState =
	| { kind: "idle" }
	| { kind: "pairing"; verificationUri: string }
	| { kind: "error"; message: string };

function announceAccountChange(next: SynthBackendSettings) {
	window.dispatchEvent(new CustomEvent("synth:account-changed", {
		detail: { apiKeyConfigured: next.apiKeyConfigured }
	}));
}

/**
 * Browser sign-in for this device. Lives on its own so the Account page can put
 * it under Devices & security while the endpoint/key editor stays demoted to
 * Advanced connection — one sign-in affordance, not two.
 */
export function AccountSignIn() {
	const [settings, setSettings] = useState<SynthBackendSettings | null>(null);
	const [status, setStatus] = useState<string | null>(null);
	const [saving, setSaving] = useState(false);
	const [pair, setPair] = useState<PairState>({ kind: "idle" });
	const pollTimer = useRef<number | null>(null);

	const load = () => {
		void bridges.config?.get().then(setSettings).catch(() => undefined);
	};
	useEffect(() => {
		load();
		const onChanged = () => load();
		window.addEventListener("synth:account-changed", onChanged);
		return () => window.removeEventListener("synth:account-changed", onChanged);
	}, []);

	const stopPolling = () => {
		if (pollTimer.current !== null) {
			window.clearInterval(pollTimer.current);
			pollTimer.current = null;
		}
	};
	useEffect(() => stopPolling, []);

	const beginSignIn = async () => {
		if (!bridges.account) return;
		try {
			const begin = await bridges.account.beginSignIn();
			setPair({ kind: "pairing", verificationUri: begin.verificationUri });
			stopPolling();
			pollTimer.current = window.setInterval(() => {
				void bridges.account?.pollSignIn().then((result) => {
					if (result.status === "active") {
						stopPolling();
						setPair({ kind: "idle" });
						setStatus("Signed in · runtime reconnected");
						void bridges.config?.get().then((next) => {
							setSettings(next);
							announceAccountChange(next);
						});
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
		void bridges.account?.cancelSignIn();
	};
	const signOut = async () => {
		if (!bridges.account) return;
		setSaving(true);
		try {
			const next = await bridges.account.signOut();
			setSettings(next);
			announceAccountChange(next);
			setStatus("Signed out · cloud credentials removed");
		} catch (error) {
			setStatus(error instanceof Error ? error.message : String(error));
		} finally {
			setSaving(false);
		}
	};

	return (
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
					{/* Steady-state copy stays here; a transient confirmation gets its own
					    line so the status never hides what the device's state is. */}
					<span role="status" className="finetune-meta" data-testid="sign-in-status">
						{pair.kind === "error"
							? pair.message
							: settings?.apiKeyConfigured
								? "Connected to Synth. Sign in again to switch accounts."
								: "New here? Browser sign-in creates your Synth account and connects this device."}
					</span>
					{status ? <span className="finetune-meta" data-testid="account-sign-in-note">{status}</span> : null}
					<button type="button" className="settings-secondary-btn" data-testid="sign-in-begin" onClick={() => void beginSignIn()}>
						{settings?.apiKeyConfigured ? "Sign in again" : "Sign in with browser"}
					</button>
					{settings?.apiKeyConfigured ? (
						<button type="button" className="settings-secondary-btn" data-testid="account-sign-out" disabled={saving} onClick={() => void signOut()}>
							Sign out
						</button>
					) : null}
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
			void bridges.config?.get().then(apply).catch((error) => setStatus(String(error)));
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
			setStatus(error instanceof Error ? error.message : String(error));
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
