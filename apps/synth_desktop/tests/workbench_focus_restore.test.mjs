import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src");
const read = (rel) => readFileSync(join(root, rel), "utf8");

test("closing the side panel restores its persistent titlebar toggle", () => {
	const panel = read("components/WorkbenchSidePanel.tsx");
	const titlebar = read("components/AppTitlebar.tsx");
	const helper = read("runtime/restoreFocus.ts");
	assert.match(titlebar, /data-testid="toggle-inference-rail"/);
	assert.match(panel, /function closeSelectedTab\(\)[\s\S]*?restoreFocusIfLost\('\[data-testid="toggle-inference-rail"\]'\)/);
	assert.match(panel, /aria-label=\{item\.onClose \? `Close \$\{item\.title \?\? item\.label\}` : "Close side panel"\}/);
	assert.match(helper, /queueMicrotask/);
	assert.match(helper, /requestAnimationFrame/);
	assert.match(helper, /document\.body/);
	assert.match(helper, /documentElement/);
	assert.match(helper, /isConnected/);
});

test("the persistent titlebar control owns terminal visibility", () => {
	const layout = read("hooks/useShellLayout.ts");
	const titlebar = read("components/AppTitlebar.tsx");
	const terminal = read("components/TerminalPanel.tsx");
	const helper = read("runtime/restoreFocus.ts");
	assert.match(titlebar, /data-testid="toggle-terminal"/);
	assert.match(titlebar, /Show terminal/);
	assert.doesNotMatch(terminal, /aria-label="Hide terminal"/);
	assert.match(layout, /restoreFocusIfLost\('\[data-testid="toggle-terminal"\]'\)/);
	assert.match(helper, /queueMicrotask/);
	assert.match(helper, /requestAnimationFrame/);
});

test("composer is owned by the active transcript layout instead of global pane geometry", () => {
	const layout = read("components/ComposerLayout.tsx");
	const dock = read("components/ComposerDock.tsx");
	const transcript = read("components/ChatTranscript.tsx");
	const landing = read("components/LandingPage.tsx");
	const css = read("styles/app.css");
	assert.match(layout, /ComposerLayoutProvider/);
	assert.match(layout, /ComposerLayoutHost/);
	assert.match(dock, /return createPortal\([\s\S]*?,\s*host\s*\);/);
	assert.match(transcript, /<ComposerLayoutHost \/>/);
	assert.match(landing, /<ComposerLayoutHost \/>/);
	assert.match(css, /\.composer-layout-host/);
	assert.doesNotMatch(css, /--composer-dock-(?:left|right)/);
	assert.doesNotMatch(css, /--live-transcript-width/);
});

test("visual tabs use short display names while retaining descriptive titles", () => {
	const routes = read("routes.tsx");
	const panel = read("components/WorkbenchSidePanel.tsx");
	const sessionView = read("runtime/sessionView.ts");
	assert.match(routes, /label:\s*artifact\.displayName\?\.trim\(\)\s*\|\|\s*artifact\.title\s*\|\|\s*"Visual"/);
	assert.match(routes, /title:\s*artifact\.title\s*\|\|\s*artifact\.displayName\s*\|\|\s*"Visual"/);
	assert.match(panel, /title=\{item\.title \?\? item\.label\}/);
	assert.match(sessionView, /stringField\(metadata \?\? \{\}, "displayName", "display_name"\)/);
});

test("side-panel tabs remain interactive inside the draggable desktop shell", () => {
	const panel = read("components/WorkbenchSidePanel.tsx");
	const css = read("styles/app.css");
	assert.match(panel, /closest<HTMLElement>\('\[role="tablist"\]'\)/);
	assert.match(panel, /onClick=\{\(\) => onTabChange\(item\.id\)\}/);
	assert.match(css, /\.workbench-side-panel-tabs \{[^}]*-webkit-app-region: no-drag;/);
	assert.match(css, /\.workbench-side-panel-option-tabs \[role="tab"\] \{[^}]*pointer-events: auto;[^}]*-webkit-app-region: no-drag;/);
});
