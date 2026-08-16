import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "./browser.fixture";

/**
 * The renderer's half of the diagnostic contract.
 *
 * The failure that motivated this system was visible only as a `console.error`
 * in a webview nobody had open. These tests run the real emitter module in a
 * real browser with the Tauri IPC stubbed, and assert the structured command
 * actually goes out — a console line is not a record.
 */

type RecordedInvoke = { command: string; args: Record<string, unknown> };

async function withStubbedIpc(page: import("@playwright/test").Page): Promise<void> {
	await page.evaluate(() => {
		const recorded: RecordedInvoke[] = [];
		(window as typeof window & { __recordedInvokes: RecordedInvoke[] }).__recordedInvokes = recorded;
		(window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
			invoke: async (command: string, args: Record<string, unknown>) => {
				recorded.push({ command, args });
				return null;
			},
			transformCallback: (callback: unknown) => callback
		};
	});
}

test("a renderer failure is emitted as a structured diagnostic, not only a console line", async ({ page }) => {
	await withStubbedIpc(page);
	const recorded = await page.evaluate(async () => {
		const diagnostics = await import("/src/runtime/diagnostics.ts");
		diagnostics.resetDiagnosticThrottle();
		diagnostics.reportDiagnostic({
			severity: "error",
			component: "visual-host",
			event: "visual.projection.rejected",
			code: diagnostics.DIAGNOSTIC_CODES.unsupportedTraceProjectionSchema,
			message: "Unsupported trace projection schema: synth.trace.v5",
			visualId: "vis_9",
			visualRevision: 14,
			traceId: "trace_1",
			details: { receivedSchema: "synth.trace.v5" }
		});
		await new Promise((resolve) => setTimeout(resolve, 50));
		return (window as typeof window & { __recordedInvokes: RecordedInvoke[] }).__recordedInvokes;
	});

	expect(recorded).toHaveLength(1);
	expect(recorded[0].command).toBe("diagnostics_report");
	const request = recorded[0].args.request as Record<string, unknown>;
	expect(request.code).toBe("unsupported_trace_projection_schema");
	expect(request.component).toBe("visual-host");
	expect(request.severity).toBe("error");
	// Every identity the renderer held has to travel, or the backend cannot
	// correlate the blank pane to the rollout that produced it.
	expect(request.visualId).toBe("vis_9");
	expect(request.visualRevision).toBe(14);
	expect(request.traceId).toBe("trace_1");
	expect((request.details as Record<string, unknown>).receivedSchema).toBe("synth.trace.v5");
});

test("identical failures are collapsed so a re-rendering boundary cannot flood the queue", async ({ page }) => {
	await withStubbedIpc(page);
	const recorded = await page.evaluate(async () => {
		const diagnostics = await import("/src/runtime/diagnostics.ts");
		diagnostics.resetDiagnosticThrottle();
		for (let index = 0; index < 25; index += 1) {
			diagnostics.reportDiagnostic({
				severity: "error",
				component: "visual-host",
				event: "visual.render.failed",
				code: diagnostics.DIAGNOSTIC_CODES.visualRenderFailed,
				message: "shell threw",
				visualId: "vis_loop"
			});
		}
		// A different visual is a different fact and must still be recorded.
		diagnostics.reportDiagnostic({
			severity: "error",
			component: "visual-host",
			event: "visual.render.failed",
			code: diagnostics.DIAGNOSTIC_CODES.visualRenderFailed,
			message: "shell threw",
			visualId: "vis_other"
		});
		await new Promise((resolve) => setTimeout(resolve, 50));
		return (window as typeof window & { __recordedInvokes: RecordedInvoke[] }).__recordedInvokes;
	});

	expect(recorded).toHaveLength(2);
	expect((recorded[0].args.request as Record<string, unknown>).visualId).toBe("vis_loop");
	expect((recorded[1].args.request as Record<string, unknown>).visualId).toBe("vis_other");
});

test("credential-shaped and prompt-shaped details never leave the renderer", async ({ page }) => {
	await withStubbedIpc(page);
	const recorded = await page.evaluate(async () => {
		const diagnostics = await import("/src/runtime/diagnostics.ts");
		diagnostics.resetDiagnosticThrottle();
		diagnostics.reportDiagnostic({
			severity: "error",
			component: "renderer",
			event: "provider.disconnected",
			code: diagnostics.DIAGNOSTIC_CODES.providerDisconnected,
			message: "connection interrupted",
			details: {
				authorization: "Bearer sk-abcdefghijklmnop",
				api_key: "sk-live-secret",
				prompt: "the entire system prompt",
				status: 503
			}
		});
		await new Promise((resolve) => setTimeout(resolve, 50));
		return (window as typeof window & { __recordedInvokes: RecordedInvoke[] }).__recordedInvokes;
	});

	const encoded = JSON.stringify(recorded);
	expect(encoded).not.toContain("sk-abcdefghijklmnop");
	expect(encoded).not.toContain("sk-live-secret");
	expect(encoded).not.toContain("the entire system prompt");
	expect(encoded).toContain("503");
});

test("a failure to report a failure is never a second failure", async ({ page }) => {
	await page.evaluate(() => {
		(window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
			invoke: async () => {
				throw new Error("backend unavailable");
			},
			transformCallback: (callback: unknown) => callback
		};
	});
	const threw = await page.evaluate(async () => {
		const diagnostics = await import("/src/runtime/diagnostics.ts");
		diagnostics.resetDiagnosticThrottle();
		try {
			diagnostics.reportDiagnostic({
				severity: "error",
				component: "renderer",
				event: "visual.render.failed",
				code: diagnostics.DIAGNOSTIC_CODES.visualRenderFailed,
				message: "shell threw"
			});
			await new Promise((resolve) => setTimeout(resolve, 50));
			return false;
		} catch {
			return true;
		}
	});
	expect(threw).toBe(false);
});

