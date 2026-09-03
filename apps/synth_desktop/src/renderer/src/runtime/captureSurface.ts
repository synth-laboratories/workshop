/**
 * The renderer half of host surface capture.
 *
 * The host resizes, snapshots its own WKWebView, and restores. All it needs
 * from this side is (a) routing to the surface it was asked to photograph, and
 * (b) an acknowledgement that the surface is actually mounted — a fixed sleep
 * cannot tell a cold React mount from a slow one, and capturing early yields a
 * screenshot of a spinner that looks exactly like a screenshot of a bug.
 *
 * This is deliberately a second protocol alongside `synth:visual-review-capture`
 * rather than a rename of it. Visual review is a certified chain — capture
 * receipts gate `visual_mark_ready` — so it keeps emitting and observing
 * exactly what it did before, and only the new scopes come through here.
 */

export const CAPTURE_EVENT = "synth:capture";

/** Plugin destinations a capture may route to. Mirrors the host's allowlist. */
export const CAPTURE_PLUGIN_IDS = [
	"visuals",
	"reports",
	"experiments",
	"optimizers",
	"inventory",
	"inference",
	"computer-use"
] as const;

export type CapturePluginId = (typeof CAPTURE_PLUGIN_IDS)[number];
export type CaptureScope = "app" | "plugin" | "visual" | "element";

export type CaptureRequest = {
	active?: boolean;
	scope?: CaptureScope;
	target?: string | null;
	/** False for scopes that photograph whatever is already on screen. */
	route?: boolean;
};

type CaptureWindow = Window & { __synthCapture?: CaptureRequest };

export function currentCaptureRequest(): CaptureRequest | undefined {
	return (window as CaptureWindow).__synthCapture;
}

export function isCapturePluginId(value: unknown): value is CapturePluginId {
	return typeof value === "string" && (CAPTURE_PLUGIN_IDS as readonly string[]).includes(value);
}

/**
 * The token the host polls for. Scope and target are both in it so an
 * acknowledgement left over from a previous capture cannot be mistaken for
 * this one — the failure that would otherwise photograph the wrong surface.
 */
export function captureReadyToken(scope: CaptureScope, target: string): string {
	return `${scope}:${target}`;
}

export function markCaptureReady(scope: CaptureScope, target: string): void {
	document.documentElement.dataset.synthCaptureReady = captureReadyToken(scope, target);
}
