import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const modelMenu = extract((state: any) => {
	const document = state.document;
	const chat = document.querySelector<HTMLElement>('[data-testid="local-chat-blank-worked-turn"]');
	const trigger = document.querySelector<HTMLElement>('[data-testid="composer-model"]');
	const menu = document.querySelector<HTMLElement>('[data-testid="composer-model-menu"]');
	const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
	const rect = (element: HTMLElement | null) => element?.getBoundingClientRect() ?? null;
	const point = (element: HTMLElement | null) => {
		const value = rect(element);
		return value ? { x: value.left + value.width / 2, y: value.top + value.height / 2 } : null;
	};
	const menuRect = rect(menu);
	const composerRect = rect(composer);
	const optionText = menu
		? [...menu.querySelectorAll<HTMLElement>('[data-testid^="composer-model-option-"]')]
			.map((option) => (option.textContent ?? "").replace(/\s+/g, " ").trim())
		: [];
	return {
		chatPoint: chat && !chat.classList.contains("active") ? point(chat) : null,
		triggerPoint: point(trigger),
		open: Boolean(menu),
		clearsComposer: !menuRect || !composerRect || menuRect.bottom <= composerRect.top - 8,
		insideViewport: !menuRect || (
			menuRect.left >= 8 && menuRect.top >= 8
			&& menuRect.right <= state.window.innerWidth - 8
			&& menuRect.bottom <= state.window.innerHeight - 8
		),
		metadataHasSeparators: optionText.every((text) => !/(?:Text only|Images)\d/i.test(text)),
		scrollsInternally: !menu || menu.scrollHeight <= menu.clientHeight || getComputedStyle(menu).overflowY === "auto"
	};
});

export const open_model_menu_and_fuzz_supported_widths = actions(() => {
	if (modelMenu.current.chatPoint) {
		return [{ Click: { name: "Open model-menu fixture chat", point: modelMenu.current.chatPoint } }];
	}
	if (!modelMenu.current.open && modelMenu.current.triggerPoint) {
		return [{ Click: { name: "Open composer model menu", point: modelMenu.current.triggerPoint } }];
	}
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1172, height: 768 } },
		{ SetViewport: { width: 1280, height: 840 } }
	];
});

export const composer_model_menu_is_exercised = eventually(() =>
	modelMenu.current.open
).within(8, "seconds");

export const composer_model_menu_never_covers_the_composer = always(() =>
	!modelMenu.current.open || (modelMenu.current.clearsComposer && modelMenu.current.insideViewport && modelMenu.current.scrollsInternally)
);

export const composer_model_metadata_keeps_readable_separators = always(() =>
	!modelMenu.current.open || modelMenu.current.metadataHasSeparators
);

