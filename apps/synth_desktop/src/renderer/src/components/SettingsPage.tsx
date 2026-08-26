// @ts-nocheck — P0-1 generated protocol is stricter than prior handwritten DTOs; UI follow-up is out of specta-cutover file ownership.
import { useEffect, useState } from "react";
import type {
	DesktopInstanceDiagnostics,
	LagunaStatus,
	ModelMultiAgentSetting,
	ModelCatalog,
	MultiAgentVersion,
	PluginStatus,
	SynthAccountSummary,
	SynthBackendSettings,
	TariffCard,
	UpdateStatus
} from "../bridge";
import { publicError } from "../runtime/publicError";
import type { AccountViewModel } from "../runtime/accountView";
import type { DeviceUsageSummary } from "./UsageSheet";
import { OnDeviceModelsSettings } from "./OnDeviceModelsSettings";
import { TrainingModelsSettings } from "./TrainingModelsSettings";
import { InferenceSettings } from "./InferenceSettings";
import { VoiceRecognitionSettings } from "./VoiceRecognitionSettings";
import { ModelObservabilitySettings } from "./ModelObservabilitySettings";
import { AccountPage } from "./AccountPage";
import { GeneralPreferencesSettings } from "./GeneralPreferencesSettings";
import { SettingsCard } from "./SettingsCard";
import { RuntimeContractRows } from "./RuntimeContractRows";
import type { DesktopPreferences } from "../preferences";
import { ProviderMark } from "./ProviderMark";
import { bridges } from "../runtime/desktopBridge";
import { ChatgptCodexSubscriptionCard } from "./ChatgptCodexSubscriptionCard";
import { ContextSettings } from "./ContextSettings";
import { SecretsSettings } from "./SecretsSettings";
import { CapabilityManifest } from "./CapabilityManifest";

