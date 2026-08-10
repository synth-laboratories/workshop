import { useEffect, useState } from "react";
import type {
	DesktopInstanceDiagnostics,
	LagunaStatus,
	ModelMultiAgentSetting,
	MultiAgentVersion,
	SynthAccountSummary,
	SynthBackendSettings,
	TariffCard
} from "../env";
import type { AccountViewModel } from "../runtime/accountView";
import type { DeviceUsageSummary } from "./UsageSheet";
import { OnDeviceModelsSettings } from "./OnDeviceModelsSettings";
import { InferenceSettings } from "./InferenceSettings";
import { VoiceRecognitionSettings } from "./VoiceRecognitionSettings";
import { ModelObservabilitySettings } from "./ModelObservabilitySettings";
import { AccountPage } from "./AccountPage";
import { GeneralPreferencesSettings } from "./GeneralPreferencesSettings";
import { SettingsCard } from "./SettingsCard";
import type { DesktopPreferences } from "../preferences";
import { ProviderMark } from "./ProviderMark";

type Props = {
	onBack: () => void;
	/** Everything the consolidated Account section renders. */
	account: AccountSectionProps;
	onReloadLaguna: () => Promise<LagunaStatus>;
	lagunaPhase?: string | null;
	initialSection?: SectionId;
	preferences?: DesktopPreferences;
	onPreferencesChange?: (prefs: DesktopPreferences) => void;
};

function IconSliders() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path d="M2.5 4.5h7M12.5 4.5h1M2.5 11.5h1M6.5 11.5h7" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
			<circle cx="11" cy="4.5" r="1.7" stroke="currentColor" strokeWidth="1.3" />
			<circle cx="5" cy="11.5" r="1.7" stroke="currentColor" strokeWidth="1.3" />
		</svg>
	);
}

function IconChip() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="4" y="4" width="8" height="8" rx="1.5" stroke="currentColor" strokeWidth="1.3" />
			<path d="M6 1.5v2M10 1.5v2M6 12.5v2M10 12.5v2M1.5 6h2M1.5 10h2M12.5 6h2M12.5 10h2" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
		</svg>
	);
}

function IconGauge() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path d="M2.5 10.5a5.5 5.5 0 1 1 11 0" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
			<path d="M8 10.5 10.8 6.6" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
			<circle cx="8" cy="10.5" r="1.1" fill="currentColor" />
		</svg>
	);
}

function IconMic() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="6" y="1.8" width="4" height="7.4" rx="2" stroke="currentColor" strokeWidth="1.3" />
			<path d="M3.5 7.5a4.5 4.5 0 0 0 9 0M8 12v2.2" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
		</svg>
	);
}

function IconPerson() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<circle cx="8" cy="5.2" r="2.7" stroke="currentColor" strokeWidth="1.3" />
			<path d="M2.8 13.8a5.4 5.4 0 0 1 10.4 0" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
		</svg>
	);
}

function IconInfo() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<circle cx="8" cy="8" r="6.2" stroke="currentColor" strokeWidth="1.3" />
			<path d="M8 7.4v3.4" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
			<circle cx="8" cy="5" r="0.9" fill="currentColor" />
		</svg>
	);
}

function IconChevronLeft() {
	return (
		<svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path d="m10 3.5-4.5 4.5L10 12.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
		</svg>
	);
}

