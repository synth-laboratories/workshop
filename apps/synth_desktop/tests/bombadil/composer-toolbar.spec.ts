import { actions, always, eventually, extract } from "@antithesishq/bombadil";

/**
 * Composer toolbar geometry from the 2026-08-10 CUA shot:
 *   1. "Never ask · Full system access" wraps so "access" stacks on a second line
 *   2. "Unavailable tok/s observed p50" collides with the Thinking "Max" chip
 *
 * Fixture (run.mjs) seeds allow-all permissions + an implausible throughput
 * sample. This run opens Laguna S (Max) and fuzzes widths — expected RED until
 * the toolbar keeps those controls single-line and non-overlapping.
 *
 * BOMBADIL_SPEC=apps/synth_desktop/tests/bombadil/composer-toolbar.spec.ts \
 *   npm run test:bombadil:composer-toolbar --workspace @synth/synth-desktop
 */
function overlaps(a: DOMRect, b: DOMRect, pad = 1): boolean {
	return !(
		a.right <= b.left + pad ||
		b.right <= a.left + pad ||
		a.bottom <= b.top + pad ||
		b.bottom <= a.top + pad
	);
}

function center(rect: DOMRect | null | undefined) {
	return rect && rect.width > 0 && rect.height > 0
		? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
		: null;
}

const toolbar = extract((state: any) => {
	const document = state.document;
	const viewport = state.window;
	const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
	const permission = document.querySelector<HTMLElement>('[data-testid="approval-mode-select"]');
	const permissionLabel = permission?.querySelector<HTMLElement>("span") ?? null;
	const model = document.querySelector<HTMLElement>('[data-testid="composer-model"]');
	const reasoning = document.querySelector<HTMLElement>('[data-testid="reasoning-effort-select"]');
	const modelMenu = document.querySelector<HTMLElement>('[data-testid="composer-model-menu"]');
	const lagunaOption = document.querySelector<HTMLElement>(
		'[data-testid="composer-model-option-openrouter-laguna-s"]'
	);
	const permissionRect = permission?.getBoundingClientRect() ?? null;
	const permissionLabelRect = permissionLabel?.getBoundingClientRect() ?? null;
	const modelRect = model?.getBoundingClientRect() ?? null;
	const reasoningRect = reasoning?.getBoundingClientRect() ?? null;
	const permissionText = (permissionLabel?.textContent ?? "").replace(/\s+/g, " ").trim();
	const showsFullSystemAccess = /Never ask.*Full system access/i.test(permissionText)
		|| /Never ask.*Full system/i.test(permissionText);
	// getClientRects() reports one rect per wrapped line for inline text.
	const permissionLineCount = permissionLabel
		? Math.max(1, permissionLabel.getClientRects().length)
		: 0;
	const permissionStacksVertically = showsFullSystemAccess && (
		permissionLineCount > 1
		|| Boolean(permissionRect && permissionRect.height > 36)
	);
	const modelOverlapsReasoning = Boolean(
		modelRect && reasoningRect && overlaps(modelRect, reasoningRect, 0)
	);
	return {
		composerReady: Boolean(composer),
		modelPoint: center(modelRect),
		lagunaOptionPoint: center(lagunaOption?.getBoundingClientRect()),
		modelMenuOpen: Boolean(modelMenu),
		showsFullSystemAccess,
		permissionStacksVertically,
		permissionLineCount,
		permissionHeight: permissionRect?.height ?? 0,
		reasoningVisible: Boolean(reasoning),
		modelOverlapsReasoning,
		viewportWidth: viewport.innerWidth
	};
});

export const select_laguna_s_with_max_thinking = actions(() => {
	if (!toolbar.current.composerReady) return ["Wait"];
	if (!toolbar.current.modelMenuOpen && toolbar.current.modelPoint) {
		return [{ Click: { name: "Open model picker for Laguna S", point: toolbar.current.modelPoint } }];
	}
	if (toolbar.current.modelMenuOpen && toolbar.current.lagunaOptionPoint) {
		return [
			{ Click: { name: "Select OpenRouter Laguna S 2.1", point: toolbar.current.lagunaOptionPoint } },
			"Wait"
		];
	}
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1100, height: 700 } },
		{ SetViewport: { width: 1280, height: 840 } },
		{ SetViewport: { width: 1440, height: 900 } }
	];
});

export const toolbar_fixture_reaches_laguna_and_permissions = eventually(() =>
	toolbar.current.showsFullSystemAccess
		&& toolbar.current.reasoningVisible
).within(10, "seconds");

/** CUA: "Never ask · Full system access" must stay one line in the toolbar. */
export const permission_control_never_stacks_full_system_access = always(() =>
	!toolbar.current.showsFullSystemAccess || !toolbar.current.permissionStacksVertically
);

/**
 * CUA: the compact model chip must not paint through the Thinking Max chip.
 */
export const throughput_never_overlaps_thinking_chip = always(() =>
	!toolbar.current.reasoningVisible
		|| !toolbar.current.modelOverlapsReasoning
);
