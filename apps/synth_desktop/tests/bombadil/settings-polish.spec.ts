import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const settingsPolish = extract((state: any) => {
	const document = state.document as Document;
	const window = state.window as Window & { __settingsPolishVisited?: { secrets: boolean; about: boolean } };
	const visited = window.__settingsPolishVisited ??= { secrets: false, about: false };
	const point = (element: HTMLElement | null | undefined) => {
		const rect = element?.getBoundingClientRect();
		return rect && rect.width > 0 && rect.height > 0
			? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
			: null;
	};
	const fingerprint = (element: HTMLElement | null | undefined) => element ? {
		testId: element.dataset.testid ?? null,
		id: element.id || null,
		role: element.getAttribute("role"),
		accessibleName: element.getAttribute("aria-label"),
		tag: element.tagName.toLowerCase(),
		href: element.getAttribute("href"),
		nameAttr: element.getAttribute("name"),
		placeholder: element.getAttribute("placeholder"),
		inputType: element.getAttribute("type"),
		textContent: element.textContent?.trim() || null,
		structuralPath: null
	} : null;
	const accountTrigger = document.querySelector<HTMLElement>('[data-testid="account-menu-trigger"]');
	const settingsButton = document.querySelector<HTMLElement>('[data-testid="account-menu-settings"]');
	const navButton = (label: string) => [...document.querySelectorAll<HTMLElement>(".settings-nav-item")]
		.find((button) => button.textContent?.trim() === label);
	const secrets = document.querySelector<HTMLElement>('[data-testid="settings-secrets"]');
	const about = document.querySelector<HTMLElement>('[data-testid="settings-about"]');

	let secretsClean = false;
	if (secrets) {
		const rows = [...secrets.querySelectorAll<HTMLElement>(".secrets-row")];
		const compactRows = rows.every((row) => {
			const rect = row.getBoundingClientRect();
			const actions = row.querySelector<HTMLElement>(".secrets-row-actions")?.getBoundingClientRect();
			return rect.height >= 44 && rect.height <= 112
				&& (!actions || (actions.left >= rect.left && actions.right <= rect.right + 1));
		});
		const auxiliaryCards = ["secrets-known-locations", "secrets-active-capabilities", "secrets-audit-log"]
			.map((id) => document.querySelector<HTMLElement>(`[data-testid="${id}"]`))
			.filter(Boolean) as HTMLElement[];
		const substantiveCards = auxiliaryCards.every((card) => {
			const body = card.querySelector<HTMLElement>(".settings-card-body");
			return Boolean(body && body.getBoundingClientRect().height >= 44);
		});
		secretsClean = rows.length > 0 && compactRows && substantiveCards;
		if (secretsClean) visited.secrets = true;
	}

	let aboutClean = false;
	if (about) {
		const card = document.querySelector<HTMLElement>('[data-testid="capability-manifest"]')?.closest<HTMLElement>(".settings-card");
		const frame = document.querySelector<HTMLElement>('[data-testid="capability-manifest"]');
		const table = frame?.querySelector<HTMLElement>("table");
		const frameRect = frame?.getBoundingClientRect();
		const cardRect = card?.getBoundingClientRect();
		const tableRect = table?.getBoundingClientRect();
		aboutClean = Boolean(frameRect && cardRect && tableRect
			&& frameRect.left - cardRect.left >= 12
			&& cardRect.right - frameRect.right >= 12
			&& tableRect.left >= frameRect.left
			&& tableRect.right <= frameRect.right + 1);
		if (aboutClean) visited.about = true;
	}

	return {
		accountPoint: !document.querySelector('[data-testid="settings-page"]') && !settingsButton ? point(accountTrigger) : null,
		accountFingerprint: fingerprint(accountTrigger),
		settingsPoint: point(settingsButton),
		settingsFingerprint: fingerprint(settingsButton),
		secretsPoint: !visited.secrets ? point(navButton("Secrets")) : null,
		secretsFingerprint: fingerprint(navButton("Secrets")),
		aboutPoint: visited.secrets && !visited.about ? point(navButton("About")) : null,
		aboutFingerprint: fingerprint(navButton("About")),
		secretsVisible: Boolean(secrets),
		aboutVisible: Boolean(about),
		secretsClean,
		aboutClean,
		complete: visited.secrets && visited.about,
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

export const visit_polished_settings_sections = actions(() => {
	if (settingsPolish.current.accountPoint) return [{ Click: { fingerprint: settingsPolish.current.accountFingerprint, point: settingsPolish.current.accountPoint } }];
	if (settingsPolish.current.settingsPoint) return [{ Click: { fingerprint: settingsPolish.current.settingsFingerprint, point: settingsPolish.current.settingsPoint } }];
	if (settingsPolish.current.secretsPoint) return [{ Click: { fingerprint: settingsPolish.current.secretsFingerprint, point: settingsPolish.current.secretsPoint } }];
	if (settingsPolish.current.aboutPoint) return [{ Click: { fingerprint: settingsPolish.current.aboutFingerprint, point: settingsPolish.current.aboutPoint } }];
	return ["Wait"];
});

export const secrets_and_about_are_both_exercised = eventually(() => settingsPolish.current.complete)
	.within(9, "seconds");

export const secret_rows_and_empty_cards_keep_deliberate_geometry = always(() =>
	!settingsPolish.current.secretsVisible || settingsPolish.current.secretsClean
);

export const capability_table_is_inset_and_contained = always(() =>
	!settingsPolish.current.aboutVisible || settingsPolish.current.aboutClean
);

export const settings_polish_never_adds_page_overflow = always(() =>
	settingsPolish.current.noHorizontalOverflow
);