/** Adapter UI is intentionally absent until its full runtime path exists. */
const SECTIONS = [
	{ id: "general", label: "General", icon: IconSliders },
	{ id: "models", label: "Models", icon: IconChip },
	{ id: "inference", label: "Inference", icon: IconGauge },
	{ id: "voice", label: "Voice", icon: IconMic },
	{ id: "account", label: "Account", icon: IconPerson },
	{ id: "about", label: "About", icon: IconInfo }
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

const CHANGELOG = [
	{
		version: "0.1.0",
		date: "August 10, 2026",
		groups: [
			{
				label: "New",
				items: [
					"Local Laguna XS inference with managed model downloads and memory controls.",
					"A first-class right panel for conversation outputs and inference activity.",
					"Credential-aware cloud models with provider and pricing details."
				]
			},
			{
				label: "Improved",
				items: [
					"A quieter, more consistent Settings experience across models, voice, inference, and account.",
					"Compact composer controls for permissions, model choice, and thinking level.",
					"Clearer local inference throughput, latency, cache, and request telemetry."
				]
			},
			{
				label: "Fixed",
				items: [
					"Thinking streams now render at their content height without oversized empty cards.",
					"Auto-compaction preserves model-aware defaults and never falls back to a 16k limit.",
					"Restored memory release controls and removed unfinished navigation and agent stubs."
				]
			}
		]
	}
] as const;

const MULTI_AGENT_CONFIG: Record<MultiAgentVersion, string> = {
	none: "[agents] enabled=false · [features] multi_agent=false · multi_agent_v2=false",
	v1: "[agents] enabled=true · [features] multi_agent=true · multi_agent_v2=false",
	v2: "[agents] enabled=true · [features] multi_agent=true · multi_agent_v2=true"
};

type AuthorizedModel = {
	id: string;
	name: string;
	provider: string;
	providerMark: "openai" | "laguna" | "synth";
	modelId: string;
	inputPrice: string;
	outputPrice: string;
	cachedReadPrice?: string;
	cacheWritePrice?: string;
	planMetered?: boolean;
};

const OPENROUTER_DISPLAY: Record<string, { id: string; name: string; provider: string; providerMark: "openai" | "laguna" }> = {
	"openai/gpt-5.6-luna": { id: "openrouter-luna", name: "GPT 5.6 Luna", provider: "OpenRouter · OpenAI", providerMark: "openai" },
	"poolside/laguna-s-2.1": { id: "openrouter-laguna-s", name: "Laguna S 2.1", provider: "OpenRouter · Poolside", providerMark: "laguna" }
};

function formatUsdPerM(value: number): string {
	return `$${value.toFixed(2)}`;
}

function authorizedModelFromTariff(card: TariffCard): AuthorizedModel | null {
	if (card.provider !== "openrouter") return null;
	const display = OPENROUTER_DISPLAY[card.modelId] ?? {
		id: `openrouter-${card.modelId.replace(/[^a-z0-9]+/gi, "-")}`,
		name: card.modelId,
		provider: "OpenRouter",
		providerMark: "openai" as const
	};
	return {
		id: display.id,
		name: display.name,
		provider: display.provider,
		providerMark: display.providerMark,
		modelId: card.modelId,
		inputPrice: formatUsdPerM(card.inputUsdPerM),
		outputPrice: formatUsdPerM(card.outputUsdPerM),
		cachedReadPrice: card.cachedInputUsdPerM == null ? undefined : formatUsdPerM(card.cachedInputUsdPerM),
		cacheWritePrice: card.cacheWriteUsdPerM == null ? undefined : formatUsdPerM(card.cacheWriteUsdPerM)
	};
}

function AuthorizedModelsSettings({ connection }: { connection: SynthBackendSettings | null }) {
	const [tariffs, setTariffs] = useState<TariffCard[]>([]);
	useEffect(() => {
		void window.synthConfig?.listTariffs().then(setTariffs).catch(() => setTariffs([]));
	}, []);
	const models: AuthorizedModel[] = [];
	if (connection?.openrouterApiKeyConfigured) {
		for (const card of tariffs) {
			const model = authorizedModelFromTariff(card);
			if (model) models.push(model);
		}
	}
	if (connection?.apiKeyConfigured) {
		models.push({ id: "synth-cloud-laguna-s", name: "Laguna S 2.1", provider: "Synth Cloud", providerMark: "synth", modelId: "openrouter/poolside/laguna-s-2.1", inputPrice: "", outputPrice: "", planMetered: true });
	}
	if (!models.length) return null;
	return (
		<SettingsCard title="Authorized providers" testId="authorized-models" className="settings-card-embed">
			<div className="authorized-model-list">
				{models.map((model) => (
					<article className="authorized-model-row" key={model.id} data-testid={`authorized-model-${model.id}`}>
						<ProviderMark kind={model.providerMark} className="authorized-model-mark" />
						<div className="authorized-model-identity"><strong>{model.name}</strong><span>{model.provider}</span><code>{model.modelId}</code></div>
						{model.planMetered ? <dl><div><dt>Pricing</dt><dd>Plan metered</dd></div></dl> : <dl><div><dt>Input / 1M</dt><dd>{model.inputPrice}</dd></div><div><dt>Output / 1M</dt><dd>{model.outputPrice}</dd></div>{model.cachedReadPrice ? <div><dt>Cached read / 1M</dt><dd>{model.cachedReadPrice}</dd></div> : null}{model.cacheWritePrice ? <div><dt>Cache write / 1M</dt><dd>{model.cacheWritePrice}</dd></div> : null}</dl>}
					</article>
				))}
			</div>
		</SettingsCard>
	);
}

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
	lagunaPhase,
	initialSection = "general",
	preferences,
	onPreferencesChange
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

	const activeSection = SECTIONS.find((entry) => entry.id === section) ?? SECTIONS[0];

	return (
		<div className="settings-page" data-testid="settings-page">
			<nav className="settings-rail" aria-label="Settings sections">
				<button type="button" className="desk-back settings-back" aria-label="← Back" onClick={onBack}>
					<IconChevronLeft />
					Back
				</button>
				<h1 className="settings-rail-title">Settings</h1>
				<div className="settings-nav">
					{SECTIONS.map((s) => {
						const Icon = s.icon;
						return (
							<button
								key={s.id}
								type="button"
								className={`settings-nav-item${section === s.id ? " active" : ""}`}
								aria-current={section === s.id ? "page" : undefined}
								onClick={() => setSection(s.id)}
							>
								<Icon />
								<span>{s.label}</span>
							</button>
						);
					})}
				</div>
			</nav>

			<div className="settings-content">
				<div className="settings-content-inner">
					{section !== "account" ? <h2 className="settings-section-title">{activeSection.label}</h2> : null}

					{section === "general" && preferences && onPreferencesChange ? (
						<GeneralPreferencesSettings
							preferences={preferences}
							onPreferencesChange={onPreferencesChange}
						/>
					) : null}
					{section === "models" ? (
						<div className="settings-sections" data-testid="settings-models">
							<SettingsCard
								title="On-device"
								description="Managed local models and inference runtimes."
								testId="models-on-device"
								className="settings-card-embed"
							>
								<OnDeviceModelsSettings lagunaPhase={lagunaPhase} onReloadLaguna={onReloadLaguna} />
							</SettingsCard>
							<AuthorizedModelsSettings connection={account.connection} />
							<SettingsCard testId="models-all" className="settings-card-embed">
								<ModelObservabilitySettings />
								<MultiAgentModelSettings />
							</SettingsCard>
						</div>
					) : null}
					{section === "inference" ? (
						<div className="settings-sections" data-testid="settings-inference">
							<InferenceSettings />
						</div>
					) : null}
					{section === "voice" ? (
						<div className="settings-sections" data-testid="settings-voice">
							<SettingsCard
								title="Voice recognition"
								description="Local Whisper models transcribe dictation from this desktop."
								className="settings-card-embed"
							>
								<VoiceRecognitionSettings />
							</SettingsCard>
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
						<div className="settings-sections" data-testid="settings-about">
							<SettingsCard title="Synth Desktop">
								<div className="finetune-base-card" data-testid="about-build-identity">
									<span className="finetune-kicker">Build</span>
									<strong>{desktopIdentity?.displayName ?? "Synth Desktop"}</strong>
									<span className="finetune-meta">
										{desktopIdentity
											? `v${desktopIdentity.appVersion} · ${desktopIdentity.mode} · source ${desktopIdentity.sourceRevision} · build ${desktopIdentity.buildRevision}`
											: "Build identity unavailable in this environment."}
									</span>
									<code className="finetune-file">{desktopIdentity?.manifest ?? desktopIdentity?.dataRoot ?? "Local-first research workbench"}</code>
								</div>
								<p className="settings-runtime-copy">
									Synth Desktop is a local-first research workbench.
								</p>
							</SettingsCard>
							<SettingsCard title="What’s new" testId="about-changelog">
								<div className="about-changelog">
									{CHANGELOG.map((release) => (
										<article className="about-release" key={release.version}>
											<header>
												<strong>Version {release.version}</strong>
												<time>{release.date}</time>
											</header>
											<div className="about-release-groups">
												{release.groups.map((group) => (
													<section key={group.label}>
														<h4>{group.label}</h4>
														<ul>{group.items.map((item) => <li key={item}>{item}</li>)}</ul>
													</section>
												))}
											</div>
										</article>
									))}
								</div>
							</SettingsCard>
						</div>
					) : null}
				</div>
			</div>
		</div>
	);
}
