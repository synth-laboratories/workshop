import { expect, test } from "./browser.fixture";
import { installVisuals, liveVisual, openVisual } from "./v02-helpers";

test("persisted trace.workbench.v1 resolves through the bundled registry and VisualHost", async ({ page }) => {
	const run = {
		schemaVersion: "optimizer_run.v1",
		id: "opt_eval_persisted_trace",
		algorithmId: "eval",
		status: "completed",
		objective: "Persisted NanoHorizon trace",
		summary: {
			task: "NanoHorizon",
			bounds: { maximumRollouts: 0 }
		},
		usage: {}
	};
	await installVisuals(page, [liveVisual({
		id: "vis_persisted_trace_workbench",
		templateId: "trace.workbench.v1",
		title: "Persisted trace workstation",
		bindings: {
			schemaVersion: "synth.visual-bindings.v1",
			inputs: [{
				input: "optimizer_run",
				kind: "optimizer_run",
				data: { run, events: [] }
			}]
		}
	})]);

	const pane = await openVisual(page, "vis_persisted_trace_workbench");
	await expect(pane.getByTestId("trace-workbench")).toBeVisible();
	await expect(pane).not.toContainText("Template unavailable");
	await expect(pane).not.toContainText("No bundled shell is registered");
	const paneBody = pane.locator(".visual-pane-body");
	await paneBody.evaluate((element) => {
		Object.assign((element as HTMLElement).style, {
			alignSelf: "flex-end",
			flex: "0 0 340px",
			width: "340px",
			maxWidth: "340px"
		});
	});
	await expect(pane.getByTestId("trace-run-aggregates")).toHaveCSS("position", "static");
	const usageColumns = await pane.locator(".trace-workbench-usage").evaluate((element) =>
		getComputedStyle(element).gridTemplateColumns.split(" ").filter(Boolean).length
	);
	expect(usageColumns).toBe(1);
});
