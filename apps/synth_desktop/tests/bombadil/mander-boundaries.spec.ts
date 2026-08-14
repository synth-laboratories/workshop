import { actions, always, extract } from "@antithesishq/bombadil";

/**
 * Product code may import Mander / ManderState / ManderMotion through
 * components/mander/index.ts. Geometry, poses, recipes, and the motion hook
 * stay inside that folder. The static import scan lives in
 * tests/mander_motion.test.mjs; this spec asserts the runtime boundary:
 * the lab is fixture-hashed, never a navigation item, and internals are not
 * mounted on the default shell.
 */
const boundary = extract((state: any) => {
	const document = state.document;
	const sidebar = document.querySelector('[data-testid="sidebar"]');
	const lab = document.querySelector('[data-testid="mander-lab"]');
	const parts = document.querySelectorAll("[data-mander-part]");
	const navText = sidebar?.textContent ?? "";
	return {
		shellVisible: Boolean(document.querySelector(".app-shell")),
		labMounted: Boolean(lab),
		labInSidebar: /mander lab/i.test(navText),
		internalPartsOnShell: parts.length > 0 && !lab,
		hashRequestsLab: state.window.location.hash === "#mander-lab"
	};
});

export const keep_default_shell_in_view = actions(() => [
	{ SetViewport: { width: 1280, height: 840 } }
]);

export const mander_lab_is_not_a_navigation_item = always(() =>
	boundary.current.shellVisible && !boundary.current.labInSidebar
);

export const mander_internals_are_not_mounted_on_the_default_shell = always(() =>
	!boundary.current.hashRequestsLab
	&& !boundary.current.labMounted
	&& !boundary.current.internalPartsOnShell
);
