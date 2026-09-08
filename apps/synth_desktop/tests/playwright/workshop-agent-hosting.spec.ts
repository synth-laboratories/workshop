import { expect, test } from "./browser.fixture";

test("hosted ACP tasks use native commands and keep human approvals explicit", async ({ page }) => {
	// Install after boot so the ordinary browser fixture retains its mock
	// bridge. Only the panel's generated command edge is substituted here.
	await page.evaluate(() => {
		let started = false;
		let pending = false;
		let decision = "";
		(window as any).__TAURI_INTERNALS__ = { transformCallback: () => 1, unregisterCallback: () => undefined, invoke: async (command: string, args: any) => {
			if (command.startsWith("plugin:event|")) return 1;
			if (command === "agent_backends_list") return [{ id: "fixture" }];
			if (command === "agent_sessions_list") return { sessions: started ? [{ sessionId: "acp-test", title: "Fixture task", backendId: "fixture", status: "ready", attached: true }] : [] };
			if (command === "agent_session_start") { started = true; return { sessionId: "acp-test" }; }
			if (command === "agent_session_send") { pending = true; return { accepted: true, runId: "run-test" }; }
			if (command === "core_session_events_tail") return pending ? [{ eventId: "approval-event", sequence: 1, sessionId: "acp-test", kind: "approval.requested", payload: { approvalId: "approval-test", detail: "Fixture asks to modify a file" } }, ...(decision ? [{ eventId: "decision-event", sequence: 2, sessionId: "acp-test", kind: "approval.rejected", payload: { approvalId: "approval-test" } }] : [])] : [];
			if (command === "codex_approval_resolve") { decision = args.request.decision; (window as any).__acpDecision = args.request; return null; }
			throw new Error(`Unexpected panel command: ${command}`);
		} };
	});
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu-settings").click();
	await page.getByTestId("settings-nav-context").click();
	const panel = page.getByTestId("agent-hosting-panel");
	await expect(panel).toBeVisible();
	await panel.getByLabel("Agent", { exact: true }).selectOption("fixture");
	await panel.getByRole("button", { name: "Start task", exact: true }).click();
	await panel.getByLabel("Prompt", { exact: true }).fill("Inspect this project");
	await expect(panel.getByRole("button", { name: "Send", exact: true })).toBeEnabled();
	await panel.getByRole("button", { name: "Send", exact: true }).click();
	await expect(panel.getByText("Fixture asks to modify a file", { exact: true })).toBeVisible();
	await expect(panel.getByRole("button", { name: "Allow once", exact: true })).toBeVisible();
	await panel.getByRole("button", { name: "Reject", exact: true }).click();
	await expect.poll(() => page.evaluate(() => (window as any).__acpDecision)).toEqual({ sessionId: "acp-test", approvalId: "approval-test", decision: "reject" });
	await expect(panel.getByRole("button", { name: "Allow once", exact: true })).not.toBeVisible();
});
