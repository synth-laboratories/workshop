/// <reference types="vite/client" />

/**
 * Window declarations only (Wave 2b). Shared DTOs live in `bridge/types.ts`
 * and protocol types in `@synth/runtime-protocol`.
 */

import type {
	CodexBridge,
	CodexOauthBridge,
	ContextBridge,
	CoreBridge,
	DesktopInstanceDiagnostics,
	InternBridge,
	InventoryBridge,
	LagunaBridge,
	TrainingModelsBridge,
	TrainingArtifactsBridge,
	ModelPerformanceBridge,
	OptimizersBridge,
	ComputerUseBridge,
	BrowserAdminBridge,
	PluginsBridge,
	ProductTelemetryBridge,
	ReleaseTier,
	ReleaseTierBridge,
	RuntimeBridge,
	SemanticEvalApi,
	SkillsBridge,
	SynthAccountBridge,
	SynthConfigBridge,
	ProjectSourcesBridge,
	TariffsBridge,
	TerminalBridge,
	UpdatesBridge,
	UsageBridge,
	VisualsBridge,
	ReportsBridge,
	RegisteredInstance,
	WhisperBridge,
	WorkspaceScopeBridge,
	ComposerImageAttachment,
	SecretsBridge
} from "./bridge/types";

export {};

declare global {
	/** Build-tier constants injected by vite.config.ts `define`. Gate code on
	 * the `__TIER_HAS_*__` booleans directly for structural elimination from
	 * narrower bundles; see flags/tier.ts. */
	const __WORKSHOP_TIER__: ReleaseTier;
	const __TIER_HAS_BETA__: boolean;
	const __TIER_HAS_ALPHA__: boolean;
	const __TIER_HAS_DEV__: boolean;

	interface Window {
		synthDesktop: {
			platform: string;
			chooseWorkspaceDirectory(): Promise<string | null>;
			chooseImageFiles(): Promise<ComposerImageAttachment[]>;
			getInstanceDiagnostics(): Promise<DesktopInstanceDiagnostics>;
			getInstances(): Promise<RegisteredInstance[]>;
		};
		/** Browser fixture/explicit compatibility bridge; not installed by Tauri. */
		synthRuntime?: RuntimeBridge;
		synthLaguna?: LagunaBridge;
		synthTrainingModels?: TrainingModelsBridge;
		synthTrainingArtifacts?: TrainingArtifactsBridge;
		synthWhisper?: WhisperBridge;
		synthSkills?: SkillsBridge;
		synthContext?: ContextBridge;
		synthConfig?: SynthConfigBridge;
		synthWorkspaceScope?: WorkspaceScopeBridge;
		synthProjectSources?: ProjectSourcesBridge;
		synthAccount?: SynthAccountBridge;
		synthCodex?: CodexBridge;
		synthCodexOauth?: CodexOauthBridge;
		synthCore?: CoreBridge;
		synthIntern?: InternBridge;
		synthInventory?: InventoryBridge;
		synthModelPerformance?: ModelPerformanceBridge;
		synthUsage?: UsageBridge;
		synthTariffs?: TariffsBridge;
		synthUpdates?: UpdatesBridge;
		synthVisuals?: VisualsBridge;
		synthPlugins?: PluginsBridge;
		synthComputerUse?: ComputerUseBridge;
		synthBrowserAdmin?: BrowserAdminBridge;
		synthReports?: ReportsBridge;
		synthOptimizers?: OptimizersBridge;
		synthTerminal: TerminalBridge;
		synthSecrets?: SecretsBridge;
		synthTelemetry?: ProductTelemetryBridge;
		synthReleaseTier?: ReleaseTierBridge;
		/** Dev/test semantic eval API — tree-shaken from packaged production builds. */
		__synthEval?: SemanticEvalApi;
		__synthPreferences?: {
			get(): unknown;
			set(raw: unknown): unknown;
			reset(): unknown;
			storageKey: string;
		};
	}
}
