/// <reference types="vite/client" />

import type { RuntimeEvent } from "@synth/runtime-protocol";

export {};

export type RequestOptions = {
	method?: "GET" | "POST" | "DELETE";
	body?: unknown;
};

export type EventSubscription = {
	close(): void;
};

export type RuntimeBridge = {
	request<T = unknown>(path: string, options?: RequestOptions): Promise<T>;
	subscribe(
		sessionId: string,
		afterSequence: number,
		onEvent: (event: RuntimeEvent) => void,
		onStatus?: (status: { state: string; detail?: string }) => void
	): Promise<EventSubscription>;
};

export type LagunaPhase =
	| "unknown"
	| "starting"
	| "loading"
	| "ready"
	| "error"
	| "unavailable";

export type LagunaStatus = {
	phase: LagunaPhase;
	baseUrl: string | null;
	backend: string | null;
	loadedModel: string | null;
	detail: string | null;
	memoryBytes: number | null;
	updatedAt: number;
};

export type LagunaBridge = {
	getStatus(): Promise<LagunaStatus>;
	onStatus(listener: (status: LagunaStatus) => void): () => void;
};

export type CodexSessionStart = {
	sessionId: string;
	workspace: string;
	baseUrl: string;
	apiKey: string;
	model: string;
	providerName: string;
	providerTitle: string;
	providerEnvKey: string;
	approvalPolicy?: string;
	sandbox?: string;
	threadId?: string;
};

export type CodexSessionInfo = { sessionId: string; threadId: string; turnId?: string | null };
export type PersistedCodexSession = {
	sessionId: string;
	threadId: string;
	workspace: string;
	model: string;
	providerName: string;
	providerTitle: string;
	baseUrl: string;
	status: string;
};
export type CodexEvent = { sessionId: string; method: string; params: Record<string, unknown> };
export type CodexBridge = {
	defaultWorkspace(): Promise<string>;
	list(): Promise<PersistedCodexSession[]>;
	start(request: CodexSessionStart): Promise<CodexSessionInfo>;
	startTurn(sessionId: string, prompt: string): Promise<CodexSessionInfo>;
	interrupt(sessionId: string): Promise<void>;
	close(sessionId: string): Promise<void>;
	onEvent(listener: (event: CodexEvent) => void): () => void;
};

export type TerminalInfo = { id: string; workspaceId: string; cwd: string; shell: string; title: string; status: "running" | "exited" | "failed"; createdAt: number; exitCode?: number | null };
export type TerminalEvent = { terminalId: string; sequence: number; kind: "output" | "exit" | "error"; dataBase64?: string | null; exitCode?: number | null; message?: string | null };
export type TerminalCreateRequest = { workspaceId: string; workspaceRoot: string; cwd?: string; cols?: number; rows?: number };
export type TerminalBridge = {
	available: boolean;
	create(request: TerminalCreateRequest): Promise<TerminalInfo>;
	list(workspaceId?: string): Promise<TerminalInfo[]>;
	snapshot(terminalId: string, afterSequence?: number): Promise<TerminalEvent[]>;
	write(terminalId: string, data: string): Promise<void>;
	resize(terminalId: string, cols: number, rows: number): Promise<void>;
	close(terminalId: string): Promise<void>;
	onEvent(listener: (event: TerminalEvent) => void): () => void;
};

type SemanticEvalApi = {
	schemaVersion: "synth.desktop-eval-api.v1";
	getState(): unknown;
	listActions(): string[];
	invoke(action: string, argumentsValue?: Record<string, unknown>): Promise<unknown>;
};

declare global {
	interface Window {
		synthDesktop: {
			platform: string;
			chooseProjectDirectory(): Promise<string | null>;
		};
		synthRuntime: RuntimeBridge;
		synthLaguna?: LagunaBridge;
		synthCodex?: CodexBridge;
		synthTerminal: TerminalBridge;
		__synthEval?: SemanticEvalApi;
	}
}
