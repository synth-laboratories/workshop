/**
 * Static design-debt detectors. Failures here mean a known stub/smell is gone
 * (good — update the assertion) or a new one appeared (bad — investigate).
 *
 * Expected-fail style: we assert the smell *exists* so CI stays green while
 * documenting debt; flip to assert absence when the design is fixed.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const renderer = join(appRoot, "src/renderer/src");

function read(rel) {
	return readFileSync(join(renderer, rel), "utf8");
}

test("titlebar Account is wired while Downloads/Expand remain tracked debt", () => {
	const app = read("App.tsx");
	assert.doesNotMatch(app, /Account — stub/);
	assert.match(app, /data-testid="open-account-settings"/);
	assert.match(app, /setView\(\{ kind: "settings", section: "account" \}\)/);
	assert.match(app, /Account menu — stub/);
	assert.match(app, /Downloads — stub/);
	assert.match(app, /Expand — stub/);
});

test("design debt: Set up agent remains a toast stub while Laguna reload is a typed bridge", () => {
	const app = read("App.tsx");
	assert.match(app, /Set up agent — stub/);
	assert.match(app, /onReloadLaguna=\{onReloadLaguna\}/);
	assert.doesNotMatch(app, /Reload Laguna — stub/);
	const bridge = read("runtime/desktopBridge.ts");
	assert.match(bridge, /invoke<LagunaStatus>\("laguna_reload"\)/);
	const rust = readFileSync(join(appRoot, "src-tauri/src/lib.rs"), "utf8");
	assert.match(rust, /async fn laguna_reload/);
});

test("design debt: CloudDesk leave-safe is hard-wired to async kind", () => {
	const desk = read("components/CloudDesk.tsx");
	assert.match(desk, /const leaveSafe = !isSync/);
});

test("design debt: Cloud actions other than pause/resume/close/cancel/checkpoint fall through to stub toasts", () => {
	const app = read("App.tsx");
	assert.match(app, /showToast\(`\$\{label\} — stub`\)/);
	assert.ok(
		!app.includes('lower === "provide input"') && !app.includes('lower === "respond"'),
		"Provide input / Respond should be wired to Intern intervene before removing stub fallthrough"
	);
});

test("design debt: agent-authored analysis shell does not normalize the persisted type-block payload", () => {
	const shell = readFileSync(join(appRoot, "../../visuals/templates/analysis.visual.v1/shell.tsx"), "utf8");
	assert.match(shell, /if \(block\.kind === "note"\)/);
	assert.ok(!shell.includes("block.type"), "type-block normalization appeared — flip the CUA regression to a passing test");
	assert.match(shell, /return <Scatter block=\{block\}/);
});

test("composer approval policy control is wired and test-addressable", () => {
	const composer = read("components/Composer.tsx");
	assert.match(composer, /className="permission-select"/);
	const block = composer.slice(
		composer.indexOf('className="permission-select"'),
		composer.indexOf('className="permission-select"') + 320
	);
	assert.ok(block.includes("onClick"));
	assert.ok(block.includes('data-testid="approval-mode-select"'));
	assert.match(composer, /data-testid="approval-mode-menu"/);
});

test("design debt: VisualHost still uses Craftax string heuristics for preview variants", () => {
	const host = read("components/VisualHost.tsx");
	assert.match(host, /templateId\.includes\("craftax"\)/);
	assert.match(host, /templateId\.includes\("scrub"\)/);
});

test("intended design: deferred LoRA support leaves no fixture catalog or placeholder UI", () => {
	const composer = read("components/Composer.tsx");
	const settings = read("components/SettingsPage.tsx");
	const landing = read("types/landing.ts");
	assert.ok(!composer.includes("Laguna LoRAs"));
	assert.ok(!composer.includes("open-finetunes-settings"));
	assert.ok(!settings.includes('id: "finetunes"'));
	assert.ok(!settings.includes("Adapters"));
	assert.ok(!landing.includes("LoraAdapter"));
	assert.ok(!landing.includes("AVAILABLE_LORAS"));
	assert.ok(!landing.includes("selectedLoraId"));
});

test("intended design: Inventory Attach defaults to GameBench Craftax :8098", () => {
	const inventory = read("components/InventoryPage.tsx");
	assert.match(inventory, /127\.0\.0\.1:8098/);
	assert.match(inventory, /data-testid="attach-container"/);
	assert.match(inventory, /data-testid="import-trace-v5"/);
	assert.ok(!inventory.includes("127.0.0.1:8100"), "demo :8100 placeholder should not be the Attach default");
});

test("intended design: styles must not retain a LoRA picker affordance", () => {
	const composer = read("components/Composer.tsx");
	const styles = read("styles/app.css");
	assert.ok(!composer.includes("is-lora"));
	assert.ok(!styles.includes(".is-lora"));
});

test("intended design: Playwright workers use isolated renderer ports", () => {
	const fixture = readFileSync(join(appRoot, "tests/playwright/browser.fixture.ts"), "utf8");
	assert.ok(!fixture.includes("127.0.0.1:1420"));
	assert.match(fixture, /reserveLoopbackPort/);
	assert.match(fixture, /--strictPort/);
});
