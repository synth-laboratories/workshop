import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("../../../scripts/build-computer-use-helper.sh", import.meta.url));
function check(overrides = {}) {
  return spawnSync("bash", [script, "check-notary-auth"], {
    encoding: "utf8",
    env: {
      ...process.env,
      SYNTH_NOTARY_KEY_PATH: "",
      SYNTH_NOTARY_KEY_ID: "",
      SYNTH_NOTARY_ISSUER: "",
      SYNTH_NOTARY_PROFILE: "",
      SYNTH_ALLOW_KEYCHAIN: "",
      ...overrides,
    },
  });
}

test("notarization refuses missing credentials without prompting", () => {
  assert.notEqual(check().status, 0);
});

test("a configured Keychain profile does not authorize its use", () => {
  const result = check({ SYNTH_NOTARY_PROFILE: "test-profile" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /explicit authorization/);
});

test("API key selection validates without accessing Keychain or submitting", () => {
  // Only existence is checked; this fixture is not a credential and is never read.
  const result = check({ SYNTH_NOTARY_KEY_PATH: script, SYNTH_NOTARY_KEY_ID: "test-id" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /no submission performed/);
});

test("ambiguous notarization credentials fail closed", () => {
  const result = check({ SYNTH_NOTARY_KEY_PATH: script, SYNTH_NOTARY_KEY_ID: "test-id", SYNTH_NOTARY_PROFILE: "test-profile" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /not both/);
});
