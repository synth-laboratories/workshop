import { actions, always, eventually, extract } from "@antithesishq/bombadil";

/**
 * Exhaustive composer-toolbar geometry matrix.
 *
 * The directed action walks every selectable model and every supported
 * Thinking/Reasoning x Speed combination at the constrained chat width. The
 * fixture exposes all provider credentials and a ready local model so coverage
 * cannot pass by silently skipping disabled model rows.
 *
 * BOMBADIL_SPEC=apps/synth_desktop/tests/bombadil/composer-toolbar.spec.ts \
 *   npm run test:bombadil:composer-toolbar --workspace @synth/synth-desktop
 */
type MatrixModel = {
	targetId: string;
	reasoning: string[];
	speed: string[];
};

const MODELS: MatrixModel[] = [
	{ targetId: "local-laguna", reasoning: ["Minimal", "Max"], speed: [""] },
	{ targetId: "openrouter-luna", reasoning: ["Low", "Medium", "High", "XHigh", "Max"], speed: [""] },
	{ targetId: "openrouter-laguna-s", reasoning: ["None", "Max"], speed: [""] },
	{ targetId: "openrouter-muse-spark", reasoning: ["Low", "Medium", "High", "XHigh"], speed: [""] },
	{ targetId: "openrouter-gemini-flash", reasoning: ["Low", "Medium", "High", "XHigh", "Max"], speed: [""] },
	{ targetId: "chatgpt-luna", reasoning: ["Low", "Medium", "High", "XHigh", "Max"], speed: ["Standard", "Fast"] },
	{ targetId: "chatgpt-sol", reasoning: ["Low", "Medium", "High", "XHigh", "Max"], speed: ["Standard", "Fast"] },
	{ targetId: "chatgpt-terra", reasoning: ["Low", "Medium", "High", "XHigh", "Max"], speed: ["Standard", "Fast"] },
	{ targetId: "synth-cloud-laguna-s", reasoning: ["None", "Max"], speed: [""] },
	{ targetId: "synth-cloud-muse-spark", reasoning: ["Low", "Medium", "High", "XHigh"], speed: [""] }
];

const COMBINATIONS = MODELS.flatMap((model) =>
	model.reasoning.flatMap((reasoning) =>
		model.speed.map((speed) => ({ targetId: model.targetId, reasoning, speed }))
	)
);

function overlaps(a: DOMRect, b: DOMRect): boolean {
	return a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
}

function center(rect: DOMRect | null | undefined) {
	return rect && rect.width > 0 && rect.height > 0
		? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
		: null;
}

const toolbar = extract((state: any) => {
	const document = state.document as Document;
	const viewport = state.window as Window;
	const matrixWindow = viewport as Window & {
		__synthBombadilComposerMatrix?: {
			activeTarget: string;
			visited: Record<string, boolean>;
		};
	};
	const tracker = matrixWindow.__synthBombadilComposerMatrix ??= {
		activeTarget: "local-laguna",
		visited: {}
	};

	const selectedByMenu = document.querySelector<HTMLElement>(
		'[data-testid^="composer-model-option-"][aria-selected="true"]'
	);
	const selectedTestId = selectedByMenu?.dataset.testid ?? "";
	if (selectedTestId.startsWith("composer-model-option-")) {
		tracker.activeTarget = selectedTestId.slice("composer-model-option-".length);
	}
	const lastActionName = String(state.lastAction?.Click?.name ?? "");
	if (lastActionName.startsWith("Select model ")) {
		tracker.activeTarget = lastActionName.slice("Select model ".length);
	}

	const buttonText = (testId: string) => (
		document.querySelector<HTMLElement>(`[data-testid="${testId}"] span`)?.textContent ?? ""
	).trim();
	const reasoning = buttonText("reasoning-effort-select");
	const speed = buttonText("service-tier-select");
	const activeDefinition = MODELS.find((model) => model.targetId === tracker.activeTarget);
	if (
		activeDefinition?.reasoning.includes(reasoning)
		&& activeDefinition.speed.includes(speed)
	) {
		tracker.visited[`${tracker.activeTarget}|${reasoning}|${speed}`] = true;
	}

	const next = COMBINATIONS.find((combination) => (
		!tracker.visited[`${combination.targetId}|${combination.reasoning}|${combination.speed}`]
	)) ?? null;
	const modelMenuOpen = Boolean(document.querySelector('[data-testid="composer-model-menu"]'));
	const reasoningMenuOpen = Boolean(document.querySelector('[data-testid="reasoning-effort-menu"]'));
	const speedMenuOpen = Boolean(document.querySelector('[data-testid="service-tier-menu"]'));
	const point = (selector: string) => center(
		document.querySelector<HTMLElement>(selector)?.getBoundingClientRect()
	);
	const optionPoint = (menuTestId: string, label: string) => {
		const options = Array.from(document.querySelectorAll<HTMLElement>(
			`[data-testid="${menuTestId}"] [role="option"]`
		));
		return center(options.find((option) => option.textContent?.trim().startsWith(label))?.getBoundingClientRect());
	};

	const toolbarElement = document.querySelector<HTMLElement>(".composer-toolbar");
	const toolbarRect = toolbarElement?.getBoundingClientRect() ?? null;
	const controlSelectors = [
		'[data-testid="composer-add-images"]',
		'[data-testid="composer-slash-btn"]',
		'[data-testid="approval-mode-select"]',
		'[data-testid="composer-model"]',
		'[data-testid="reasoning-effort-select"]',
		'[data-testid="service-tier-select"]',
		'[data-testid="composer-mic"]',
		'[data-testid="composer-send"]'
	];
	const controls = controlSelectors.flatMap((selector) => {
		const element = document.querySelector<HTMLElement>(selector);
		if (!element) return [];
		const rect = element.getBoundingClientRect();
		return rect.width > 0 && rect.height > 0 ? [{ selector, rect }] : [];
	});
	const overlapPairs: string[] = [];
	for (let left = 0; left < controls.length; left += 1) {
		for (let right = left + 1; right < controls.length; right += 1) {
			if (overlaps(controls[left].rect, controls[right].rect)) {
				overlapPairs.push(`${controls[left].selector} / ${controls[right].selector}`);
			}
		}
	}
	const controlsInsideToolbar = !toolbarRect || controls.every(({ rect }) => (
		rect.left >= toolbarRect.left - 1
		&& rect.right <= toolbarRect.right + 1
		&& rect.top >= toolbarRect.top - 1
		&& rect.bottom <= toolbarRect.bottom + 1
	));

	return {
		composerReady: Boolean(document.querySelector('[data-testid="composer"]')),
		viewportWidth: viewport.innerWidth,
		activeTarget: tracker.activeTarget,
		reasoning,
		speed,
		next,
		modelMenuOpen,
		reasoningMenuOpen,
		speedMenuOpen,
		modelPoint: point('[data-testid="composer-model"]'),
		nextModelPoint: next ? point(`[data-testid="composer-model-option-${next.targetId}"]`) : null,
		reasoningPoint: point('[data-testid="reasoning-effort-select"]'),
		nextReasoningPoint: next ? optionPoint("reasoning-effort-menu", next.reasoning) : null,
		speedPoint: point('[data-testid="service-tier-select"]'),
		nextSpeedPoint: next?.speed ? optionPoint("service-tier-menu", next.speed) : null,
		visitedCount: Object.keys(tracker.visited).length,
		totalCount: COMBINATIONS.length,
		overlapPairs,
		controlsInsideToolbar,
		permissionStacksVertically: Boolean(
			document.querySelector<HTMLElement>('[data-testid="approval-mode-select"]')?.getBoundingClientRect().height! > 36
		),
		noHorizontalOverflow: document.documentElement.scrollWidth <= viewport.innerWidth + 1
	};
});

