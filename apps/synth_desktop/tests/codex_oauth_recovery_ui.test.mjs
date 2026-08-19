import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");

test("ChatGPT recovery UI never stringifies structured native errors", () => {
	const card = read("src/renderer/src/components/ChatgptCodexSubscriptionCard.tsx");
	assert.match(card, /function oauthErrorMessage/);
	assert.match(card, /typeof value\.message === "string"/);
	assert.doesNotMatch(card, /setError\(reason instanceof Error \? reason\.message : String\(reason\)\)/);
	assert.match(card, /data-testid="codex-oauth-restart"/);
	assert.match(card, /next\.state === "refresh_failed"/);
	assert.match(card, /next\.state === "ready"/);
	assert.doesNotMatch(card, /if \(next\.configured/);
	assert.match(card, /Start over to create a fresh authorization attempt/);
});

test("model picker reflects the Rust auth state and recovery action", () => {
	const composer = read("src/renderer/src/components/Composer.tsx");
	assert.match(composer, /state\.codexOauthStatus\?\.state === "expired" \? "Authorization expired"/);
	assert.match(composer, /state\.codexOauthStatus\?\.state === "refresh_failed" \? "Re-sync failed"/);
	assert.match(composer, /state\.codexOauthStatus\?\.action === "reauthenticate"/);
});

test("packaged startup reads ChatGPT auth passively and refreshes before use", () => {
	const controller = read("src/renderer/src/hooks/useAppController.ts");
	assert.match(controller, /const refreshOauthStatus = \(\) => \{/);
	assert.match(controller, /bridges\.codexOauth\?\.status\(\)\.then/);
	assert.match(controller, /const ensureCodexOauthReady = useCallback/);
	assert.match(controller, /bridges\.codexOauth\?\.ensureReady\(\)\.catch/);
});
