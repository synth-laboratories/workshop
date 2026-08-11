import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const switchedRail = extract((state: any) => {
	const document = state.document;
	const chat = document.querySelector<HTMLElement>('[data-testid="local-chat-blank-worked-turn"]');
	const toggle = document.querySelector<HTMLElement>('[data-testid="toggle-inference-rail"]');
	const panel = document.querySelector<HTMLElement>('[data-testid="inference-panel"]');
	const rail = document.querySelector<HTMLElement>('[data-testid="inference-rail"]');
	const model = document.querySelector<HTMLElement>('[data-testid="composer-model"]');
	const menu = document.querySelector<HTMLElement>('[data-testid="composer-model-menu"]');
	const muse = document.querySelector<HTMLElement>('[data-testid="composer-model-option-local-muse-glimmer"]');
	const transcript = document.querySelector<HTMLElement>('[data-testid="chat-transcript"]');
	const rect = (element: HTMLElement | null) => element?.getBoundingClientRect() ?? null;
	const point = (element: HTMLElement | null) => {
		const value = rect(element);
		return value ? { x: value.left + value.width / 2, y: value.top + value.height / 2 } : null;
	};
	const panelRect = rect(panel);
	const transcriptRect = rect(transcript);
	const modelText = (model?.textContent ?? "").replace(/\s+/g, " ").trim();
	const panelText = (panel?.textContent ?? "").replace(/\s+/g, " ").trim();
	const railText = (rail?.textContent ?? "").replace(/\s+/g, " ").trim();
	return {
		chatPoint: chat && !chat.classList.contains("active") ? point(chat) : null,
		togglePoint: point(toggle),
		modelPoint: point(model),
		musePoint: point(muse),
		menuOpen: Boolean(menu),
		panelOpen: Boolean(panel),
		museSelected: /Muse Glimmer/i.test(modelText),
		paused: /Monitor paused/i.test(panelText),
		claimsMlxSidecar: /MLX SIDECAR/i.test(railText),
		consumesRail: Boolean(panelRect && transcriptRect && panelRect.width >= 240 && transcriptRect.width < state.window.innerWidth * 0.65)
	};
});

export const switch_to_muse_with_inference_rail_open = actions(() => {
	if (switchedRail.current.chatPoint) return [{ Click: { name: "Open inference-switch fixture", point: switchedRail.current.chatPoint } }];
	if (!switchedRail.current.panelOpen && switchedRail.current.togglePoint) {
		return [{ Click: { name: "Open Laguna inference rail", point: switchedRail.current.togglePoint } }];
	}
	if (!switchedRail.current.museSelected && !switchedRail.current.menuOpen && switchedRail.current.modelPoint) {
		return [{ Click: { name: "Open model picker before switch", point: switchedRail.current.modelPoint } }];
	}
	if (!switchedRail.current.museSelected && switchedRail.current.musePoint) {
		return [{ Click: { name: "Switch from Laguna to Muse", point: switchedRail.current.musePoint } }];
	}
	return ["Wait"];
});

export const paused_muse_rail_fixture_is_exercised = eventually(() =>
	switchedRail.current.museSelected && switchedRail.current.panelOpen
).within(8, "seconds");

/** Switching away from Laguna must not preserve an empty rail that crushes the transcript. */
export const inactive_inference_monitor_never_consumes_a_full_rail = always(() =>
	!switchedRail.current.museSelected
	|| !switchedRail.current.panelOpen
	|| !switchedRail.current.paused
	|| !switchedRail.current.consumesRail
);

/** Muse's GGUF/llama.cpp runtime must not be mislabeled as the MLX sidecar. */
export const muse_inference_never_claims_the_mlx_backend = always(() =>
	!switchedRail.current.museSelected
	|| !switchedRail.current.panelOpen
	|| !switchedRail.current.claimsMlxSidecar
);