/** Deterministically visit every model/knob tuple instead of hoping fuzzing samples it. */
export const visit_every_model_speed_and_thinking_combination = actions<any>(() => {
	if (!toolbar.current.composerReady) return ["Wait"];
	if (toolbar.current.viewportWidth !== 960) {
		return [{ SetViewport: { width: 960, height: 720 } }];
	}
	const next = toolbar.current.next;
	if (!next) return ["Wait"];
	if (next.targetId !== toolbar.current.activeTarget) {
		if (!toolbar.current.modelMenuOpen && toolbar.current.modelPoint) {
			return [{ Click: { name: "Open model matrix picker", point: toolbar.current.modelPoint } }];
		}
		if (toolbar.current.modelMenuOpen && toolbar.current.nextModelPoint) {
			return [{ Click: { name: `Select model ${next.targetId}`, point: toolbar.current.nextModelPoint } }];
		}
		return ["Wait"];
	}
	if (next.reasoning !== toolbar.current.reasoning) {
		if (!toolbar.current.reasoningMenuOpen && toolbar.current.reasoningPoint) {
			return [{ Click: { name: `Open reasoning for ${next.targetId}`, point: toolbar.current.reasoningPoint } }];
		}
		if (toolbar.current.reasoningMenuOpen && toolbar.current.nextReasoningPoint) {
			return [{ Click: { name: `Select reasoning ${next.reasoning}`, point: toolbar.current.nextReasoningPoint } }];
		}
		return ["Wait"];
	}
	if (next.speed !== toolbar.current.speed) {
		if (!toolbar.current.speedMenuOpen && toolbar.current.speedPoint) {
			return [{ Click: { name: `Open speed for ${next.targetId}`, point: toolbar.current.speedPoint } }];
		}
		if (toolbar.current.speedMenuOpen && toolbar.current.nextSpeedPoint) {
			return [{ Click: { name: `Select speed ${next.speed}`, point: toolbar.current.nextSpeedPoint } }];
		}
	}
	return ["Wait"];
});

export const all_54_model_speed_and_thinking_combinations_are_exercised = eventually(() =>
	toolbar.current.visitedCount === toolbar.current.totalCount
).within(50, "seconds");

export const composer_controls_never_overlap_for_any_combination = always(() =>
	toolbar.current.overlapPairs.length === 0
);

export const composer_controls_never_escape_the_toolbar = always(() =>
	toolbar.current.controlsInsideToolbar
);

export const permission_control_never_stacks = always(() =>
	!toolbar.current.permissionStacksVertically
);

export const composer_matrix_never_creates_horizontal_overflow = always(() =>
	toolbar.current.noHorizontalOverflow
);
