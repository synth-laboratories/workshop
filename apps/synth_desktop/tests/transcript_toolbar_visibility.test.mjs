import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src");
const read = (rel) => readFileSync(join(root, rel), "utf8");

test("outputs use the persistent right-panel toggle instead of a transcript toolbar", () => {
	const transcript = read("components/ChatTranscript.tsx");
	const titlebar = read("components/AppTitlebar.tsx");
	const panel = read("components/WorkbenchSidePanel.tsx");

	assert.doesNotMatch(transcript, /resource-shelf-trigger/);
	assert.doesNotMatch(transcript, /transcript-toolbar/);
	assert.doesNotMatch(transcript, /activity-mode-menu-trigger/);
	assert.doesNotMatch(transcript, /Activity ·/);
	assert.match(titlebar, /outputCount > 0 \? <span className="titlebar-panel-count"/);
	assert.match(panel, /restoreFocusIfLost\('\[data-testid="toggle-inference-rail"\]'\)/);
});
