import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFileSync(join(root, path), "utf8");

test("renderer OAuth contract exposes status but never token fields", () => {
	const types = read("src/renderer/src/bridge/types.ts");
	const oauth = types.slice(types.indexOf("export type CodexOauthBegin"), types.indexOf("export type CodexOauthBridge") + 500);
	assert.match(oauth, /accountHint/);
	assert.doesNotMatch(oauth, /accessToken|refreshToken|idToken|authorization/i);
});

test("OAuth commands stay aligned across Rust and TypeScript", () => {
	const rust = read("src-tauri/src/contract/commands.rs");
	const ts = read("src/renderer/src/bridge/protocolConstants.ts");
	for (const command of ["codex_oauth_begin", "codex_oauth_complete_manual", "codex_oauth_status", "codex_oauth_disconnect", "codex_oauth_cancel"]) {
		assert.ok(rust.includes(command), `Rust command missing ${command}`);
		assert.ok(ts.includes(command), `TypeScript command missing ${command}`);
	}
});

test("subscription card carries the local custody and allowance handoff", () => {
	const card = read("src/renderer/src/components/ChatgptCodexSubscriptionCard.tsx");
	assert.match(card, /tokens remain stored locally/);
	assert.match(card, /not API credits/);
	assert.match(card, /Plan allowance \(ChatGPT\)/);
	assert.doesNotMatch(card, /console\.(?:log|debug|info|warn|error)/);
});
