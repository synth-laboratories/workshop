import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => readFileSync(join(appRoot, "src/renderer/src", relative), "utf8");

test("the browser runtime subscription has a ten-attempt terminal retry bound", () => {
	const bridge = read("runtime/desktopBridge.ts");
	assert.match(bridge, /const maxConsecutiveFailures = 10;/);
	assert.match(bridge, /consecutiveFailures >= maxConsecutiveFailures/);
	assert.match(bridge, /state: "failed"/);
	assert.match(bridge, /stopped after \$\{maxConsecutiveFailures\} attempts/);
});

test("VisualHost preserves the store's failed state instead of relabeling it interrupted", () => {
	const host = read("components/VisualHost.tsx");
	assert.match(host, /snapshot\.state === "interrupted" \|\| snapshot\.state === "failed"/);
	assert.match(host, /setConnectionState\(snapshot\.state\)/);
});

test("nullable provider cost is rendered through the missing-aware formatter", () => {
	const card = read("components/runProgress/RunProgressCard.tsx");
	assert.match(card, /formatMissingUsd\(projection\.providerAccess\.usedCostUsd\)/);
	assert.doesNotMatch(card, /providerAccess\.usedCostUsd\.toFixed/);
});
