import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const chatgptBranding = extract((state: any) => {
	const document = state.document;
	const point = (element: HTMLElement | null) => {
		const rect = element?.getBoundingClientRect();
		return rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : null;
	};
	const accountTrigger = document.querySelector<HTMLElement>('[data-testid="account-menu-trigger"]');
	const settings = document.querySelector<HTMLElement>('[data-testid="account-menu-settings"]');
	const models = [...document.querySelectorAll<HTMLButtonElement>(".settings-nav-item")]
		.find((button) => button.textContent?.trim() === "Models");
	const subscription = document.querySelector<HTMLElement>('[data-testid="chatgpt-codex-subscription"]');
	const titlebar = document.querySelector<HTMLElement>('[data-testid="titlebar"]');
	const activeTab = titlebar?.querySelector<HTMLElement>(".tab-active");
	const titlebarOpenAiMark = titlebar?.querySelector('[data-provider-mark="openai"]');
	const titlebarLeft = titlebar?.getBoundingClientRect().left ?? 0;
	const activeTabLeft = activeTab?.getBoundingClientRect().left ?? 0;

	return {
		accountPoint: accountTrigger && !settings ? point(accountTrigger) : null,
		settingsPoint: settings ? point(settings) : null,
		modelsPoint: models && !subscription ? point(models) : null,
		subscriptionVisible: Boolean(subscription),
		titlebarUsesOpenAi: Boolean(titlebarOpenAiMark),
		cardUsesOpenAi: Boolean(subscription?.querySelector('[data-provider-mark="openai"]')),
		// macOS reserves the leading titlebar region for the traffic-light
		// controls. The app tab must start after that 80px safe area.
		activeTabClearsTrafficLights: Boolean(activeTab) && activeTabLeft - titlebarLeft >= 80
	};
});

export const open_connected_chatgpt_models_settings = actions(() => {
	if (chatgptBranding.current.accountPoint) {
		return [{ Click: { name: "Open account menu", point: chatgptBranding.current.accountPoint } }];
	}
	if (chatgptBranding.current.settingsPoint) {
		return [{ Click: { name: "Open Settings", point: chatgptBranding.current.settingsPoint } }];
	}
	if (chatgptBranding.current.modelsPoint) {
		return [{ Click: { name: "Open Models settings", point: chatgptBranding.current.modelsPoint } }];
	}
	return [{ SetViewport: { width: 1200, height: 820 } }];
});

export const connected_chatgpt_subscription_settings_are_exercised = eventually(() =>
	chatgptBranding.current.subscriptionVisible
).within(8, "seconds");

export const chatgpt_subscription_surfaces_use_openai_branding = always(() =>
	!chatgptBranding.current.subscriptionVisible
	|| (chatgptBranding.current.cardUsesOpenAi && chatgptBranding.current.titlebarUsesOpenAi)
);

export const active_tab_never_overlaps_macos_traffic_lights = always(() =>
	chatgptBranding.current.activeTabClearsTrafficLights
);
