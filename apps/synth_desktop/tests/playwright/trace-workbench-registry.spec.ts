import { expect, test } from "./browser.fixture";
import { installVisuals, liveVisual, openVisual } from "./v02-helpers";

test("mirrored workstations respond to each pane width without crossing the gutter", async ({ page }) => {
	await page.setViewportSize({ width: 1280, height: 900 });
	const run = { schemaVersion: "optimizer_run.v1", id: "opt_eval_mirror", algorithmId: "eval", status: "completed", objective: "Mirrored workstation", summary: { task: "dungeongrid", bounds: { maximumRollouts: 1 } }, usage: {} };
	const events = [{ type: "eval.trial.started", delta: { workItemId: "eval:trial:0", trial_id: "trial:mirror:0", rollout_id: "roll_mirror", seed: 1, pool: "train", scenario: "dungeongrid/coordination" } }];
	// Frame-centric branding exercises both columns even without retained images.
	await installVisuals(page, [liveVisual({ id: "vis_mirror_workbench", templateId: "craftax.trace_workbench.v1", title: "Mirrored workstation", bindings: { schemaVersion: "synth.visual-bindings.v1", inputs: [{ input: "optimizer_run", kind: "optimizer_run", data: { run, events } }] } })]);
	await page.getByTestId("open-visuals").click();
	await page.getByTestId("visuals-row-vis_mirror_workbench").click();
	await page.getByRole("button", { name: "Expand", exact: true }).click();
	await page.getByLabel("More actions for Mirrored workstation", { exact: true }).click();
	await page.getByRole("menuitem", { name: "Open mirrored pane" }).click();
	const mirror = page.locator(".visual-live-mirror");
	await expect(mirror.getByTestId("craftax-trace-workbench")).toHaveCount(2);
	for (const pane of await mirror.locator(":scope > *").all()) {
		await expect(pane.locator(".trace-workbench-layout")).toBeVisible();
		const geometry = await pane.evaluate((root) => {
			const frame = root.querySelector(".trace-workbench-frame-column")!.getBoundingClientRect();
			const calls = root.querySelector(".trace-workbench-call-column")!.getBoundingClientRect();
			const bounds = root.getBoundingClientRect();
			return { paneWidth: bounds.width, verticalGap: calls.top - frame.bottom, overflow: root.scrollWidth - root.clientWidth, rightOverflow: Math.max(frame.right, calls.right) - bounds.right };
		});
		expect(geometry.paneWidth).toBeLessThan(700);
		expect(geometry.verticalGap).toBeGreaterThanOrEqual(-1);
		expect(geometry.overflow).toBeLessThanOrEqual(1);
		expect(geometry.rightOverflow).toBeLessThanOrEqual(1);
	}
});

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
	const paneBody = pane;
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

/**
 * Expanded-workbench scroll regression.
 *
 * Native acceptance reported the summary and frame overlapping in the expanded
 * workbench "while scrolling", which the narrow-library sweep above does not
 * reach. The run summary is `position: sticky; top: 0` above 420px, so it is
 * *meant* to sit over the scrolling content — the defect a sticky header can
 * actually have is being taller than the space it sticks in, at which point it
 * covers the frame permanently and no amount of scrolling reveals it.
 *
 * This measures that: after scrolling to the bottom, some of the frame column
 * must still be visible below the pinned summary. It changes no layout; the
 * sticky design belongs to the visuals owner.
 */
test("the pinned run summary never swallows the frame in an expanded workbench", async ({ page }) => {
	const run = {
		schemaVersion: "optimizer_run.v1",
		id: "opt_eval_expanded_scroll",
		algorithmId: "eval",
		status: "completed",
		objective: "Expanded workstation",
		summary: { task: "dungeongrid", bounds: { maximumRollouts: 8 } },
		usage: {}
	};
	// Enough trials that the summary carries its full block and the page scrolls.
	const events = Array.from({ length: 8 }, (_, index) => ({
		type: "eval.trial.started",
		delta: {
			workItemId: `eval:trial:${index}`,
			trial_id: `trial:dungeongrid:${index}`,
			rollout_id: `roll_${index}`,
			seed: index,
			pool: "train",
			scenario: "dungeongrid/coordination"
		}
	}));
	await installVisuals(page, [liveVisual({
		id: "vis_expanded_scroll_workbench",
		templateId: "trace.workbench.v1",
		title: "Expanded workstation",
		bindings: {
			schemaVersion: "synth.visual-bindings.v1",
			inputs: [{ input: "optimizer_run", kind: "optimizer_run", data: { run, events } }]
		}
	})]);

	await page.getByTestId("open-visuals").click();
	await page.getByTestId("visuals-row-vis_expanded_scroll_workbench").click();
	const preview = page.getByTestId("visuals-preview");
	const workbench = preview.getByTestId("trace-workbench");
	await expect(workbench).toBeVisible();

	// Short viewports are where a tall pinned summary runs out of room.
	for (const height of [900, 700, 560]) {
		await page.setViewportSize({ width: 1440, height });
		await expect(workbench).toBeVisible();

		const geometry = await workbench.evaluate((root) => {
			const summary = root.querySelector(".trace-workbench-aggregate") as HTMLElement | null;
			const frame = root.querySelector(".trace-workbench-frame-column") as HTMLElement | null;
			if (!summary || !frame) return null;
			// Scroll whichever ancestor actually scrolls, then measure.
			let scroller: HTMLElement | null = root;
			while (scroller && scroller.scrollHeight <= scroller.clientHeight + 1) {
				scroller = scroller.parentElement;
			}
			if (scroller) scroller.scrollTop = scroller.scrollHeight;
			const summaryRect = summary.getBoundingClientRect();
			const frameRect = frame.getBoundingClientRect();
			const covered = Math.max(0, Math.min(summaryRect.bottom, frameRect.bottom) - Math.max(summaryRect.top, frameRect.top));
			return {
				scrolled: Boolean(scroller),
				summaryHeight: summaryRect.height,
				frameHeight: frameRect.height,
				uncovered: frameRect.height - covered,
				viewport: window.innerHeight
			};
		});

		// A pinned panel that content scrolls beneath has to hide it. Anything
		// translucent reads as the two boxes overlapping.
		const background = await workbench.locator(".trace-workbench-aggregate").evaluate((element) =>
			getComputedStyle(element).backgroundColor
		);
		expect(background, `the pinned summary must be opaque at ${height}px`).not.toMatch(/rgba\(.*,\s*0?\.\d+\)$/);

		expect(geometry, `the ${height}px layout must render a summary and a frame column`).not.toBeNull();
		const measured = geometry!;
		// A pinned header taller than its own viewport can never be scrolled past.
		expect(
			measured.summaryHeight,
			`the pinned summary must leave room to scroll at ${height}px viewport`
		).toBeLessThan(measured.viewport);
		expect(
			measured.uncovered,
			`some of the frame must stay visible below the pinned summary at ${height}px`
		).toBeGreaterThan(0);
	}
});
