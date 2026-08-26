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
