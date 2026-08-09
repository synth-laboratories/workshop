/// <reference types="vite/client" />

import type { RuntimeEvent } from "@synth/runtime-protocol";

export {};

type RequestOptions = {
	method?: "GET" | "POST" | "DELETE";
	body?: unknown;
};

type EventSubscription = {
	close(): void;
};

type RuntimeBridge = {
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

type LagunaBridge = {
	getStatus(): Promise<LagunaStatus>;
	onStatus(listener: (status: LagunaStatus) => void): () => void;
};

type SemanticEvalApi = {
	schemaVersion: "synth.desktop-eval-api.v1";
	getState(): unknown;
	listActions(): string[];
	invoke(action: string, argumentsValue?: Record<string, unknown>): Promise<unknown>;
};

declare global {
	interface Window {
		synthDesktop?: {
			platform: string;
			chooseProjectDirectory(): Promise<string | null>;
		};
		synthRuntime: RuntimeBridge;
		synthLaguna?: LagunaBridge;
		__synthEval?: SemanticEvalApi;
	}
}
