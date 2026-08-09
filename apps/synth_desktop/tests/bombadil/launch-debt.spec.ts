import { actions, always, extract } from "@antithesishq/bombadil";

/**
 * Deliberately-red launch backpressure.  Run with:
 *
 * BOMBADIL_SPEC=apps/synth_desktop/tests/bombadil/launch-debt.spec.ts \
 *   npm run test:bombadil --workspace @synth/synth-desktop
 *
 * The bridge injects the persisted CUA payload.  This test must fail until
 * analysis.visual.v1 accepts the agent-authored `type` block format (or the
 * registry rejects it before it becomes a renderable visual).
 */
const visuals = extract((state: any) => {
	const button = state.document.querySelector<HTMLElement>('[data-testid="open-visuals"]');
	const rect = button?.getBoundingClientRect();
	return {
		openPoint: rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : null,
		pageVisible: Boolean(state.document.querySelector('[data-testid="visuals-page"]')),
		invalidVisible: Boolean(state.document.querySelector('[data-testid="visual-invalid"]')),
		analysisVisible: Boolean(state.document.querySelector('[data-testid="visual-analysis-spec"]'))
	};
});

export const open_cua_analysis_visual = actions(() =>
	visuals.current.openPoint ? [
		{ Click: { name: "Open Visuals with CUA analysis payload", point: visuals.current.openPoint } },
		"Wait"
	] : ["Wait"]
);

export const persisted_analysis_visual_never_hits_the_error_boundary = always(() =>
	!visuals.current.pageVisible || (!visuals.current.invalidVisible && visuals.current.analysisVisible)
);
