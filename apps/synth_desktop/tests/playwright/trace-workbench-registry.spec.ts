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

/**
 * Narrow-library viewport regression.
 *
 * The Visuals library renders the workstation in its own preview column, which
 * is where the run summary and the frame column were reported to overlap. This
 * selects the row and measures realized geometry rather than asserting a CSS
 * rule, so it stays true whichever way the layout is eventually expressed.
 *
 * It changes no layout and uses no shared open helper: the extraction work on
 * these columns, and on the library chrome around them, belongs to the visuals
 * owner. This only pins the invariant their work has to keep.
 */
test("the workstation summary and frame do not overlap in a narrow library window", async ({ page }) => {
	const run = {
		schemaVersion: "optimizer_run.v1",
		id: "opt_eval_narrow_library",
		algorithmId: "eval",
		status: "completed",
		objective: "Narrow library workstation",
		summary: { task: "runebench", bounds: { maximumRollouts: 1 } },
		usage: {}
	};
	const events = [
		{
			type: "eval.trial.started",
			delta: {
				workItemId: "eval:trial:0",
				trial_id: "trial:runebench:0",
				rollout_id: "roll_0",
				seed: 0,
				pool: "train",
				scenario: "runebench/woodcutting"
			}
		}
	];
	await installVisuals(page, [liveVisual({
		id: "vis_narrow_library_workbench",
		templateId: "trace.workbench.v1",
		title: "Narrow library workstation",
		bindings: {
			schemaVersion: "synth.visual-bindings.v1",
			inputs: [{ input: "optimizer_run", kind: "optimizer_run", data: { run, events } }]
		}
	})]);

	await page.getByTestId("open-visuals").click();
	await page.getByTestId("visuals-row-vis_narrow_library_workbench").click();
	const preview = page.getByTestId("visuals-preview");
	const workbench = preview.getByTestId("trace-workbench");

	// The library survives being dragged narrow, not merely one chosen width.
	for (const width of [1180, 960, 820, 700, 620]) {
		await page.setViewportSize({ width, height: 800 });
		await expect(workbench).toBeVisible();
		await expect(preview.locator(".trace-workbench-layout")).toBeVisible();

		const geometry = await workbench.evaluate((root) => {
			const summary = root.querySelector(".trace-workbench-aggregate");
			const frame = root.querySelector(".trace-workbench-frame-column");
			const calls = root.querySelector(".trace-workbench-call-column");
			if (!summary || !frame) return { measured: false, overlaps: [] as string[], overflow: 0 };
			// Sub-pixel contact along a shared edge is a border, not an overlap.
			const overlapping = (a: Element, b: Element) => {
				const one = a.getBoundingClientRect();
				const two = b.getBoundingClientRect();
				return (
					Math.min(one.right, two.right) - Math.max(one.left, two.left) > 1 &&
					Math.min(one.bottom, two.bottom) - Math.max(one.top, two.top) > 1
				);
			};
			const overlaps: string[] = [];
			if (overlapping(summary, frame)) overlaps.push("summary/frame");
			if (calls && overlapping(frame, calls)) overlaps.push("frame/calls");
			if (calls && overlapping(summary, calls)) overlaps.push("summary/calls");
			return { measured: true, overlaps, overflow: root.scrollWidth - root.clientWidth };
		});

		expect(geometry.measured, `the ${width}px layout must render a summary and a frame column to measure`).toBe(true);
		expect(geometry.overlaps, `workstation regions must not overlap at ${width}px`).toEqual([]);
		expect(geometry.overflow, `the workstation must not force horizontal scroll at ${width}px`).toBeLessThanOrEqual(1);
	}
});
