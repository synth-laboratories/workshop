import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const composerLabel = extract((state: any) => {
	const document = state.document;
	const chat = document.querySelector<HTMLElement>('[data-testid="local-chat-blank-worked-turn"]');
	const model = document.querySelector<HTMLElement>('[data-testid="composer-model"]');
	const menu = document.querySelector<HTMLElement>('[data-testid="composer-model-menu"]');
	const museOption = document.querySelector<HTMLElement>('[data-testid="composer-model-option-local-muse-glimmer"]');
	const input = document.querySelector<HTMLTextAreaElement>('[data-testid="composer-input"]');
	const chatRect = chat?.getBoundingClientRect() ?? null;
	const modelText = (model?.textContent ?? "").replace(/\s+/g, " ").trim();
	const placeholder = input?.getAttribute("placeholder") ?? "";
	const modelRect = model?.getBoundingClientRect() ?? null;
	const optionRect = museOption?.getBoundingClientRect() ?? null;
	return {
		chatPoint: chat && !chat.classList.contains("active") && chatRect
			? { x: chatRect.left + chatRect.width / 2, y: chatRect.top + chatRect.height / 2 }
			: null,
		modelText,
		menuOpen: Boolean(menu),
		modelPoint: modelRect ? { x: modelRect.left + modelRect.width / 2, y: modelRect.top + modelRect.height / 2 } : null,
		museOptionPoint: optionRect ? { x: optionRect.left + optionRect.width / 2, y: optionRect.top + optionRect.height / 2 } : null,
		placeholder,
		museSelected: /Muse Glimmer/i.test(modelText),
		placeholderClaimsLaguna: /Laguna/i.test(placeholder)
	};
});

export const open_muse_composer_fixture = actions(() => {
	if (composerLabel.current.chatPoint) return [{ Click: { name: "Open Muse composer fixture", point: composerLabel.current.chatPoint } }];
	if (!composerLabel.current.museSelected && !composerLabel.current.menuOpen && composerLabel.current.modelPoint) {
		return [{ Click: { name: "Open local model picker", point: composerLabel.current.modelPoint } }];
	}
	if (!composerLabel.current.museSelected && composerLabel.current.museOptionPoint) {
		return [{ Click: { name: "Select Muse Glimmer", point: composerLabel.current.museOptionPoint } }];
	}
	return ["Wait"];
});

export const muse_composer_fixture_is_exercised = eventually(() =>
	composerLabel.current.museSelected && Boolean(composerLabel.current.placeholder)
).within(8, "seconds");

/** The draft surface must name the selected model family, not a stale provider default. */
export const composer_placeholder_never_claims_laguna_when_muse_is_selected = always(() =>
	!composerLabel.current.museSelected || !composerLabel.current.placeholderClaimsLaguna
);
