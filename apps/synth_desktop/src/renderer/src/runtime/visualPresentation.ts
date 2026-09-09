/** One host-to-renderer presentation intent, shared by the shell and library. */
export type VisualPresentation = { visualId: string; requestId: string };

export function presentWorkspaceVisual(visualId:string):void{
	const intent={visualId,requestId:crypto.randomUUID()};
	window.__workshopVisualPresentation=intent;
	window.dispatchEvent(new CustomEvent("workshop:visual-present",{detail:intent}));
}

declare global {
	interface Window {
		__workshopVisualPresentation?: VisualPresentation;
	}
}

export function subscribeVisualPresentation(apply: (intent: VisualPresentation) => void): () => void {
	const receive = (value: unknown) => {
		if (!value || typeof value !== "object") return;
		const intent = value as Partial<VisualPresentation>;
		if (typeof intent.visualId === "string" && intent.visualId && typeof intent.requestId === "string" && intent.requestId) {
			apply(intent as VisualPresentation);
		}
	};
	const listener = (event: Event) => receive((event as CustomEvent).detail);
	window.addEventListener("workshop:visual-present", listener);
	receive(window.__workshopVisualPresentation);
	return () => window.removeEventListener("workshop:visual-present", listener);
}

/** Retain an intent across mounting, then stop replaying it on later visits. */
export function completeVisualPresentation(requestId: string): void {
	if (window.__workshopVisualPresentation?.requestId === requestId) {
		delete window.__workshopVisualPresentation;
	}
}
