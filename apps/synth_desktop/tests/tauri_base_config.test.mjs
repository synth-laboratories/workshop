/**
 * P0-14 lock: the base `tauri.conf.json` carries no `bundle.resources`.
 *
 * tauri-build validates every resource path at `cargo check` time, so a
 * resource that only exists after a staging script (packaged cookbooks, the
 * Computer Use helper bundle) makes a fresh worktree unable to compile or run
 * library tests. Packaged resources live in `tauri.package.json`, which every
 * build entry point passes with `--config` (the instance overlay chains after
 * it). Failing here means a resource crept back into the base config or an
 * entry point stopped passing the overlay.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(appRoot, "../..");

function readJson(rel) {
	return JSON.parse(readFileSync(join(appRoot, rel), "utf8"));
}

test("base tauri.conf.json declares no bundle.resources", () => {
	const base = readJson("src-tauri/tauri.conf.json");
	assert.equal(base.bundle.resources, undefined, "bundle.resources belongs in tauri.package.json");
	const text = JSON.stringify(base);
	assert.doesNotMatch(text, /generated-resources/);
	assert.doesNotMatch(text, /helpers\//);
});

test("tauri.package.json is the one place packaged resources are declared", () => {
	const overlay = readJson("src-tauri/tauri.package.json");
	const resources = overlay.bundle?.resources ?? {};
	assert.equal(resources["generated-resources/cookbooks"], "cookbooks");
	const helper = Object.entries(resources).find(([source]) => source.includes("helpers/synth-computer-use"));
	assert.ok(helper, "Computer Use helper bundle is a packaged resource");
	assert.equal(helper[1], "Synth Computer Use.app");
	const keys = Object.keys(overlay);
	assert.deepEqual(keys.filter((key) => key !== "$schema"), ["bundle"], "overlay carries only packaging keys");
	assert.deepEqual(Object.keys(overlay.bundle), ["resources"]);
});

test("every build entry point passes the packaging overlay", () => {
	const pkg = readJson("package.json");
	assert.match(pkg.scripts.build, /tauri build --config src-tauri\/tauri\.package\.json/);
	const instance = readFileSync(join(repoRoot, "scripts/desktop-instance.sh"), "utf8");
	assert.match(instance, /PACKAGE_CONFIG="src-tauri\/tauri\.package\.json"/);
	assert.match(instance, /tauri build --debug [^\n]*--config "\$PACKAGE_CONFIG" --config "\$CONFIG"/);
	assert.match(instance, /tauri dev [^\n]*--config "\$PACKAGE_CONFIG" --config "\$CONFIG"/);
	assert.doesNotMatch(instance, /tauri (dev|build)(?![^\n]*PACKAGE_CONFIG)[^\n]*--config "\$CONFIG"/);
	const desktop = readFileSync(join(repoRoot, "scripts/desktop.sh"), "utf8");
	assert.match(desktop, /tauri build --bundles app --config src-tauri\/tauri\.package\.json/);
});

test("isolated packaged QA launches preserve operator-provided SFT dataset paths", () => {
	const instance = readFileSync(join(repoRoot, "scripts/desktop-instance.sh"), "utf8");
	assert.match(instance, /SYNTH_MLX_SFT_TRAIN_JSONL="\$sft_train_jsonl"/);
	assert.match(instance, /SYNTH_MLX_SFT_EVAL_JSONL="\$sft_eval_jsonl"/);
});
