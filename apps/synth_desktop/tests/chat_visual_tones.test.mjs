import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/styles/app.css"), "utf8");

function rule(selector) {
	return css.match(new RegExp(`${selector.replaceAll(/[.*+?^${}()|[\\]\\]/g, "\\$&")}\\s*\\{[^}]*\\}`, "s"))?.[0] ?? "";
}

test("user messages use the Codex-like black highlight", () => {
	assert.match(rule(".local-bubble-user"), /background:\s*#111111/);
	assert.match(rule(".local-bubble-user"), /color:\s*#fff/);
});

test("visual-created lifecycle cards use neutral gray rather than a colored tint", () => {
	assert.match(rule(".visual-lifecycle"), /background:\s*var\(--color-card-header\)/);
	assert.doesNotMatch(rule(".visual-lifecycle"), /#635bff|color-mix/);
	assert.match(rule(".visual-lifecycle-mark"), /color:\s*var\(--color-text-muted\)/);
});
