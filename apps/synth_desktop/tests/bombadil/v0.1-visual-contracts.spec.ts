import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const visualContracts = extract((state: any) => {
	const document = state.document;
	const point = (element: HTMLElement | null) => {
		const box = element?.getBoundingClientRect();
		return box ? { x: box.left + box.width / 2, y: box.top + box.height / 2 } : null;
	};
	const titlebar = document.querySelector<HTMLElement>('[data-testid="titlebar"]');
	const actions = [...(titlebar?.querySelectorAll<HTMLElement>(".titlebar-actions > *") ?? [])];
	const account = document.querySelector<HTMLElement>('[data-testid="account-menu-trigger"]');
	const accountSettings = document.querySelector<HTMLElement>('[data-testid="account-menu-settings"], [data-testid="open-account-settings"]');
	const modelsButton = [...document.querySelectorAll<HTMLButtonElement>("button")]
		.find((button) => button.textContent?.trim() === "Models") ?? null;
	const providerCard = document.querySelector<HTMLElement>('[data-testid="authorized-models"]');
	const rows = [...(providerCard?.querySelectorAll<HTMLElement>(".authorized-model-row") ?? [])];
	const marks = [...(providerCard?.querySelectorAll<HTMLElement>(".authorized-model-mark") ?? [])];
	const slugs = [...(providerCard?.querySelectorAll<HTMLElement>(".authorized-model-identity code") ?? [])];
	const markBoxes = marks.map((mark) => mark.getBoundingClientRect());
	const cardBox = providerCard?.getBoundingClientRect();
	return {
		accountPoint: point(account),
		accountSettingsPoint: point(accountSettings),
		modelsPoint: point(modelsButton),
		settingsVisible: Boolean(document.querySelector('[data-testid="settings-page"]')),
		providersVisible: Boolean(providerCard),
		titlebarIsClean: Boolean(titlebar) &&
			!titlebar.querySelector('[data-testid="runtime-status"], [data-testid="titlebar-account"], [data-testid="titlebar-account-avatar"], [data-testid="titlebar-cloud-status"]') &&
			!actions.some((element) => !["toggle-terminal", "toggle-inference-rail"].includes(element.dataset.testid ?? "")) &&
			![...titlebar.querySelectorAll("*")].some((element) => ["Local", "S"].includes(element.textContent?.trim() ?? "")),
		providerRowsComplete: !providerCard || rows.length === 3,
		providerMarksQuiet: !providerCard || (
			marks.length === 3 && markBoxes.every((box) => box.width <= 22 && box.height <= 22)
		),
		providerMarksAligned: markBoxes.length < 2 || (
			Math.max(...markBoxes.map((box) => box.left + box.width / 2)) -
			Math.min(...markBoxes.map((box) => box.left + box.width / 2)) <= 1
		),
		slugsSubordinate: !providerCard || (
			slugs.length === 3 && slugs.every((slug) => Number.parseFloat(getComputedStyle(slug).fontSize) <= 10)
		),
		providerCardContained: !cardBox || (
			cardBox.left >= 0 && cardBox.right <= state.window.innerWidth &&
			document.documentElement.scrollWidth <= state.window.innerWidth + 1
		)
	};
});

export const navigate_to_credentialed_models_and_resize = actions(() => {
	if (!visualContracts.current.settingsVisible && !visualContracts.current.accountSettingsPoint && visualContracts.current.accountPoint) {
		return [{ Click: { name: "Open account menu", point: visualContracts.current.accountPoint } }];
	}
	if (!visualContracts.current.settingsVisible && visualContracts.current.accountSettingsPoint) {
		return [{ Click: { name: "Open Settings", point: visualContracts.current.accountSettingsPoint } }];
	}
	if (!visualContracts.current.providersVisible && visualContracts.current.modelsPoint) {
		return [{ Click: { name: "Open Models settings", point: visualContracts.current.modelsPoint } }];
	}
	return [
		{ SetViewport: { width: 900, height: 640 } },
		{ SetViewport: { width: 1100, height: 700 } },
		{ SetViewport: { width: 1440, height: 900 } }
	];
});

export const credentialed_model_surface_is_exercised = eventually(() =>
	visualContracts.current.providersVisible
).within(8, "seconds");

export const legacy_titlebar_chrome_never_returns = always(() =>
	visualContracts.current.titlebarIsClean
);

export const authorized_provider_rows_remain_complete = always(() =>
	visualContracts.current.providerRowsComplete
);

export const authorized_provider_marks_remain_small_and_aligned = always(() =>
	visualContracts.current.providerMarksQuiet && visualContracts.current.providerMarksAligned
);

export const model_slugs_remain_subordinate = always(() =>
	visualContracts.current.slugsSubordinate
);

export const authorized_provider_card_never_overflows = always(() =>
	visualContracts.current.providerCardContained
);
