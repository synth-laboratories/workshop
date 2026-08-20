import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { SettingsCard } from "./SettingsCard";
import { publicError } from "../runtime/publicError";
import { bridges } from "../runtime/desktopBridge";
import type { MaskedImportCandidate, PendingGrantSummary, SecretAuditEvent, SecretCapabilitySummary, SecretImportPreview, SecretSummary, SecretsInbox } from "../bridge";

const PROVIDERS = [
	{ id: "openai", label: "OpenAI" },
	{ id: "anthropic", label: "Anthropic" },
	{ id: "openrouter", label: "OpenRouter" },
	{ id: "tinker", label: "Tinker" },
	{ id: "groq", label: "Groq" }
];

function backendLabel(backend: string) {
	if (backend === "os-keychain") return "macOS Keychain";
	if (backend === "memory") return "this Workshop instance";
	return backend;
}

function statusLabel(item: SecretSummary) {
	if (item.status === "valid") return "Registered · tested";
	if (item.status === "locked") return "Registered · locked";
	if (item.status === "invalid") return "Registered · invalid";
	return "Registered";
}

export function SecretsSettings() {
	const secrets = bridges.secrets;
	const [items, setItems] = useState<SecretSummary[]>([]);
	const [capabilities, setCapabilities] = useState<SecretCapabilitySummary[]>([]);
	const [audit, setAudit] = useState<SecretAuditEvent[]>([]);
	const [inbox, setInbox] = useState<SecretsInbox>({ imports: [], grants: [], proxy: { running: false } });
	const [error, setError] = useState<string | null>(null);
	const [adding, setAdding] = useState<{ provider: string; alias: string; value: string } | null>(null);
	const [replacing, setReplacing] = useState<{ id: string; value: string } | null>(null);
	const [busy, setBusy] = useState(false);
	const [importPreview, setImportPreview] = useState<SecretImportPreview | null>(null);
	const [selectedVars, setSelectedVars] = useState<string[]>([]);
	const [afterImport, setAfterImport] = useState<"keep" | "replace_aliases" | "remove_entries">("keep");
	const [confirmCleanup, setConfirmCleanup] = useState(false);

	const refresh = async () => {
		if (!secrets) return;
		try {
			const [nextItems, nextCaps, nextAudit, nextInbox] = await Promise.all([
				secrets.list(),
				secrets.capabilities(),
				secrets.audit(40),
				secrets.pending()
			]);
			setItems(nextItems);
			setCapabilities(nextCaps);
			setAudit(nextAudit);
			setInbox(nextInbox);
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	};

	useEffect(() => {
		void refresh();
		const timer = window.setInterval(() => void refresh(), 2500);
		return () => window.clearInterval(timer);
	}, []);

	const review = importPreview ?? inbox.imports[0] ?? null;
	useEffect(() => {
		if (!review) return;
		setSelectedVars(review.candidates.filter((candidate) => candidate.selected).map((candidate) => candidate.variable));
	}, [review?.requestId]);

	const knownIds = new Set(PROVIDERS.map((provider) => provider.id));
	const grouped = PROVIDERS.map((provider) => ({
		...provider,
		connections: items.filter((item) => item.provider === provider.id)
	}));
	const other = items.filter((item) => !knownIds.has(item.provider));

	const add = async () => {
		if (!secrets || !adding) return;
		setBusy(true);
		try {
			await secrets.create({ alias: adding.alias || `Personal ${adding.provider}`, provider: adding.provider, value: adding.value });
			setAdding(null);
			await refresh();
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	const replace = async () => {
		if (!secrets || !replacing) return;
		setBusy(true);
		try {
			await secrets.replace(replacing.id, replacing.value);
			setReplacing(null);
			await refresh();
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	const pickEnv = async () => {
		if (!secrets) return;
		const selection = await open({ multiple: false, filters: [{ name: "Env files", extensions: ["env", ""] }] });
		const path = typeof selection === "string" ? selection : null;
		if (!path) return;
		setBusy(true);
		try {
			const preview = await secrets.requestEnvImport(path);
			setImportPreview(preview);
			setSelectedVars(preview.candidates.filter((candidate) => candidate.selected).map((candidate) => candidate.variable));
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	const commitImport = async () => {
		if (!secrets || !review) return;
		setBusy(true);
		try {
			await secrets.commitEnvImport(review.requestId, selectedVars, afterImport, afterImport === "keep" ? false : confirmCleanup);
			setImportPreview(null);
			await refresh();
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	const denyImport = async (requestId: string) => {
		if (!secrets) return;
		setBusy(true);
		try {
			await secrets.denyEnvImport(requestId);
			setImportPreview(null);
			await refresh();
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	const allowGrant = async (grant: PendingGrantSummary, remember: boolean) => {
		if (!secrets) return;
		setBusy(true);
		try {
			await secrets.grantUse(grant.secretId, grant.runId, grant.recipeId, remember, grant.requestId);
			await refresh();
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	const registeredCount = items.length;

	return (
		<div className="settings-sections" data-testid="settings-secrets">
			<SettingsCard
				title="Secrets & providers"
				description={
					registeredCount === 0
						? "No provider credentials are registered on this device yet. Add one here or ask the agent to import a .env — Workshop stores the value in Keychain and never shows it again."
						: `${registeredCount} registered on this device. Agents see aliases only. ${inbox.proxy.running ? "Provider proxy is running." : "Provider proxy is not running."}`
				}
				actions={
					<div className="secrets-card-actions">
						<button type="button" className="settings-secondary-btn" data-testid="secrets-add" onClick={() => setAdding({ provider: "openai", alias: "Personal OpenAI", value: "" })}>
							Add connection
						</button>
						<button type="button" className="settings-secondary-btn" data-testid="secrets-import" onClick={() => void pickEnv()}>
							Import from .env
						</button>
					</div>
				}
			>
				{error ? <p className="secrets-error" role="alert">{error}</p> : null}
				{grouped.map((group) => (
					<div className="secrets-provider" key={group.id} data-testid={`secrets-provider-${group.id}`}>
						<h4>{group.label}</h4>
						{group.connections.length === 0 ? (
							<div className="secrets-empty">
								<span className="secrets-badge">Not registered</span>
								<button type="button" className="settings-secondary-btn" onClick={() => setAdding({ provider: group.id, alias: `Personal ${group.label}`, value: "" })}>
									Add
								</button>
							</div>
						) : group.connections.map((item) => (
							<div className="secrets-row" key={item.id} data-testid={`secret-${item.id}`}>
								<div>
									<strong>{item.alias}</strong>
									<p>
										<span className="secrets-badge secrets-badge-on">{statusLabel(item)}</span>
										{" · "}
										{item.displaySuffix ?? "••••"}
										{" · "}
										{backendLabel(item.backend)}
									</p>
								</div>
								<div className="secrets-row-actions">
									<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void secrets?.test(item.id).then(refresh).catch((reason) => setError(publicError(reason)))}>Test</button>
									<button type="button" className="settings-secondary-btn" onClick={() => setReplacing({ id: item.id, value: "" })}>Replace</button>
									<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void secrets?.delete(item.id).then(refresh).catch((reason) => setError(publicError(reason)))}>Remove</button>
								</div>
							</div>
						))}
					</div>
				))}
				{other.map((item) => (
					<div className="secrets-provider" key={item.id}>
						<h4>{item.provider}</h4>
						<div className="secrets-row" data-testid={`secret-${item.id}`}>
							<div>
								<strong>{item.alias}</strong>
								<p>
									<span className="secrets-badge secrets-badge-on">{statusLabel(item)}</span>
									{" · "}
									{item.displaySuffix ?? "••••"}
									{" · "}
									{backendLabel(item.backend)}
								</p>
							</div>
							<div className="secrets-row-actions">
								<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void secrets?.test(item.id).then(refresh).catch((reason) => setError(publicError(reason)))}>Test</button>
								<button type="button" className="settings-secondary-btn" onClick={() => setReplacing({ id: item.id, value: "" })}>Replace</button>
								<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void secrets?.delete(item.id).then(refresh).catch((reason) => setError(publicError(reason)))}>Remove</button>
							</div>
						</div>
					</div>
				))}

				{adding ? (
					<form className="secrets-form" data-testid="secrets-add-form" onSubmit={(event) => { event.preventDefault(); void add(); }}>
						<label>
							Provider
							<select value={adding.provider} onChange={(event) => setAdding({ ...adding, provider: event.target.value })}>
								{PROVIDERS.map((provider) => <option key={provider.id} value={provider.id}>{provider.label}</option>)}
							</select>
						</label>
						<label>
							Alias
							<input value={adding.alias} onChange={(event) => setAdding({ ...adding, alias: event.target.value })} />
						</label>
						<label>
							Credential
							<input type="password" autoComplete="off" data-testid="secrets-credential-input" value={adding.value} onChange={(event) => setAdding({ ...adding, value: event.target.value })} />
						</label>
						<div className="secrets-row-actions">
							<button type="button" className="settings-secondary-btn" onClick={() => setAdding(null)}>Cancel</button>
							<button type="submit" className="settings-secondary-btn" disabled={busy || !adding.value}>Save</button>
						</div>
					</form>
				) : null}

				{replacing ? (
					<form className="secrets-form" data-testid="secrets-replace-form" onSubmit={(event) => { event.preventDefault(); void replace(); }}>
						<p>Replacement is write-only. Workshop never shows the previous value.</p>
						<label>
							New credential
							<input type="password" autoComplete="off" data-testid="secrets-replace-input" value={replacing.value} onChange={(event) => setReplacing({ ...replacing, value: event.target.value })} />
						</label>
						<div className="secrets-row-actions">
							<button type="button" className="settings-secondary-btn" onClick={() => setReplacing(null)}>Cancel</button>
							<button type="submit" className="settings-secondary-btn" disabled={busy || !replacing.value}>Replace</button>
						</div>
					</form>
				) : null}
			</SettingsCard>

			{inbox.grants.length > 0 ? (
				<SettingsCard title="Agent use requests" testId="secrets-agent-grants">
					<p className="secrets-copy">The agent asked to use a registered connection. Allowing it issues a bounded capability, not the key.</p>
					{inbox.grants.map((grant) => (
						<div className="secrets-row" key={grant.requestId}>
							<div>
								<strong>{grant.alias ?? grant.secretId}</strong>
								<p>{grant.provider} · {grant.recipeId} · {grant.maxCalls} calls · ${grant.maxCostUsd.toFixed(2)}</p>
							</div>
							<div className="secrets-row-actions">
								<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void allowGrant(grant, false)}>Allow once</button>
								<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void allowGrant(grant, true)}>Always for recipe</button>
								<button type="button" className="settings-secondary-btn" disabled={busy} onClick={() => void secrets?.denyUse(grant.secretId).then(refresh)}>Deny</button>
							</div>
						</div>
					))}
				</SettingsCard>
			) : null}

			{review ? (
				<SettingsCard title="Import credentials from .env" testId="secrets-import-review">
					<p className="secrets-copy">Source {review.sourcePath}</p>
					{review.warning ? <p className="secrets-warning">{review.warning}</p> : null}
					<ul className="secrets-import-list">
						{review.candidates.map((candidate: MaskedImportCandidate) => (
							<li key={candidate.variable}>
								<label>
									<input
										type="checkbox"
										checked={selectedVars.includes(candidate.variable)}
										onChange={(event) => setSelectedVars((current) => event.target.checked ? [...current, candidate.variable] : current.filter((name) => name !== candidate.variable))}
									/>
									<span>{candidate.variable}</span>
									<span>{candidate.provider}</span>
									<span>{candidate.masked}</span>
								</label>
							</li>
						))}
					</ul>
					<fieldset className="secrets-after">
						<legend>After import</legend>
						<label><input type="radio" checked={afterImport === "keep"} onChange={() => { setAfterImport("keep"); setConfirmCleanup(false); }} /> Keep file</label>
						<label><input type="radio" checked={afterImport === "replace_aliases"} onChange={() => setAfterImport("replace_aliases")} /> Replace with aliases</label>
						<label><input type="radio" checked={afterImport === "remove_entries"} onChange={() => setAfterImport("remove_entries")} /> Remove entries</label>
					</fieldset>
					{afterImport !== "keep" ? (
						<div className="secrets-cleanup">
							<pre className="secrets-diff">{review.cleanupDiff ?? "Selected variables will be edited. Values are never shown."}</pre>
							<label>
								<input type="checkbox" checked={confirmCleanup} onChange={(event) => setConfirmCleanup(event.target.checked)} />
								I confirm this will edit the file. Workshop will not show the previous values.
							</label>
						</div>
					) : null}
					<div className="secrets-row-actions">
						<button type="button" className="settings-secondary-btn" onClick={() => void denyImport(review.requestId)}>Cancel</button>
						<button type="button" className="settings-secondary-btn" disabled={busy || selectedVars.length === 0 || (afterImport !== "keep" && !confirmCleanup)} onClick={() => void commitImport()}>Import selected</button>
					</div>
				</SettingsCard>
			) : null}

			<SettingsCard title="Active capabilities">
				{capabilities.length === 0 ? <p className="secrets-copy">No run is currently authorized to use a provider through Workshop.</p> : capabilities.map((capability) => (
					<div className="secrets-row" key={capability.id}>
						<div>
							<strong>{capability.provider} · {capability.recipeId}</strong>
							<p>{capability.usedCalls} / {capability.maxCalls} calls · ${capability.usedCostUsd.toFixed(2)} / ${capability.maxCostUsd.toFixed(2)} · {capability.status}</p>
						</div>
						<button type="button" className="settings-secondary-btn" onClick={() => void secrets?.revokeCapability(capability.id).then(refresh)}>Revoke</button>
					</div>
				))}
			</SettingsCard>

			<SettingsCard title="Audit log">
				{audit.length === 0 ? <p className="secrets-copy">No secret events yet.</p> : (
					<ul className="secrets-audit">
						{audit.map((event) => (
							<li key={event.eventId}>
								<span>{event.at}</span>
								<strong>{event.action}</strong>
								<span>{event.decision}</span>
								{event.provider ? <span>{event.provider}</span> : null}
							</li>
						))}
					</ul>
				)}
			</SettingsCard>
		</div>
	);
}
