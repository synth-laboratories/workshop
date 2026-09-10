import { useEffect, useState } from "react";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";
import type { SynthBackendSettings } from "../bridge";
import { SettingsCard } from "./SettingsCard";

export function OpenRouterSetupCard() {
	const [settings, setSettings] = useState<SynthBackendSettings | null>(null);
	const [path, setPath] = useState("");
	const [busy, setBusy] = useState(false);
	const [message, setMessage] = useState("");
	useEffect(() => {
		let active = true;
		void bridges.config?.get().then(value => { if (active) { setSettings(value); setPath(value.envFile); } }).catch(error => { if (active) setMessage(publicError(error)); });
		return () => { active = false; };
	}, []);
	const save = async () => {
		if (!bridges.config || !path.trim()) return;
		if (!path.trim().startsWith("/") || path.includes("=") || /[\r\n]/.test(path)) {
			setMessage("Choose an absolute file path, not a key or an environment assignment.");
			return;
		}
		setBusy(true); setMessage("");
		try {
			// Read current routing before saving; never overwrite it from stale UI state.
			const current = await bridges.config.get();
			const next = await bridges.config.update({ profile: current.profile, backendUrl: current.backendUrl, apiKeyEnv: current.apiKeyEnv, envFile: path.trim() });
			setSettings(next); setPath(next.envFile);
			window.dispatchEvent(new CustomEvent("synth:account-changed"));
			setMessage(next.openrouterApiKeyConfigured ? "OpenRouter key detected. Account credit and model access are checked when you send a message." : "No OpenRouter key found. Add OPENROUTER_API_KEY to this file, then check again.");
		} catch (error) { setMessage(publicError(error)); }
		finally { setBusy(false); }
	};
	return <SettingsCard title="OpenRouter · API credits" testId="openrouter-setup" className="settings-card-embed">
		<p>Use your own OpenRouter key. This bills OpenRouter credits, not your ChatGPT allowance. A failed ChatGPT request never switches here automatically.</p>
		<p>Create a private file containing <code>OPENROUTER_API_KEY=your-key</code>, restrict its permissions to your user (for example <code>chmod 600 .env</code>), and select its path below. Never paste a key into this path field or commit the file.</p>
		<p>The native host reads the file; this setup does not import anything into Keychain. This env-file setting is shared with Synth API routing: preserve any existing variables in the selected file.</p>
		<label>Private env file path<input aria-label="OpenRouter env file path" value={path} onChange={e => setPath(e.target.value)} spellCheck={false} /></label>
		<div className="settings-inline-actions">
			<button type="button" disabled={busy || !bridges.config?.chooseEnvFile} onClick={() => void bridges.config?.chooseEnvFile?.().then(value => { if (value) setPath(value); }).catch(error => setMessage(publicError(error)))}>Choose file…</button>
			<button type="button" disabled={busy || !path.trim()} onClick={() => void save()}>{busy ? "Checking…" : "Save and check key"}</button>
		</div>
		<p>{settings?.openrouterApiKeyConfigured ? `Key detected · ${settings.openrouterApiKeyFingerprint ?? "configured"}` : "OpenRouter not configured"}</p>
		<p role="status">{message}</p>
		<p>Then choose API → OpenRouter → GPT-6 Astra in the composer. Metadata refresh uses the public catalog and does not run a model.</p>
	</SettingsCard>;
}
