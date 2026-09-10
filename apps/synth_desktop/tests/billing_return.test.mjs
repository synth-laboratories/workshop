import assert from "node:assert/strict";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";
const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dir = join(root, "node_modules/.cache/synth-desktop-tests");
mkdirSync(dir, { recursive: true });
const target = join(dir, "billingReturn.mjs");
writeFileSync(target, transformSync(readFileSync(join(root, "src/renderer/src/runtime/billingReturn.ts"), "utf8"), { loader: "ts", format: "esm" }).code);
const { billingIdentity, billingReturnState, parseBillingReturn, BILLING_RETURN_TTL_MS } = await import(pathToFileURL(target));
const summary = { signedIn: true, accountId: "user", organization: { id: "org" }, environment: "local", plan: { tier: "starter" } };
const pending = { identity: billingIdentity(summary), tier: "starter", startedAt: 1000 };
test("pending checkout survives restart beyond the old four-second window", () => {
 assert.deepEqual(parseBillingReturn(JSON.stringify(pending), 60_000), pending);
});
test("only authoritative matching identity and tier confirm checkout", () => {
 assert.equal(billingReturnState(pending, summary), "confirmed");
 assert.equal(billingReturnState(pending, { ...summary, stale: true }), "pending");
 assert.equal(billingReturnState(pending, { ...summary, plan: { tier: "free" } }), "pending");
 assert.equal(billingReturnState(pending, { ...summary, organization: { id: "other" } }), "changed_identity");
 assert.equal(billingReturnState(pending, { ...summary, signedIn: false }), "changed_identity");
});
test("expired malformed and future state is discarded", () => {
 for (const raw of ["{", "{}", JSON.stringify({ ...pending, tier: "enterprise" }), JSON.stringify({ ...pending, startedAt: 9000 })]) assert.equal(parseBillingReturn(raw, 5000), null);
 assert.equal(parseBillingReturn(JSON.stringify(pending), 1000 + BILLING_RETURN_TTL_MS), null);
});