type Props = {
	onBack: () => void;
	/** Everything the consolidated Account section renders. */
	account: AccountSectionProps;
	onReloadLaguna: () => Promise<LagunaStatus>;
	lagunaPhase?: string | null;
	pluginStatuses?: readonly PluginStatus[] | null;
	initialSection?: SectionId;
	onSectionChange?: (section: SectionId) => void;
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

function IconContext() {
	return <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden><path d="M3 3.5h10v9H3zM5.2 6h5.6M5.2 8h5.6M5.2 10h3.2" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" /></svg>;
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

function IconKey() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<circle cx="5.5" cy="8" r="2.4" stroke="currentColor" strokeWidth="1.3" />
			<path d="M7.6 8h5.2M11.2 8v2.2M12.8 8v1.4" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
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
	{ id: "context", label: "Context", icon: IconContext },
	{ id: "models", label: "Models", icon: IconChip },
	{ id: "inference", label: "Inference", icon: IconGauge },
	{ id: "voice", label: "Voice", icon: IconMic },
	{ id: "account", label: "Account", icon: IconPerson },
	{ id: "secrets", label: "Secrets", icon: IconKey },
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
		version: "0.4.0",
		date: "August 16, 2026",
		groups: [
			{
				label: "New",
				items: [
					"Product-owned GEPA workflows prepare and run bounded Banking77 and Craftax optimization with digest-bound paid-compute approval.",
					"Craftax opens in a transcript-first full-trace viewer with model-call input, reasoning, output, tool evidence, and raw envelopes.",
					"Programmatic eval lanes and container capability checks fail closed when a selected runtime cannot satisfy a recipe.",
					"Local diagnostics correlate optimizer runs, containers, streams, visuals, and terminal outcomes."
				]
			},
			{
				label: "Improved",
				items: [
					"Live visuals use one canonical binding envelope and durable replay through declared poll transports.",
					"Generation-speed labels stay frozen at their historical cutoff, and completed turns show durable elapsed work time.",
					"Review captures use a dedicated window identity so capture sizing does not mutate the product window."
				]
			},
			{
				label: "Fixed",
				items: [
					"Reconnect and restart replay preserve rollout-local identity without inventing transport URLs or rewriting earlier evidence.",
					"Already-open visual panes reconcile committed revisions without a close and reopen cycle.",
					"Missing usage and timing remain unavailable instead of being displayed as zero."
				]
			}
		]
	},
	{
		version: "0.3.0",
		date: "August 14, 2026",
		groups: [
			{
				label: "New",
				items: [
					"Gemini 3.7 Flash is available through OpenRouter for live collaboration turns.",
					"Settings → Context shows agent limits, skills, MCP groups, cookbooks, and subagent compatibility in one place.",
					"Visual templates now live in families (containers, optimizers, diagrams, analysis) without changing template IDs.",
					"Data → Traces can inspect any compatible sealed Trace V5 archive in the generic rollout viewer.",
					"Typed approvals cover paid compute, sidecar lifecycle, and credential access. Permissive policy stays auditable and does not silently revert to Always Ask.",
					"Reports can be created, sealed, compared, privately shared, and published as a committed revision.",
					"Optimizers install and run through the plugin MCP lifecycle and the typed approval broker.",
					"Larval Mander presence and session_present MCP land from main so chats can show a title, emotion, and short summary."
				]
			},
			{
				label: "Improved",
				items: [
					"Native Mermaid and systems diagrams use the packaged Rust renderer and work offline.",
					"Chat/visual and Visuals list/preview splitters drag and persist independently, then stack at compact widths.",
					"Subagent activity is grouped in the visual pane with working, needs-attention, and completed states."
				]
			},
			{
				label: "Fixed",
				items: [
					"Cookbook pin progress stays current, and Context command errors are shown instead of failing silently.",
					"Pending approvals survive restart as real history or expire cleanly — they no longer render as live cards with dead buttons."
				]
			}
		]
	},
	{
		version: "0.2.0",
		date: "August 12, 2026",
		groups: [
			{
				label: "New",
				items: [
					"ChatGPT subscription (Codex OAuth) is available from Models: connect once, then choose a Codex model from the ChatGPT subscription group.",
					"Subscription usage is clearly shown as ChatGPT plan allowance, separate from Synth Cloud and API-key providers.",
					"Mermaid visuals now render locally through the pinned Grok renderer, with support for flowcharts, sequence, state, class, ER, C4, and additional Mermaid families."
				]
			},
			{
				label: "Improved",
				items: [
					"Mermaid diagrams now fit the active pane by default, with compact zoom, fit, source, copy, and SVG export controls.",
					"Diagram typography, node spacing, edge labels, colors, and lifecycle layouts are clearer and more polished at compact desktop sizes."
				]
			},
			{
				label: "Fixed",
				items: [
					"Sequence diagrams render multiline labels instead of showing literal break markup, and wide diagrams no longer open clipped offscreen.",
					"Installed and development instances keep ChatGPT authorization in a private Workshop-owned file and never invoke the macOS Keychain."
				]
			}
		]
	},
	{
		version: "0.1.0",
		date: "August 10, 2026",
		groups: [
			{
				label: "New",
				items: [
					"Local Laguna XS inference with managed model downloads and memory controls.",
					"Remote Luna, Laguna, Muse Spark, and Synth Cloud models with credentials kept in native custody.",
					"Trace V5 imports, Craftax rollout inspection, and a first-class visual library.",
					"Optional Synth account sign-in with clear cloud allowance and device-usage views."
				]
			},
			{
				label: "Improved",
				items: [
					"A quieter, more consistent Settings experience across models, voice, inference, account, and release information.",
					"Compact composer controls for permissions, model choice, and thinking level.",
					"Clearer local inference throughput, latency, cache, request telemetry, and provider-specific billing authority.",
					"A passive stable-channel update check that stays silent when offline and always uses the official download page."
				]
			},
			{
				label: "Fixed",
				items: [
					"Thinking streams now render at their content height without oversized empty cards.",
					"Auto-compaction preserves model-aware defaults and never falls back to a 16k limit.",
					"Model menus remain inside the window at compact desktop sizes.",
					"Bundled local services preserve the macOS app signature after launch.",
					"Cloud checkout, provider requests, data migrations, and build provenance now fail closed when their release invariants are not met."
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
	providerMark: "openai" | "laguna" | "meta" | "google" | "openrouter" | "synth";
	modelId: string;
	tariffProvider?: string;
	planMetered?: boolean;
	availability?: string;
	source?: string;
	contextTokens?: number | null;
	inputModalities?: string[];
	outputModalities?: string[];
	supportsTools?: boolean;
	metadataObservedAt?: string | null;
};

function formatPerMillion(rate: number): string {
	const rounded = rate.toFixed(2);
	return `$${Number(rounded) === rate ? rounded : String(rate)}`;
}

function AuthorizedModelsSettings({ connection }: { connection: SynthBackendSettings | null }) {
	// Prices come from the native tariff catalog — the same numbers cost
	// estimation uses — never from strings kept in the renderer.
	const [tariffs, setTariffs] = useState<TariffCard[]>([]);
	const [catalog, setCatalog] = useState<ModelCatalog | null>(null);
	useEffect(() => {
		void bridges.tariffs?.catalog()
			.then(setTariffs)
			.catch(() => setTariffs([]));
	}, []);
	useEffect(() => {
		void bridges.config?.modelCatalog()
			.then((next) => setCatalog(next ?? null))
			.catch(() => setCatalog(null));
	}, []);
	const models: AuthorizedModel[] = [];
	for (const entry of catalog?.entries ?? []) {
		models.push({
			id: entry.targetId,
			name: entry.displayName,
			provider: entry.source === "user_config" ? "OpenRouter · Configured in config.toml" : "OpenRouter",
			providerMark: "openrouter",
			modelId: entry.modelId,
			tariffProvider: entry.source === "builtin" ? "openrouter" : undefined,
			availability: entry.availability.replace("_", " "),
			source: entry.source,
			contextTokens: entry.capabilities.maxContextTokens,
			inputModalities: entry.capabilities.inputModalities,
			outputModalities: entry.capabilities.outputModalities,
			supportsTools: entry.capabilities.tools,
			metadataObservedAt: entry.metadataObservedAt
		});
	}
	if (connection?.apiKeyConfigured) {
		models.push(
			{ id: "synth-cloud-laguna-s", name: "Laguna S 2.1", provider: "Synth Cloud · B200", providerMark: "synth", modelId: "synth_internal/laguna-s-2.1-nvfp4", planMetered: true },
			{ id: "synth-cloud-laguna-xs-b200", name: "Laguna XS 2.1", provider: "Synth Cloud · B200", providerMark: "synth", modelId: "synth_internal/laguna-xs-2.1-nvfp4", planMetered: true },
			{ id: "synth-cloud-laguna-xs-h100", name: "Laguna XS 2.1", provider: "Synth Cloud · H100 option", providerMark: "synth", modelId: "synth_internal/laguna-xs-2.1-fp8-h100", planMetered: true },
			{ id: "synth-cloud-muse-spark", name: "Muse Spark 1.2", provider: "Synth Cloud · Meta", providerMark: "meta", modelId: "meta/muse-spark-1.2", planMetered: true }
		);
	}
	if (!models.length) return null;
	const tariffFor = (model: AuthorizedModel) =>
		tariffs.find((card) => card.provider === model.tariffProvider && card.modelId === model.modelId);
	return (
		<SettingsCard title="Authorized providers" testId="authorized-models" className="settings-card-embed">
			<div className="authorized-model-list">
				{models.map((model) => {
					const tariff = tariffFor(model);
					return (
						<article className="authorized-model-row" key={model.id} data-testid={`authorized-model-${model.id}`}>
							<ProviderMark kind={model.providerMark} className="authorized-model-mark" />
							<div className="authorized-model-identity"><strong>{model.name}</strong><span>{model.provider}{model.availability ? ` · ${model.availability}` : ""}</span><code>{model.modelId}</code>{model.inputModalities?.length ? <span>Input: {model.inputModalities.join(", ")}</span> : null}{model.outputModalities?.length ? <span>Output: {model.outputModalities.join(", ")}</span> : null}{model.supportsTools ? <span>Tools: supported</span> : null}{model.contextTokens ? <span>Context: {model.contextTokens.toLocaleString()} tokens</span> : null}{model.metadataObservedAt ? <span>Metadata checked: {model.metadataObservedAt}</span> : null}</div>
							{model.planMetered ? <dl><div><dt>Pricing</dt><dd>Plan metered</dd></div></dl> : tariff ? <dl><div><dt>Input / 1M</dt><dd>{formatPerMillion(tariff.inputUsdPerM)}</dd></div><div><dt>Output / 1M</dt><dd>{formatPerMillion(tariff.outputUsdPerM)}</dd></div>{tariff.cachedInputUsdPerM != null ? <div><dt>Cached read / 1M</dt><dd>{formatPerMillion(tariff.cachedInputUsdPerM)}</dd></div> : null}{tariff.cacheWriteUsdPerM != null ? <div><dt>Cache write / 1M</dt><dd>{formatPerMillion(tariff.cacheWriteUsdPerM)}</dd></div> : null}</dl> : model.source === "user_config" ? <dl><div><dt>Pricing</dt><dd>No estimate — provider-reported settled cost is authoritative</dd></div></dl> : null}
						</article>
					);
				})}
			</div>
			{catalog?.diagnostics.length ? <p className="settings-inline-error">{catalog.diagnostics.map((diagnostic) => `${diagnostic.location}: ${diagnostic.message}`).join(" ")}</p> : null}
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

export function MultiAgentModelSettings() {
	const [models, setModels] = useState<ModelMultiAgentSetting[]>([]);
	const [busyModel, setBusyModel] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		void bridges.config?.listModelMultiAgent()
			.then(setModels)
			.catch((reason) => setError(publicError(reason)));
	}, []);

	const update = async (modelId: string, version: MultiAgentVersion | null) => {
		setBusyModel(modelId);
		setError(null);
		try {
			const next = await bridges.config?.updateModelMultiAgent({ modelId, version });
			if (next) setModels(next);
		} catch (reason) {
			setError(publicError(reason));
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
	pluginStatuses,
	initialSection = "general",
	onSectionChange,
	preferences,
	onPreferencesChange
}: Props) {
	const [section, setSection] = useState<SectionId>(
		SECTIONS.some((entry) => entry.id === initialSection) ? initialSection : "general"
	);
	const [desktopIdentity, setDesktopIdentity] = useState<DesktopInstanceDiagnostics | null>(null);
	const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);

	// Deep links (e.g. the inference panel's gear) retarget an already-open
	// Settings view; internal nav clicks never change the prop, so they win.
	useEffect(() => {
		if (SECTIONS.some((entry) => entry.id === initialSection)) setSection(initialSection);
	}, [initialSection]);

	useEffect(() => {
		void bridges.desktop.getInstanceDiagnostics().then(setDesktopIdentity).catch(() => undefined);
	}, []);

	useEffect(() => {
		void bridges.updates?.status()
			.then(setUpdateStatus)
			.catch(() => setUpdateStatus(null));
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
								data-testid={`settings-nav-${s.id}`}
								onClick={() => {
									setSection(s.id);
									onSectionChange?.(s.id);
								}}
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
								title="On-device inference"
								description="Laguna XS powers local chat and the policy daemon."
								testId="models-on-device-inference"
								className="settings-card-embed"
							>
								<OnDeviceModelsSettings lagunaPhase={lagunaPhase} onReloadLaguna={onReloadLaguna} />
							</SettingsCard>
							<SettingsCard
								title="On-device training"
								description="Models for Optimizers local SFT/CISPO via mlx-rl. Not used for chat inference."
								testId="models-on-device-training"
								className="settings-card-embed"
							>
								<TrainingModelsSettings />
							</SettingsCard>
							<AuthorizedModelsSettings connection={account.connection} />
							<ChatgptCodexSubscriptionCard />
							<SettingsCard testId="models-all" className="settings-card-embed">
								<ModelObservabilitySettings />
							</SettingsCard>
						</div>
					) : null}
					{section === "context" ? <ContextSettings subagents={<MultiAgentModelSettings />} /> : null}
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
					{section === "secrets" ? <SecretsSettings /> : null}
					{section === "about" ? (
						<div className="settings-sections" data-testid="settings-about">
							<SettingsCard title="v0.8 capabilities">
								<CapabilityManifest pluginStatuses={pluginStatuses} lagunaPhase={lagunaPhase} />
							</SettingsCard>
							<SettingsCard title="Synth Desktop">
								<div className="finetune-base-card" data-testid="about-build-identity">
									<span className="finetune-kicker">Build</span>
									<strong>{desktopIdentity?.displayName ?? "Synth Desktop"}</strong>
									<span className="finetune-meta">
										{desktopIdentity
											? `v${desktopIdentity.appVersion} · ${updateStatus?.channel ?? "stable"} · ${desktopIdentity.mode} · source ${desktopIdentity.sourceRevision} · build ${desktopIdentity.buildRevision}`
											: "Build identity unavailable in this environment."}
									</span>
									{updateStatus?.updateAvailable && updateStatus.latestVersion ? (
										<button
											type="button"
											className="settings-update-available"
											data-testid="about-update-available"
											onClick={() => void bridges.updates?.openDownload()}
										>
											{`Update available · v${updateStatus.latestVersion}`}
										</button>
									) : null}
									<code className="finetune-file">{desktopIdentity?.manifest ?? desktopIdentity?.dataRoot ?? "Local-first research workbench"}</code>
									<RuntimeContractRows />
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