/**
 * The Diagnostics pane is rendered in isolation here rather than driven through
 * a chat: mounting the real component against stubbed IPC exercises exactly the
 * markup a user sees, without the fixture weight of a live session.
 */
async function mountDiagnosticsPane(
	page: import("@playwright/test").Page,
	result: Record<string, unknown>
): Promise<void> {
	await page.evaluate(async (payload) => {
		(window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
			invoke: async (command: string) => {
				if (command === "diagnostics_status") {
					return {
						state: "degraded",
						reason: "binary_missing",
						local_only: true,
						retention_days: 7,
						quota_bytes: 2147483648,
						index_bytes: 0,
						stored_events: 2,
						queue: { depth: 0, capacity: 10240 }
					};
				}
				if (command === "diagnostics_query") return payload;
				if (command === "diagnostics_explain") {
					return {
						cause: {
							event_id: "diag_cause",
							code: "container_capability_rejected",
							component: "containers",
							message: "container ctr_1 does not declare rollouts/start",
							rank: 10,
							correlation: {}
						},
						symptoms: [
							{
								event_id: "diag_symptom",
								code: "unsupported_trace_projection_schema",
								component: "visual-host",
								rank: 50
							}
						],
						remediation: "Re-probe the container, then start only against a declared capability set.",
						matched: 2,
						identities: {}
					};
				}
				return null;
			},
			transformCallback: (callback: unknown) => callback
		};
		// `page.evaluate` runs outside Vite's module graph, so bare specifiers
		// like "react" are not rewritten and will not resolve. Scan the URLs Vite
		// already rewrote for the app's own entry instead of guessing them; a
		// changed rewriting fails loudly here rather than testing nothing.
		const entry = await fetch("/src/main.tsx").then((response) => response.text());
		const specifiers = entry.split('"').filter((token) => token.startsWith("/"));
		const resolve = (suffix: string) => {
			const hit = specifiers.find((token) => token.split("?")[0].endsWith(suffix));
			if (!hit) throw new Error(`could not resolve ${suffix}; saw ${specifiers.join(", ")}`);
			return hit;
		};
		const [domModule, reactModule, panel] = await Promise.all([
			import(/* @vite-ignore */ resolve("react-dom_client.js")),
			import(/* @vite-ignore */ resolve("/react.js")),
			import("/src/components/DiagnosticsPanel.tsx")
		]);
		// Vite's optimized deps are CJS interop: named exports may live on the
		// module or on its default.
		const interop = <T,>(module: unknown): T => {
			const candidate = module as { default?: unknown };
			return ((candidate as { createElement?: unknown; createRoot?: unknown }).createElement ??
				(candidate as { createRoot?: unknown }).createRoot
				? candidate
				: candidate.default) as T;
		};
		const react = interop<{ createElement: (...args: unknown[]) => unknown }>(reactModule);
		const { createRoot } = interop<{ createRoot: (container: Element) => { render: (node: unknown) => void } }>(domModule);
		const host = document.createElement("div");
		host.id = "diagnostics-test-host";
		document.body.appendChild(host);
		createRoot(host).render(react.createElement(panel.DiagnosticsPanel, { sessionId: "sess_1" }));
	}, result);
	await page.getByTestId("diagnostics-panel").waitFor();
}

const SAMPLE_RESULT = {
	source: "journal",
	count: 1,
	truncated: false,
	groups: [
		{
			code: "unsupported_trace_projection_schema",
			count: 1,
			severity: "error",
			component: "visual-host",
			message: "Unsupported trace projection schema: synth.trace.v5",
			first_seen: "2026-08-16T00:00:00Z"
		}
	],
	events: [
		{
			journal_sequence: 2,
			event_id: "diag_1",
			timestamp: "2026-08-16T00:00:00Z",
			severity: "error",
			component: "visual-host",
			event: "visual.projection.rejected",
			code: "unsupported_trace_projection_schema",
			message: "Unsupported trace projection schema: synth.trace.v5",
			visual_id: "vis_9",
			rollout_id: "roll_7"
		}
	]
};

test("the diagnostics pane states its local-only and degraded status plainly", async ({ page }) => {
	await mountDiagnosticsPane(page, SAMPLE_RESULT);
	const status = page.getByTestId("diagnostics-status");
	await expect(status).toHaveAttribute("data-state", "degraded");
	await expect(status).toContainText("Local only");
	// Degraded must not read as "no evidence": the event is still listed.
	await expect(page.getByTestId("diagnostics-event")).toHaveCount(1);
	await expect(page.getByTestId("diagnostics-events")).toContainText("synth.trace.v5");
});

test("explain names the upstream cause in the pane, not only to the agent", async ({ page }) => {
	await mountDiagnosticsPane(page, SAMPLE_RESULT);
	await page.getByTestId("diagnostics-explain").click();
	const explanation = page.getByTestId("diagnostics-explanation");
	await expect(explanation).toBeVisible();
	await expect(explanation).toContainText("container_capability_rejected");
	await expect(explanation).toContainText("Re-probe the container");
	// The symptom is shown as a symptom, not as a second cause.
	await expect(explanation).toContainText("unsupported_trace_projection_schema");
});

test("the diagnostics pane has no serious accessibility violations", async ({ page }) => {
	await mountDiagnosticsPane(page, SAMPLE_RESULT);
	const axe = await new AxeBuilder({ page })
		.include('[data-testid="diagnostics-panel"]')
		.withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
		.analyze();
	const blocking = axe.violations.filter(
		(violation) => violation.impact === "critical" || violation.impact === "serious"
	);
	expect(blocking, JSON.stringify(blocking, null, 2)).toHaveLength(0);
});
