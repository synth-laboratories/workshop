import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src");
const read = (rel) => readFileSync(join(root, rel), "utf8");

test("close-outputs restores resource-shelf-trigger", () => {
	const panel = read("components/WorkbenchSidePanel.tsx");
	const transcript = read("components/ChatTranscript.tsx");
	const helper = read("runtime/restoreFocus.ts");
	const close = panel.match(/aria-label="Close side panel"[\s\S]*?<\/button>/)?.[0] ?? "";
	assert.match(transcript, /data-testid="resource-shelf-trigger"/);
	assert.match(close, /restoreFocusIfLost/);
	assert.match(close, /resource-shelf-trigger/);
	assert.match(helper, /queueMicrotask/);
	assert.match(helper, /requestAnimationFrame/);
	assert.match(helper, /document\.body/);
	assert.match(helper, /documentElement/);
	assert.match(helper, /isConnected/);
});

test("hide-terminal restores toggle-terminal", () => {
	const layout = read("hooks/useShellLayout.ts");
	const titlebar = read("components/AppTitlebar.tsx");
	const terminal = read("components/TerminalPanel.tsx");
	const helper = read("runtime/restoreFocus.ts");
	const hide = terminal.match(/aria-label="Hide terminal"[\s\S]*?<\/button>/)?.[0] ?? "";
	assert.match(titlebar, /data-testid="toggle-terminal"/);
	assert.match(titlebar, /Show terminal/);
	assert.match(hide, /restoreFocusIfLost/);
	assert.match(hide, /toggle-terminal/);
	assert.match(layout, /restoreFocusIfLost\('\[data-testid="toggle-terminal"\]'\)/);
	assert.match(helper, /queueMicrotask/);
	assert.match(helper, /requestAnimationFrame/);
	assert.doesNotMatch(hide, /restoreFocusAfterVisualPaneClose/);
});

test("composer follows the live transcript width while the side panel divider moves", () => {
	const composer = read("components/Composer.tsx");
	const splitter = read("components/PaneResizeHandle.tsx");
	const css = read("styles/app.css");
	assert.doesNotMatch(composer, /dock\.style\.setProperty\("left"/);
	assert.doesNotMatch(composer, /dock\.style\.setProperty\("width"/);
	assert.match(splitter, /publishOutputWidth\(target, next, bounds\.width - next - 7\)/);
	assert.match(css, /--live-transcript-width/);
	assert.match(css, /\.main-pane:has\(\.workbench\.with-side-panel\) \.composer-dock/);
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
