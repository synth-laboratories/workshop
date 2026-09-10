import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = new URL("../../../", import.meta.url);
const script = fileURLToPath(new URL("scripts/build-tier.sh", root));
const build = readFileSync(script, "utf8");

test("macOS browser return is registered, delivered, and never authenticates", () => {
  const plist = readFileSync(new URL("apps/synth_desktop/src-tauri/Info.plist", root), "utf8");
  assert.match(plist, /CFBundleURLTypes[\s\S]*CFBundleURLSchemes[\s\S]*<string>synth-workshop<\/string>/);
  const host = readFileSync(new URL("apps/synth_desktop/src-tauri/src/lib.rs", root), "utf8");
  assert.match(host, /RunEvent::Opened \{ urls \}[\s\S]*desktop_links::open\(app, url.as_str\(\)\)/);
  const links = readFileSync(new URL("apps/synth_desktop/src-tauri/src/desktop_links.rs", root), "utf8");
  assert.match(links, /raw != "synth-workshop:\/\/auth-return"/);
  assert.match(links, /parse_workshop_deep_link\(raw\)/);
  assert.match(links, /control\(&app, "attach"\)/);
  assert.doesNotMatch(links, /set_credential|set_api_key|poll_device|exchange_token/);
  const archive = readFileSync(new URL("scripts/build.sh", root), "utf8");
  assert.match(archive, /plutil -extract CFBundleURLTypes\.0\.CFBundleURLSchemes\.0/);
});

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

test("source builds resolve public pinned inputs and explicitly use ad-hoc signing", () => {
  const local = readFileSync(new URL("scripts/workshop.sh", root), "utf8");
  assert.match(local, /source "\$ROOT\/scripts\/prepare-build-sources.sh"/);
  assert.match(local, /SYNTH_APP_SIGN_IDENTITY=- SYNTH_SIGN_IDENTITY=- APPLE_SIGNING_IDENTITY=-/);
  assert.doesNotMatch(local, /codesign --force --deep/);
  const sources = readFileSync(new URL("scripts/prepare-build-sources.sh", root), "utf8");
  assert.match(sources, /credential.helper=/);
  assert.match(sources, /GIT_TERMINAL_PROMPT=0/);
  assert.doesNotMatch(sources, /tblite|reset --hard/);
});

test("build receipts bind the source present before compilation", () => {
  assert.ok(build.indexOf("BUILD_SOURCE_REVISION=") < build.indexOf('"$ROOT/scripts/stage-mlx-runtime-distribution.sh"'));
  assert.match(build, /source changed during build; refusing/);
  assert.match(build, /commit = "\$BUILD_SOURCE_REVISION"/);
  assert.match(build, /dirty = "\$BUILD_SOURCE_DIRTY" == "true"/);
});

test("local build prerequisites reject a pre-Swift-6 terminal toolchain", () => {
  const install = readFileSync(new URL("scripts/install.sh", root), "utf8");
  assert.match(install, /swift_major >= 6/);
  assert.match(install, /Select Xcode 16\+/);
});
