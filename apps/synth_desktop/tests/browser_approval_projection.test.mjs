import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const compiled = join(compiledDir, "sessionView.browser-approval.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/sessionView.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile: compiled
});

const { eventsToLocalActivity, formatApprovalPayloadValue } = await import(pathToFileURL(compiled).href);

test("browser exact-action approval renders bounded structured metadata", () => {
	const secret = "this-value-must-not-be-in-an-approval";
	const activity = eventsToLocalActivity([{
		schemaVersion: "synth.desktop-runtime-event.v1",
		sessionId: "browser-approval-session",
		sequence: 1,
		eventKind: "approval.requested",
		createdAt: "2026-08-17T00:00:00.000Z",
		source: "local",
		payload: {
			approvalId: "approval-browser-fill",
			kind: "computer_use",
			hazard: true,
			app: "browser:https://example.com",
			action: "browser_fill",
			alwaysSupported: false,
			payload: {
				origin: "https://example.com",
				role: "textbox",
				name: "Account name",
				tab: "tab-123",
				documentRevision: "4.2.1.digest",
				actionDetails: { valueLength: secret.length, valueSha256: "sha256-redacted-value" }
			}
		}
	}], []);
	const [line] = Object.values(activity).flat();
	assert.equal(line.label, "Confirm this action");
	assert.match(line.detail, /browser_fill/);
	assert.match(line.detail, /origin: https:\/\/example\.com/);
	assert.match(line.detail, /name: Account name/);
	assert.match(line.detail, new RegExp(`valueLength=${secret.length}`));
	assert.match(line.detail, /valueSha256=sha256-redacted-value/);
	assert.doesNotMatch(line.detail, /\[object Object\]/);
	assert.doesNotMatch(line.detail, new RegExp(secret));
	assert.equal(line.alwaysAllowSupported, false);
});

test("approval metadata formatting is bounded", () => {
	const rendered = formatApprovalPayloadValue(Array.from({ length: 20 }, (_, index) => `item-${index}`));
	assert.match(rendered, /item-0/);
	assert.match(rendered, /\+12 more/);
	assert.doesNotMatch(rendered, /item-19/);
});
