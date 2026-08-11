import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const whisperStatus = extract((state: any) => {
	const document = state.document;
	const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
	const status = document.querySelector<HTMLElement>('[data-testid="composer-whisper-status"]');
	return {
		composerVisible: Boolean(composer && composer.getBoundingClientRect().width > 0),
		statusVisible: Boolean(status && status.getBoundingClientRect().width > 0),
		uninitialized: Boolean(status?.classList.contains("is-undefined")),
		text: (status?.textContent ?? "").replace(/\s+/g, " ").trim()
	};
});

export const exercise_composer_across_viewports = actions(() => [
	{ SetViewport: { width: 960, height: 640 } },
	{ SetViewport: { width: 1172, height: 768 } },
	{ SetViewport: { width: 1440, height: 900 } }
]);

export const composer_fixture_is_exercised = eventually(() =>
	whisperStatus.current.composerVisible
).within(5, "seconds");

/** An absent runtime status must not be presented as a user-facing failure. */
export const whisper_attention_never_renders_from_an_uninitialized_runtime = always(() =>
	!whisperStatus.current.statusVisible
	|| !whisperStatus.current.uninitialized
	|| !/needs attention/i.test(whisperStatus.current.text)
);
