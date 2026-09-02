import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const page = readFileSync(join(root, "src/renderer/src/components/OptimizersPage.tsx"), "utf8");
const service = readFileSync(join(root, "src-tauri/src/plugins/service.rs"), "utf8");
const ipc = readFileSync(join(root, "src-tauri/src/visuals_ipc.rs"), "utf8");

test("Optimizers exposes bounded restart and update-and-restart controls", () => {
	assert.match(page, /operation: "restart"/);
	assert.match(page, /label: "Restart service"/);
	assert.match(page, /label: "Update & restart"/);
	assert.match(page, /Runs, artifacts, and visuals are retained/);
	assert.match(service, /"restart" => \{/);
	assert.match(service, /manager\.stop\(\)\.await\?/);
	assert.match(service, /manager\.start\(\)\.await\?/);
	assert.match(service, /"stop" \| "restart" \| "remove"/);
});

test("evaluator restart refuses active optimizer work before approval", () => {
	const guard = ipc.match(/container_restart_blocked_active_optimizer_runs[\s\S]{0,500}/)?.[0] ?? "";
	assert.match(ipc, /OptimizerRunStatus::str_is_terminal/);
	assert.match(ipc, /binding\.kind == "container_http"/);
	assert.match(ipc, /\.get\("containerId"\)/);
	assert.match(guard, /cancel or finish those runs before restarting/);
});
