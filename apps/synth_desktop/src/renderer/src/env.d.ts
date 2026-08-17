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
	ModelPerformanceBridge,
	OptimizersBridge,
	ComputerUseBridge,
	PluginsBridge,
	RuntimeBridge,
	SemanticEvalApi,
	SkillsBridge,
	SynthAccountBridge,
	SynthConfigBridge,
	TariffsBridge,
	TerminalBridge,
	UpdatesBridge,
	UsageBridge,
	VisualsBridge,
	ReportsBridge,
	WhisperBridge,
	WorkspaceScopeBridge,
	ComposerImageAttachment
} from "./bridge/types";

export {};

declare global {
	interface Window {
		synthDesktop: {
			platform: string;
			chooseWorkspaceDirectory(): Promise<string | null>;
			chooseImageFiles(): Promise<ComposerImageAttachment[]>;
			getInstanceDiagnostics(): Promise<DesktopInstanceDiagnostics>;
		};
		/** Browser fixture/explicit compatibility bridge; not installed by Tauri. */
		synthRuntime?: RuntimeBridge;
		synthLaguna?: LagunaBridge;
		synthWhisper?: WhisperBridge;
		synthSkills?: SkillsBridge;
		synthContext?: ContextBridge;
		synthConfig?: SynthConfigBridge;
		synthWorkspaceScope?: WorkspaceScopeBridge;
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
		synthReports?: ReportsBridge;
		synthOptimizers?: OptimizersBridge;
		synthTerminal: TerminalBridge;
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
