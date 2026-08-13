import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const renderer = join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src");
const read = (rel) => readFileSync(join(renderer, rel), "utf8");

test("v0.2 pending approval cards pin above Working in ChatTranscript", () => {
	const source = read("components/ChatTranscript.tsx");
	const approvals = source.indexOf("{pendingApprovals.map((line) => renderActivityLine(line, [], false, false))}");
	const working = source.indexOf('data-testid="model-working"');
	assert.ok(approvals >= 0, "pending approval pin is missing");
	assert.ok(working >= 0, "Working… marker is missing");
	assert.ok(approvals < working, "approval cards must render above Working…");
	assert.match(source, /Approve once/);
	assert.match(source, /Always allow for this session/);
	// Staleness is no longer inferred from run status — an unresolved request is
	// terminalized by a durable approval.expired event, so a restored session
	// shows real history instead of a live-looking card with dead buttons. That
	// behavior is owned by tests/playwright/v02-approval-ux.spec.ts; asserting
	// the old `if (!running) return []` guard here pinned the defect in place.
	assert.doesNotMatch(source, /if \(!running\) return \[\];/);
});

test("v0.2 grouped activity keeps visual and container MCP calls out of used-tools summaries", () => {
	const source = read("preferences/activityPresentation.ts");
	assert.match(source, /export function isAuthoredEvidence/);
	assert.match(source, /synth_visuals/);
	assert.match(source, /synth_containers/);
	assert.match(source, /if \(isAuthoredEvidence\(line\)\) \{\s*\n\s*flush\(\);/);
	assert.match(source, /Ran commands, used tools N calls/);
});

test("v0.2 Full system access is danger-full-access, distinct from workspace-write", () => {
	const source = read("components/Composer.tsx");
	assert.match(source, /id: "danger-full-access", label: "Full system access"/);
	assert.match(source, /id: "workspace-write", label: "Workspace access"/);
	assert.match(source, /unrestricted filesystem and network access/);
	assert.match(source, /"danger-full-access": "Full"/);
});
