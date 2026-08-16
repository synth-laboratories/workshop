/**
 * Transcript projection: a container capability preflight failure crosses MCP
 * as `isError: true` and must render as a failed tool call that keeps the
 * stable code and remediation. The regression it guards is the opposite —
 * a rejected prepare rendering as a green "Completed" with the cause buried in
 * a successful result body.
 */
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const source = join(appRoot, "src/renderer/src/runtime/sessionView.ts");
const compiled = join(compiledDir, "sessionView.mjs");
buildSync({
	entryPoints: [source],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile: compiled
});

const { eventsToLocalActivity } = await import(pathToFileURL(compiled).href);

function event(overrides = {}) {
	return {
		schemaVersion: "synth.desktop-runtime-event.v1",
		sessionId: "sess-1",
		sequence: 1,
		eventKind: "item/completed",
		payload: {},
		createdAt: "2026-08-15T00:00:00.000Z",
		source: "local",
		...overrides
	};
}

function preflightFailure(structuredError) {
	return event({
		payload: {
			item: {
				type: "mcpToolCall",
				id: "prepare-1",
				server: "synth_containers",
				tool: "container_prepare_rollout",
				// The provider reported the item itself as completed; only the
				// MCP result says the call failed.
				status: "completed",
				arguments: { container_id: "ctr_33d6ee47de1e430ab80b1403ba04e555" },
				result: {
					isError: true,
					structuredContent: {
						error: "container_capability_mismatch: missing rollouts.prepare",
						terminal: false,
						structuredError
					}
				}
			}
		}
	});
}

const CAPABILITY_MISMATCH = {
	code: "container_capability_mismatch",
	container_id: "ctr_33d6ee47de1e430ab80b1403ba04e555",
	missing: ["rollouts.prepare", "trace_v5.capture"],
	retryable: false,
	remediation: "Select a normalized live-policy pool; this record is a raw environment engine."
};

test("a capability mismatch renders as a failed tool call, not a completed one", () => {
	const activity = eventsToLocalActivity([preflightFailure(CAPABILITY_MISMATCH)], []);
	const lines = Object.values(activity).flat();
	assert.equal(lines.length, 1);
	assert.equal(lines[0].toolStatus, "failed");
	assert.equal(lines[0].label, "synth_containers.container_prepare_rollout");
});

test("the transcript preserves the structured code and remediation", () => {
	const activity = eventsToLocalActivity([preflightFailure(CAPABILITY_MISMATCH)], []);
	const detail = Object.values(activity).flat()[0].detail;
	assert.match(detail, /container_capability_mismatch/);
	assert.match(detail, /Select a normalized live-policy pool/);
});

test("an unhealthy pool keeps its retry remediation in the transcript", () => {
	const activity = eventsToLocalActivity([
		preflightFailure({
			code: "container_unhealthy",
			container_id: "ctr_ba9ba61b1a694ef8979aac97a2cd8cd5",
			base_url: "http://127.0.0.1:8104",
			last_probe_error: "connection refused",
			retryable: true,
			remediation: "Start or repair the registered pool at this URL, then call container_probe."
		})
	], []);
	const line = Object.values(activity).flat()[0];
	assert.equal(line.toolStatus, "failed");
	assert.match(line.detail, /container_unhealthy/);
	assert.match(line.detail, /container_probe/);
});

test("a successful container tool call still renders as completed", () => {
	const activity = eventsToLocalActivity([
		event({
			payload: {
				item: {
					type: "mcpToolCall",
					id: "probe-1",
					server: "synth_containers",
					tool: "container_probe",
					status: "completed",
					arguments: { container_id: "ctr_1" },
					result: { structuredContent: { container: { id: "ctr_1" } } }
				}
			}
		})
	], []);
	const line = Object.values(activity).flat()[0];
	assert.equal(line.toolStatus, "completed");
	assert.equal(line.detail, "container id ctr_1");
});
