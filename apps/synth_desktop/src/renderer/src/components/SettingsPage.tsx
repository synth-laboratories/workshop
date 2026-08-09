import { useEffect, useState } from "react";
import type { RuntimeHealth } from "@synth/runtime-protocol";
import type { DesktopInstanceDiagnostics, LagunaStatus, ModelMultiAgentSetting, MultiAgentVersion } from "../env";
import { ModelLocationsSettings } from "./ModelLocationsSettings";
import { BackendSettings } from "./BackendSettings";
import { LegacyMigrationSettings } from "./LegacyMigrationSettings";
import { WorkspaceAccessSettings } from "./WorkspaceAccessSettings";

type Props = {
	onBack: () => void;
	onReloadLaguna: () => Promise<LagunaStatus>;
	health?: RuntimeHealth | null;
	lagunaPhase?: string | null;
	initialSection?: SectionId;
};

/** Adapter UI is intentionally absent until its full runtime path exists. */
const SECTIONS = [
	{ id: "models", label: "Models" },
	{ id: "runtime", label: "Runtime" },
	{ id: "account", label: "Account" }
] as const;

type SectionId = (typeof SECTIONS)[number]["id"];

const MULTI_AGENT_OPTIONS: Array<{ value: MultiAgentVersion; label: string }> = [
	{ value: "none", label: "None" },
	{ value: "v1", label: "V1" },
	{ value: "v2", label: "V2" }
];

const MULTI_AGENT_CONFIG: Record<MultiAgentVersion, string> = {
	none: "[agents] enabled=false · [features] multi_agent=false · multi_agent_v2=false",
	v1: "[agents] enabled=true · [features] multi_agent=true · multi_agent_v2=false",
	v2: "[agents] enabled=true · [features] multi_agent=true · multi_agent_v2=true"
};

function multiAgentOverrideWarning(model: ModelMultiAgentSetting): string | null {
	if (model.effective === model.preset) return null;
	if (model.effective === "none") {
		return `Override disables the model’s ${model.preset.toUpperCase()} multi-agent preset and removes all Codex collaboration tools from new sessions.`;
	}
	if (model.effective === "v1") {
		return model.preset === "v2"
			? "Override writes the V1 feature flags, but Codex’s built-in V2 model metadata can take precedence for an exact Sol/Terra slug. Provider-qualified custom slugs use V1; existing threads keep the version pinned on their first turn."
			: "Override exposes the V1 namespaced collaboration tools to a model with no compatibility preset. V1 does not use V2 encrypted message or tool payloads.";
	}
	return "Override exposes V2 direct collaboration tools, agent-message routing, and encrypted message/tool payloads. Models or Responses-compatible providers without V2 support may reject the request or fail to read delegated tasks.";
}

function MultiAgentModelSettings() {
	const [models, setModels] = useState<ModelMultiAgentSetting[]>([]);
	const [busyModel, setBusyModel] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		void window.synthConfig?.listModelMultiAgent()
			.then(setModels)
			.catch((reason) => setError(String(reason)));
	}, []);

	const update = async (modelId: string, version: MultiAgentVersion | null) => {
		setBusyModel(modelId);
		setError(null);
		try {
			const next = await window.synthConfig?.updateModelMultiAgent({ modelId, version });
			if (next) setModels(next);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
		} finally {
			setBusyModel(null);
		}
	};

	return (
		<section className="model-capabilities" data-testid="model-multi-agent-settings">
			<header className="model-capabilities-head">
				<h3>Multi-agent compatibility</h3>
				<p>Presets follow the model family across providers. Overrides apply to new Codex app-server sessions.</p>
			</header>
			{error ? <div className="model-locations-error">{error}</div> : null}
			<div className="model-capability-list">
				{models.map((model) => {
					const forced = model.effective !== model.preset;
					const warning = multiAgentOverrideWarning(model);
					return (
						<div className={`model-capability-row${forced ? " forced" : ""}`} key={model.modelId}>
							<div className="model-capability-copy">
								<strong>{model.displayName}</strong>
								<code>{model.modelId}</code>
								<span>Preset: {model.preset.toUpperCase()}{forced ? " · advanced override" : ""}</span>
								<span className="model-capability-config">Writes: <code>{MULTI_AGENT_CONFIG[model.effective]}</code></span>
							</div>
							<div className="model-capability-controls" role="group" aria-label={`${model.displayName} multi-agent compatibility`}>
								{MULTI_AGENT_OPTIONS.map((option) => (
									<button
										type="button"
										key={option.value}
										className={model.effective === option.value ? "active" : ""}
										disabled={busyModel === model.modelId}
										onClick={() => void update(model.modelId, option.value)}
									>{option.label}</button>
								))}
								{model.overridden ? <button type="button" className="model-capability-reset" onClick={() => void update(model.modelId, null)}>Reset</button> : null}
							</div>
							{warning ? <p className="model-capability-warning">{warning}</p> : null}
						</div>
					);
				})}
			</div>
		</section>
	);
}

