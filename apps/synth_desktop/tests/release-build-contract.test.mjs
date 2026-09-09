import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = new URL("../../../", import.meta.url);
const script = fileURLToPath(new URL("scripts/build-tier.sh", root));
const build = readFileSync(script, "utf8");

test("packaging rejects retired envelopes and invalid options before staging", () => {
  for (const args of [["beta"], ["all"], ["core"], ["stable", "--features"], ["stable", "--debug", "extra"]]) {
    const result = spawnSync("bash", [script, ...args], { encoding: "utf8" });
    assert.equal(result.status, 2, result.stderr);
    assert.doesNotMatch(result.stdout, /staging|building|downloading/);
  }
});

test("the single-envelope build does not request removed Cargo tier features", () => {
  const pkg = JSON.parse(readFileSync(new URL("apps/synth_desktop/package.json", root), "utf8"));
  assert.equal(pkg.scripts.build, "../../scripts/build-tier.sh stable");
  const cargo = readFileSync(new URL("apps/synth_desktop/src-tauri/Cargo.toml", root), "utf8");
  assert.doesNotMatch(cargo, /^tier-stable\s*=/m);
  assert.doesNotMatch(build, /--features|WORKSHOP_TIER=/);
});

test("packaging builds and installs adapters before browser finalization and staging", () => {
  assert.match(build, /source "\$ROOT\/scripts\/mcp-adapters.sh"/);
  assert.match(build, /cargo build --locked[\s\S]*--bins/);
  const copy = build.indexOf('Contents/MacOS/$adapter');
  const finalize = build.indexOf('"$ROOT/scripts/finalize-browser-app.sh"');
  const stage = build.indexOf('ditto "$bundle_dir/$product.app" "$out_dir/$product.app"');
  assert.ok(copy > 0 && finalize > copy && stage > finalize);
  assert.match(build, /build-browser-runtime.sh" assemble/);
});

test("archive sealing preserves nested browser JIT entitlements", () => {
  const archive = readFileSync(new URL("scripts/build.sh", root), "utf8");
  assert.doesNotMatch(archive, /codesign --force --deep/);
  assert.match(archive, /codesign --verify --deep --strict/);
});
