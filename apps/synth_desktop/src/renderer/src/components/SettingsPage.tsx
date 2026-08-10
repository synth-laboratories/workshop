import { useEffect, useState } from "react";
import type { RuntimeHealth } from "@synth/runtime-protocol";
import type {
	DesktopInstanceDiagnostics,
	LagunaStatus,
	ModelMultiAgentSetting,
	MultiAgentVersion,
	SynthAccountSummary,
	SynthBackendSettings
} from "../env";
import type { AccountViewModel } from "../runtime/accountView";
import type { DeviceUsageSummary } from "./UsageSheet";
import { OnDeviceModelsSettings } from "./OnDeviceModelsSettings";
import { InferenceSettings } from "./InferenceSettings";
import { VoiceRecognitionSettings } from "./VoiceRecognitionSettings";
import { ModelObservabilitySettings } from "./ModelObservabilitySettings";
import { AccountPage } from "./AccountPage";
import { WorkspaceAccessSettings } from "./WorkspaceAccessSettings";
import { GeneralPreferencesSettings } from "./GeneralPreferencesSettings";
import type { DesktopPreferences } from "../preferences";

type Props = {
	onBack: () => void;
	/** Everything the consolidated Account section renders. */
	account: AccountSectionProps;
	onReloadLaguna: () => Promise<LagunaStatus>;
	health?: RuntimeHealth | null;
	lagunaStatus?: LagunaStatus | null;
	initialSection?: SectionId;
	preferences?: DesktopPreferences;
	onPreferencesChange?: (prefs: DesktopPreferences) => void;
	conversationTitles?: Record<string, string>;
	onUnarchiveConversation?: (id: string) => void;
	onOpenConversation?: (id: string) => void;
};

/** Adapter UI is intentionally absent until its full runtime path exists. */
const SECTIONS = [
	{ id: "general", label: "General" },
	{ id: "models", label: "Models" },
	{ id: "inference", label: "Inference" },
	{ id: "voice", label: "Voice" },
	{ id: "runtime", label: "Runtime" },
	{ id: "account", label: "Account" },
	{ id: "about", label: "About" }
] as const;

export type SectionId = (typeof SECTIONS)[number]["id"];

export type AccountSectionProps = {
	view: AccountViewModel;
	summary: SynthAccountSummary | null;
	deviceUsage: DeviceUsageSummary | null;
	connection: SynthBackendSettings | null;
	onBilling: (action: "upgrade" | "manage") => void;
	onRefresh: () => void;
	onOpenDeviceUsage: () => void;
};

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
				<h4>Multi-agent compatibility</h4>
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

