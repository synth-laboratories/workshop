import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../../..", import.meta.url);

test("installed and development windows share the v0.1 traffic-light anchor", async () => {
	const config = JSON.parse(await readFile(new URL("apps/synth_desktop/src-tauri/tauri.conf.json", root), "utf8"));
	assert.deepEqual(config.app.windows[0].trafficLightPosition, { x: 20, y: 22 });

	const instanceScript = await readFile(new URL("scripts/desktop-instance.sh", root), "utf8");
	assert.match(instanceScript, /"trafficLightPosition": \{ "x": 20, "y": 22 \}/);
	assert.doesNotMatch(instanceScript, /"trafficLightPosition": \{ "x": 16, "y": 13 \}/);
});
