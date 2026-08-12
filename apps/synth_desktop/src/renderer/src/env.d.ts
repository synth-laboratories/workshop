/// <reference types="vite/client" />

/**
 * Window declarations only (Wave 2b). Shared DTOs live in `bridge/types.ts`
 * and protocol types in `@synth/runtime-protocol`.
 */

import type {
	CodexBridge,
	CoreBridge,
	DesktopInstanceDiagnostics,
	InternBridge,
	InventoryBridge,
	LagunaBridge,
	ModelPerformanceBridge,
	OptimizersBridge,
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
		synthConfig?: SynthConfigBridge;
		synthWorkspaceScope?: WorkspaceScopeBridge;
		synthAccount?: SynthAccountBridge;
		synthCodex?: CodexBridge;
		synthCore?: CoreBridge;
		synthIntern?: InternBridge;
		synthInventory?: InventoryBridge;
		synthModelPerformance?: ModelPerformanceBridge;
		synthUsage?: UsageBridge;
		synthTariffs?: TariffsBridge;
		synthUpdates?: UpdatesBridge;
		synthVisuals?: VisualsBridge;
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