export function SettingsPage({
	onBack,
	account,
	onReloadLaguna,
	health,
	lagunaStatus,
	initialSection = "general",
	preferences,
	onPreferencesChange,
	conversationTitles,
	onUnarchiveConversation,
	onOpenConversation
}: Props) {
	const [section, setSection] = useState<SectionId>(
		SECTIONS.some((entry) => entry.id === initialSection) ? initialSection : "general"
	);
	const [desktopIdentity, setDesktopIdentity] = useState<DesktopInstanceDiagnostics | null>(null);

	// Deep links (e.g. the inference panel's gear) retarget an already-open
	// Settings view; internal nav clicks never change the prop, so they win.
	useEffect(() => {
		if (SECTIONS.some((entry) => entry.id === initialSection)) setSection(initialSection);
	}, [initialSection]);

	useEffect(() => {
		void window.synthDesktop.getInstanceDiagnostics().then(setDesktopIdentity).catch(() => undefined);
	}, []);

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
					{section === "general" && preferences && onPreferencesChange ? (
						<GeneralPreferencesSettings
							preferences={preferences}
							onPreferencesChange={onPreferencesChange}
							conversationTitles={conversationTitles}
							onUnarchive={onUnarchiveConversation}
							onOpenConversation={onOpenConversation}
						/>
					) : null}
					{section === "models" ? (
						<div className="settings-finetunes" data-testid="settings-models">
							<header className="settings-section-head">
								<div>
									<p className="settings-breadcrumb">Settings → Models</p>
									<h2>Models</h2>
									<p>On-device model weights, telemetry, and compatibility for every provider.</p>
								</div>
							</header>
							<section className="models-half" data-testid="models-on-device">
								<header className="models-half-head">
									<h3>On-device</h3>
									<p>Managed local models and inference runtimes for Workshop coding agents.</p>
								</header>
								<OnDeviceModelsSettings lagunaStatus={lagunaStatus} onReloadLaguna={onReloadLaguna} />
							</section>
							<section className="models-half models-half-all" data-testid="models-all">
								<header className="models-half-head">
									<h3>All</h3>
									<p>Observability and multi-agent compatibility across local and cloud models.</p>
								</header>
								<ModelObservabilitySettings />
								<MultiAgentModelSettings />
							</section>
						</div>
					) : null}
					{section === "inference" ? (
						<div className="settings-finetunes" data-testid="settings-inference">
							<header className="settings-section-head">
								<div>
									<p className="settings-breadcrumb">Settings → Inference</p>
									<h2>Inference</h2>
									<p>Daemon-side defaults for sampling, reasoning, and runtime residency.</p>
								</div>
							</header>
							<InferenceSettings />
						</div>
					) : null}
					{section === "voice" ? (
						<div className="settings-finetunes" data-testid="settings-voice">
							<header className="settings-section-head">
								<div>
									<p className="settings-breadcrumb">Settings → Voice Recognition</p>
									<h2>Voice Recognition</h2>
									<p>Local Whisper models that transcribe dictation from this desktop.</p>
								</div>
							</header>
							<VoiceRecognitionSettings />
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
							<div className="finetune-base-card" data-testid="intern-routing">
								<span className="finetune-kicker">Intern routing · [alpha]</span>
								<strong>Deferred to v0.2</strong>
								<span className="finetune-meta">
									v0.1 does not claim a live Sync/Async cloud mailbox. Proper Intern cloud
									routing returns when public backend contracts ship; internal or unfinished
									endpoints are not shown as connected.
								</span>
								<code className="finetune-file">See launch_v0p1.md · Intern [alpha] → v0.2</code>
							</div>
							<WorkspaceAccessSettings />
						</div>
					) : null}
					{section === "account" ? (
						<AccountPage
							view={account.view}
							summary={account.summary}
							deviceUsage={account.deviceUsage}
							connection={account.connection}
							onBilling={account.onBilling}
							onRefresh={account.onRefresh}
							onOpenDeviceUsage={account.onOpenDeviceUsage}
						/>
					) : null}
					{section === "about" ? (
						<div className="settings-finetunes" data-testid="settings-about">
							<header className="settings-section-head">
								<div>
									<h2>About</h2>
									<p>Version, build identity, and changelog entry points.</p>
								</div>
							</header>
							<div className="finetune-base-card" data-testid="about-build-identity">
								<span className="finetune-kicker">Synth Desktop</span>
								<strong>{desktopIdentity?.displayName ?? "Synth Desktop"}</strong>
								<span className="finetune-meta">
									{desktopIdentity
										? `v${desktopIdentity.appVersion} · ${desktopIdentity.mode} · source ${desktopIdentity.sourceRevision} · build ${desktopIdentity.buildRevision}`
										: "Build identity unavailable in this environment."}
								</span>
								<code className="finetune-file">{desktopIdentity?.manifest ?? desktopIdentity?.dataRoot ?? "Local-first research workbench"}</code>
							</div>
							<p className="settings-runtime-copy">
								Synth Desktop is a local-first research workbench. Release notes and acknowledgements ship with each desktop build.
							</p>
						</div>
					) : null}
				</div>
			</div>
		</div>
	);
}
