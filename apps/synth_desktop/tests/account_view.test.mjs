import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const source = join(appRoot, "src/renderer/src/runtime/accountView.ts");
const compiled = join(compiledDir, "accountView.mjs");
writeFileSync(compiled, transformSync(readFileSync(source, "utf8"), {
	loader: "ts",
	format: "esm",
	target: "es2022",
	sourcefile: source
}).code);

const { buildAccountView, resolveAccountState, formatUsd } = await import(pathToFileURL(compiled).href);

const cloudSummary = (overrides = {}) => ({
	signedIn: true,
	state: "active",
	environment: "prod",
	source: "cloud",
	accountId: "acct_1",
	displayName: "ada",
	email: "ada@example.com",
	organization: { id: "org_1", displayName: "Ada Labs", role: "owner" },
	plan: {
		name: "Pro",
		tier: "pro",
		state: "active",
		metered: true,
		monthlyAllowanceUsd: 200,
		usedUsd: 42.5,
		remainingUsd: 157.5,
		resetsAt: "2026-09-01T00:00:00+00:00",
		source: "cloud"
	},
	cloudUsage: {
		today: { events: 2, costUsd: 0.15 },
		sevenDays: { events: 9, costUsd: 1.2 },
		thirtyDays: { events: 40, costUsd: 13 }
	},
	billing: {
		checkoutUrl: "https://example.test/usage?upgrade=pro",
		portalUrl: "https://example.test/usage",
		upgradeTier: "pro"
	},
	lastUpdated: "2026-08-10T12:00:00+00:00",
	...overrides
});

test("a device that never paired reads as local-only, not signed out", () => {
	const view = buildAccountView(null, false);
	assert.equal(view.state, "local_only");
	assert.equal(view.signedIn, false);
	assert.equal(view.title, "Sign in to Synth");
	assert.equal(view.subtitle, "Local mode");
	assert.deepEqual(view.primaryAction, { kind: "sign_in", label: "Sign in to Synth" });
	assert.equal(view.plan, null);
});

test("pairing wins over the host state because only the renderer knows it", () => {
	assert.equal(resolveAccountState(cloudSummary(), true, true), "pairing");
	assert.equal(resolveAccountState(cloudSummary(), true, false), "active");
});

test("a host without a state field falls back to key presence", () => {
	const legacy = { signedIn: true, environment: "dev" };
	assert.equal(resolveAccountState(legacy, true, false), "active");
	assert.equal(resolveAccountState(null, false, false), "local_only");
});

test("an active cloud account renders its own identity, plan, and manage action", () => {
	const view = buildAccountView(cloudSummary(), true);
	assert.equal(view.state, "active");
	assert.equal(view.title, "ada");
	assert.equal(view.subtitle, "Ada Labs");
	assert.equal(view.planIsDevSeed, false);
	assert.equal(view.planHasDollars, true);
	assert.equal(view.cloudBlockedReason, null);
	assert.deepEqual(view.primaryAction, { kind: "manage", label: "Manage billing" });
});

test("an exhausted allowance blocks cloud spend and offers upgrade, never blocking local", () => {
	const view = buildAccountView(
		cloudSummary({
			state: "limited",
			plan: { name: "Starter", metered: true, monthlyAllowanceUsd: 20, usedUsd: 20, remainingUsd: 0, source: "cloud" }
		}),
		true
	);
	assert.equal(view.state, "limited");
	assert.deepEqual(view.primaryAction, { kind: "upgrade", label: "Upgrade" });
	assert.match(view.cloudBlockedReason, /Starter monthly allowance is used up/);
	assert.match(view.cloudBlockedReason, /Local models keep working/);
});

test("past due and cancelled route to billing management, not upgrade", () => {
	assert.deepEqual(
		buildAccountView(cloudSummary({ state: "past_due" }), true).primaryAction,
		{ kind: "manage", label: "Manage billing" }
	);
	assert.equal(buildAccountView(cloudSummary({ state: "canceled" }), true).state, "canceled");
	assert.match(
		buildAccountView(cloudSummary({ state: "canceled" }), true).cloudBlockedReason,
		/no longer active/
	);
});

test("an unmetered account shows no dollar figures", () => {
	const view = buildAccountView(
		cloudSummary({ plan: { name: "Pro", metered: false, usedUsd: 0, source: "cloud" } }),
		true
	);
	assert.equal(view.planHasDollars, false);
	assert.equal(view.cloudBlockedReason, null);
});

test("the local/dev stand-in is flagged so the UI can label it", () => {
	const view = buildAccountView(
		{
			signedIn: true,
			state: "active",
			environment: "local",
			source: "dev_seed",
			displayName: "Synth Dev",
			plan: {
				name: "Synth Dev",
				metered: true,
				monthlyAllowanceUsd: 200,
				usedUsd: 13,
				remainingUsd: 187,
				source: "dev_seed"
			}
		},
		true
	);
	assert.equal(view.planIsDevSeed, true);
	assert.equal(view.planHasDollars, true);
});

test("a stale snapshot keeps rendering and says so", () => {
	const view = buildAccountView(
		cloudSummary({ stale: true, error: "Synth Cloud is unavailable right now" }),
		true
	);
	assert.equal(view.state, "active");
	assert.match(view.statusNote, /Showing the last known plan/);
	assert.match(view.statusNote, /unavailable/);
});

test("a failed snapshot in prod offers retry and no plan", () => {
	const view = buildAccountView(
		{ signedIn: true, state: "error", environment: "prod", source: "none", error: "Synth Cloud is unavailable right now" },
		true
	);
	assert.equal(view.plan, null);
	assert.equal(view.planHasDollars, false);
	assert.deepEqual(view.primaryAction, { kind: "retry", label: "Retry" });
});

test("dollars format to two places, and nullish reads as zero rather than NaN", () => {
	assert.equal(formatUsd(157.5), "$157.50");
	assert.equal(formatUsd(undefined), "$0.00");
	assert.equal(formatUsd(Number.NaN), "$0.00");
});
