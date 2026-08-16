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
