import assert from "node:assert/strict";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";
import { chromium } from "playwright";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const source = join(appRoot, "../../visuals/families/experiments/experiment.overview.v1/shell.tsx");
const bundle = buildSync({
	stdin: {
		contents: `
			import React from "react";
			import { createRoot } from "react-dom/client";
			import { Shell } from ${JSON.stringify(source)};
			let root;
			window.evalHarness = {
				render(props) {
					root ??= createRoot(document.getElementById("root"));
					root.render(React.createElement(Shell, props));
				}
			};
		`,
		resolveDir: appRoot,
		sourcefile: "experiment-overview-playback-entry.tsx",
		loader: "tsx"
	},
	bundle: true,
	write: false,
	format: "iife",
	platform: "browser",
	target: "es2022",
	loader: { ".css": "empty" }
}).outputFiles[0].text;

const pixel = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

function frame(sequence, index, elapsedMs) {
	return {
		sequenceNumber: sequence,
		type: "eval.trial.event",
		delta: { trial_id: "trial-live", containerEvent: {
			kind: "frame",
			rollout_id: "rollout-live",
			payload: {
				frame_index: index,
				elapsed_ms: elapsedMs,
				data_url: pixel,
				stream_health: {
					frames_captured: index + 1,
					frames_dropped: 0,
					bytes_captured: (index + 1) * 1000,
					average_capture_latency_ms: 25,
					source_interval_ms: 1000
				}
			}
		} }
	};
}

const action = {
	sequenceNumber: 3,
	type: "eval.trial.event",
	delta: { trial_id: "trial-live", containerEvent: {
		kind: "agent.action",
		rollout_id: "rollout-live",
		payload: { elapsed_ms: 1500, tool: "execute_code", status: "completed", arguments_preview: "await bot.chopTree()" }
	} }
};

test("RuneBench playback follows incoming frames and supports live-edge controls", async () => {
	const browser = await chromium.launch({ headless: true });
	try {
		const page = await browser.newPage();
		await page.setContent("<main id='root'></main>");
		await page.addScriptTag({ content: bundle });
		const running = { experiment: { title: "RuneBench", status: "running" }, run: { status: "running" } };
		await page.evaluate(({ running, events }) => window.evalHarness.render({ ...running, events }), {
			running,
			events: [frame(1, 0, 1000), frame(2, 1, 2000), action]
		});

		const clip = page.getByTestId("runebench-live-frame");
		await clip.waitFor();
		await page.getByText(/frame 2\/2/).waitFor();
		await page.getByText("execute_code").waitFor();

		await page.getByRole("button", { name: "Previous frame" }).click();
		await page.getByText(/frame 1\/2/).waitFor();
		await page.getByLabel("Playback speed").selectOption("4");
		assert.equal(await page.getByLabel("Playback speed").inputValue(), "4");

		await clip.focus();
		await page.keyboard.press("ArrowRight");
		await page.getByText(/frame 2\/2/).waitFor();
		await page.keyboard.press("ArrowLeft");
		await page.getByRole("button", { name: "Jump to live" }).click();

		await page.evaluate(({ running, events }) => window.evalHarness.render({ ...running, events }), {
			running,
			events: [frame(1, 0, 1000), frame(2, 1, 2000), action, frame(4, 2, 3000)]
		});
		await page.getByText(/frame 3\/3/).waitFor();

		await page.evaluate((events) => window.evalHarness.render({
			experiment: { title: "RuneBench", status: "completed" },
			run: { status: "completed" },
			events
		}), [frame(1, 0, 1000), frame(2, 1, 2000), action, frame(4, 2, 3000)]);
		await page.getByRole("button", { name: "Jump to latest" }).waitFor();
	} finally {
		await browser.close();
	}
});