export function SettingsPage({ onBack, onReloadLaguna, health, lagunaPhase, initialSection = "models" }: Props) {
	const [section, setSection] = useState<SectionId>(initialSection);
	const [desktopIdentity, setDesktopIdentity] = useState<DesktopInstanceDiagnostics | null>(null);
	const [reloadState, setReloadState] = useState<"idle" | "reloading" | "ready" | "error">("idle");
	const [reloadDetail, setReloadDetail] = useState<string | null>(null);

	useEffect(() => {
		void window.synthDesktop.getInstanceDiagnostics().then(setDesktopIdentity).catch(() => undefined);
	}, []);

	const reloadLaguna = async () => {
		setReloadState("reloading");
		setReloadDetail("Reloading Laguna XS…");
		try {
			const status = await onReloadLaguna();
			setReloadState("ready");
			setReloadDetail(status.detail ?? "Laguna XS is ready.");
		} catch (reason) {
			setReloadState("error");
			setReloadDetail(reason instanceof Error ? reason.message : String(reason));
		}
	};

	return (
		<div className="settings-page" data-testid="settings-page">
			<header className="settings-top">
				<button type="button" className="desk-back" onClick={onBack}>
					← Back
				</button>
				<h1>Settings</h1>
			</header>

			<div className="settings-body">
				<nav className="settings-nav" aria-label="Settings sections">
					{SECTIONS.map((s) => (
						<button
							key={s.id}
							type="button"
							className={`settings-nav-item${section === s.id ? " active" : ""}`}
							onClick={() => setSection(s.id)}
						>
							{s.label}
						</button>
					))}
				</nav>

				<div className="settings-content">
					{section === "models" ? (
						<div className="settings-finetunes" data-testid="settings-models">
							<header className="settings-section-head">
								<div>
									<h2>Models</h2>
									<p>Local Metal residency, remote providers, and Intern routing.</p>
								</div>
									<div className="settings-reload-control">
										<button type="button" className="settings-secondary-btn" onClick={() => void reloadLaguna()} disabled={reloadState === "reloading"}>
											{reloadState === "reloading" ? "Reloading…" : "Reload"}
										</button>
										{reloadState !== "idle" ? (
											<p data-testid="laguna-reload-status" role={reloadState === "error" ? "alert" : "status"} data-state={reloadState}>
												{reloadState === "reloading" && lagunaPhase ? `Laguna ${lagunaPhase}…` : reloadDetail}
											</p>
										) : null}
									</div>
							</header>
							<div className="finetune-base-card">
								<span className="finetune-kicker">Local base</span>
								<strong>Laguna XS 2.1 · NVFP4</strong>
								<span className="finetune-meta">{health?.local.mode === "mlx" ? "MLX / Metal resident" : "Stub boundary"} · {lagunaPhase ?? "unknown"}</span>
								<span className="finetune-file">{health?.local.modelPath ?? "See discovered local model locations below"}</span>
							</div>
							<div className="settings-runtime-grid">
								<div><span>OpenRouter</span><strong>{health?.openrouter.mode ?? "connecting"}</strong></div>
								<div><span>Intern</span><strong>{health?.intern.mode ?? "connecting"}</strong></div>
							</div>
							<ModelLocationsSettings />
							<MultiAgentModelSettings />
						</div>
					) : null}
					{section === "runtime" ? (
						<div className="settings-finetunes" data-testid="settings-runtime">
							<h2>Runtime</h2>
							<p className="settings-runtime-copy">One append-only local authority owns sessions, runs, events, approvals, traces, visuals, and usage. The UI can inspect the store without leaving the workbench.</p>
							<div className="finetune-base-card" data-testid="desktop-build-identity">
								<span className="finetune-kicker">Desktop identity</span>
								<strong>{desktopIdentity?.displayName ?? "Reading running build…"}</strong>
								<span className="finetune-meta">
									{desktopIdentity
										? `v${desktopIdentity.appVersion} · ${desktopIdentity.mode} · source ${desktopIdentity.sourceRevision} · build ${desktopIdentity.buildRevision}`
										: "The running process will report its exact source and build revision."}
								</span>
								<code className="finetune-file">
									{desktopIdentity ? `PID ${desktopIdentity.processId} · ${desktopIdentity.executable}` : "Waiting for desktop diagnostics"}
								</code>
								<code className="finetune-file">{desktopIdentity?.manifest ?? desktopIdentity?.dataRoot ?? ""}</code>
							</div>
							<div className="finetune-base-card">
								<span className="finetune-kicker">Data store</span>
								<strong>{health?.dataStore?.events ?? 0} events · {health?.dataStore?.runs ?? 0} runs</strong>
								<span className="finetune-meta">{health?.dataStore?.projects ?? 0} projects · {health?.dataStore?.usage ?? 0} usage entries</span>
								<span className="finetune-file">{health?.dataStore?.path ?? "Runtime is connecting"}</span>
							</div>
							<div className="finetune-base-card">
								<span className="finetune-kicker">Intern routing</span>
								<strong>
									{health?.intern.mode === "remote"
										? "Cloud mailbox connected"
										: health?.intern.mode === "demo"
											? "Explicit demo mailbox"
											: "Setup required"}
								</strong>
								<span className="finetune-meta">
									{health?.intern.mode === "remote"
										? "Sync and Async mirror the cloud Intern through the Rust runtime."
										: health?.intern.mode === "demo"
											? "Scripted local events are active; no cloud mailbox is contacted."
											: "Choose cloud credentials or explicit demo mode before starting Intern."}
								</span>
								<code className="finetune-file">
									{health?.intern.mode === "unconfigured"
										? "Settings → Account → Synth backend"
										: health?.intern.mode === "demo"
											? "SYNTH_INTERN_DEMO=1 npm run dev:desktop"
											: health?.intern.backendUrl ?? "Cloud endpoint configured"}
								</code>
							</div>
							<WorkspaceAccessSettings />
							<LegacyMigrationSettings />
						</div>
					) : null}
					{section === "account" ? (
						<BackendSettings />
					) : null}
				</div>
			</div>
		</div>
	);
}
