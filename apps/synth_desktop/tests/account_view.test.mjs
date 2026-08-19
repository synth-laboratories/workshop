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
const backendSettingsSource = readFileSync(join(appRoot, "src/renderer/src/components/BackendSettings.tsx"), "utf8");
const nativeCodexSource = readFileSync(join(appRoot, "src/renderer/src/runtime/nativeCodex.ts"), "utf8");

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

test("active free and starter accounts offer the backend-issued upgrade path", () => {
	for (const [tier, upgradeTier] of [["free", "starter"], ["starter", "pro"]]) {
		const view = buildAccountView(
			cloudSummary({
				plan: {
					name: tier === "free" ? "Free" : "Starter",
					tier,
					state: "active",
					metered: true,
					monthlyAllowanceUsd: tier === "free" ? 0 : 20,
					usedUsd: 0,
					remainingUsd: tier === "free" ? 0 : 20,
					source: "cloud"
				},
				billing: {
					checkoutUrl: `https://example.test/usage?upgrade=${upgradeTier}`,
					portalUrl: "https://example.test/usage",
					upgradeTier
				}
			}),
			true
		);
		assert.deepEqual(view.primaryAction, { kind: "upgrade", label: "Upgrade" });
	}
});

test("the renderer never accepts API-key material", () => {
	assert.doesNotMatch(backendSettingsSource, /type=["']password["']/);
	assert.doesNotMatch(backendSettingsSource, /\bapiKey\s*:/);
	assert.doesNotMatch(backendSettingsSource, /\bopenrouterApiKey\s*:/);
	assert.doesNotMatch(nativeCodexSource, /\bapiKey\s*:/);
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
		cloudSummary({ stale: true, error: "Synth Cloud is unavailable right now." }),
		true
	);
	assert.equal(view.state, "active");
	// The exact public sentence the host now produces (`AccountError`), composed
	// into the exact note the product contract specifies.
	assert.equal(
		view.statusNote,
		"Showing the last known plan — Synth Cloud is unavailable right now."
	);
});

test("a failed snapshot in prod offers retry and no plan", () => {
	const view = buildAccountView(
		{ signedIn: true, state: "error", environment: "prod", source: "none", error: "Synth Cloud is unavailable right now." },
		true
	);
	assert.equal(view.plan, null);
	assert.equal(view.planHasDollars, false);
	assert.deepEqual(view.primaryAction, { kind: "retry", label: "Retry" });
});

test("dollars format to two places, and nullish/NaN read as UNKNOWN (never invent $0.00)", () => {
	assert.equal(formatUsd(157.5), "$157.50");
	assert.equal(formatUsd(undefined), "UNKNOWN");
	assert.equal(formatUsd(Number.NaN), "UNKNOWN");
});

// ── `Usage remaining` — the expandable cloud allowance summary ──────────────
//
// Two documents specify this row (`HANDOFF_CLOUD_ACCOUNT_QA.md` A3/C2 and the
// target UX in `synth_cloud_api_usage.md`), and it is the only surface that
// states the allowance without opening a sheet. These tests pin the one
// canonical behavior so implementation and contract cannot drift again.

test("the allowance summary states plan, allowance, used, remaining and resets", () => {
	const { allowance } = buildAccountView(cloudSummary(), true);
	assert.equal(allowance.headline, "$157.50");
	assert.equal(allowance.note, null);
	assert.equal(allowance.isDevSeed, false);
	assert.deepEqual(
		allowance.rows.map((row) => row.label),
		["Plan", "Monthly allowance", "Used this period", "Remaining", "Resets"]
	);
	assert.deepEqual(
		allowance.rows.slice(0, 4).map((row) => row.value),
		["Pro", "$200.00", "$42.50", "$157.50"]
	);
});

test("signed out, the allowance invites sign-in and never shows $0.00", () => {
	for (const state of ["local_only", "signed_out", "pairing"]) {
		const view = buildAccountView({ signedIn: false, state, environment: "prod", source: "none" }, false);
		assert.equal(view.allowance.note, "Sign in to Synth to see a cloud allowance");
		assert.deepEqual(view.allowance.rows, []);
		assert.equal(view.allowance.headline, null);
		assert.equal(
			JSON.stringify(view.allowance).includes("$"),
			false,
			`${state} must not render any dollar figure`
		);
	}
});

test("an unmetered account names its plan but invents no allowance", () => {
	const { allowance } = buildAccountView(
		cloudSummary({ plan: { name: "Research", metered: false, usedUsd: 0 } }),
		true
	);
	assert.equal(allowance.headline, null);
	assert.deepEqual(allowance.rows, [{ label: "Plan", value: "Research" }]);
	assert.equal(allowance.note, "This account is not metered in dollars");
});

test("a signed-in account with no plan says so instead of showing zeros", () => {
	const { allowance } = buildAccountView(
		{ signedIn: true, state: "unknown", environment: "prod", source: "none" },
		true
	);
	assert.deepEqual(allowance.rows, []);
	assert.equal(allowance.note, "Synth Cloud has not reported a plan for this account yet");
	assert.equal(allowance.headline, null);
});

test("the local/dev stand-in allowance is flagged so the menu can label it", () => {
	const { allowance } = buildAccountView(
		cloudSummary({ source: "dev_seed", plan: { ...cloudSummary().plan, source: "dev_seed" } }),
		true
	);
	assert.equal(allowance.isDevSeed, true);
	assert.equal(allowance.headline, "$157.50");
});

test("the allowance summary is cloud-only and never mixes in device usage", () => {
	// Device totals arrive through a different bridge entirely; nothing in the
	// summary may be derived from them.
	const { allowance } = buildAccountView(cloudSummary(), true);
	const labels = allowance.rows.map((row) => row.label.toLowerCase());
	assert.equal(labels.some((label) => label.includes("device")), false);
	assert.equal(labels.some((label) => label.includes("token")), false);
});

test("a revoked session invites sign-in again and keeps the auth error", () => {
	const view = buildAccountView({
		signedIn: false,
		state: "signed_out",
		environment: "prod",
		source: "none",
		sessionHealth: "revoked",
		failureKind: "auth",
		error: "Synth Cloud rejected this device's key. Sign in again to continue.",
		reconciliation: "failed"
	}, false);
	assert.equal(view.signedIn, false);
	assert.equal(view.subtitle, "Sign in again");
	assert.equal(view.primaryAction?.kind, "sign_in");
	assert.equal(view.statusNote, "Synth Cloud rejected this device's key. Sign in again to continue.");
	assert.equal(view.allowance.headline, null);
});
