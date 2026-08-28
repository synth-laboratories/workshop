import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const renderer = join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src");
const read = (rel) => readFileSync(join(renderer, rel), "utf8");
const tauri = join(dirname(fileURLToPath(import.meta.url)), "../src-tauri/src");
const readTauri = (rel) => readFileSync(join(tauri, rel), "utf8");

test("v0.2 pending approval cards pin above Working in ChatTranscript", () => {
	const source = read("components/ChatTranscript.tsx");
	const approvals = source.indexOf("{inlineApprovals.map((line) => renderActivityLine(line, [], false, false))}");
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

test("v0.4 transcript shows immutable generation speed and final elapsed work", () => {
	const source = read("components/ChatTranscript.tsx");
	const turnLabels = read("hooks/useTurnPerformanceLabels.ts");
	const labels = read("runtime/modelPerformanceLabels.ts");
	assert.match(source, /data-testid="model-working-generation-tps"/);
	assert.match(source, /data-testid={`assistant-generation-tps-\${m\.id}`}/);
	assert.match(source, /useTurnPerformanceLabels\(\s*chat,\s*events,\s*running,/);
	assert.doesNotMatch(source, /medianTpsLabel\?\.replace/);
	assert.match(turnLabels, /Generation speed unavailable/);
	assert.match(turnLabels, /event\.eventKind === "turn\/accepted"/);
	// The turn-wide estimate is gone. The renderer computes no rate: it shows
	// backend per-segment measurements, so there is no gap threshold, no
	// tokens-over-duration arithmetic, and nothing called a median.
	assert.doesNotMatch(turnLabels, /MAX_GENERATION_DELTA_GAP_MS/);
	assert.doesNotMatch(turnLabels, /generationActiveMs/);
	assert.doesNotMatch(turnLabels, /median/i);
	assert.doesNotMatch(source, /median/i);
	assert.match(turnLabels, /turn\/generationSpeed/);
	assert.match(source, /Elapsed work time/);
	assert.match(labels, /summary\.provider === "openai-codex-oauth"/);
	assert.match(labels, /CHATGPT_LUNA_MODEL\) return "chatgpt-luna"/);
	assert.match(labels, /CHATGPT_SOL_MODEL\) return "chatgpt-sol"/);
	assert.match(labels, /CHATGPT_TERRA_MODEL\) return "chatgpt-terra"/);
});

test("running tool calls show a compact progress icon before their tool icon", () => {
	const transcript = read("components/ChatTranscript.tsx");
	const css = read("styles/app.css");
	assert.match(transcript, /line\.toolStatus === "running"[\s\S]*className="tool-running-indicator"/);
	assert.match(transcript, /\{runningIndicator\}[\s\S]*className="tool-activity-icon"/);
	assert.match(css, /\.tool-running-indicator[\s\S]*animation: tool-running-spin/);
	assert.match(css, /prefers-reduced-motion: reduce[\s\S]*\.tool-running-indicator/);
});

test("finished tool calls show duration only after fifteen seconds", () => {
	const transcript = read("components/ChatTranscript.tsx");
	const projection = read("runtime/sessionView.ts");
	assert.match(transcript, /durationMs == null \|\| durationMs <= 15_000/);
	assert.match(transcript, /className="tool-duration"/);
	assert.match(projection, /durationMs: safeTool\.durationMs/);
});

test("paid-compute approval is a cap-scoped modal, not a transcript card", () => {
	const transcript = read("components/ChatTranscript.tsx");
	assert.match(transcript, /data-testid="paid-compute-approval-modal"/);
	assert.match(transcript, /data-testid="paid-compute-approve"/);
	assert.match(transcript, /data-testid="paid-compute-reject"/);
	assert.match(transcript, /data-approval-digest=\{approvalDigest\}/);
	assert.match(transcript, /data-expires-at=\{payload\?\.expiresAt\}/);
	assert.match(transcript, /role="dialog" aria-modal="true"/);
	assert.match(transcript, />Approve</);
	assert.match(transcript, /Predicted spend/);
	assert.match(transcript, /requestingAgent/);
	assert.match(transcript, /line\.approvalKind !== "paid_compute"/);
	assert.match(transcript, /Escape/);
	assert.match(transcript, /pendingApprovals\.find\(\(line\) => line\.approvalKind === "paid_compute"\)/);
});

test("paid-compute auto-approval settings live on the Workshop permission surface", () => {
	const settings = read("components/PaidComputePermissionSettings.tsx");
	const general = read("components/GeneralPreferencesSettings.tsx");
	assert.match(general, /PaidComputePermissionSettings/);
	assert.match(settings, /data-testid="paid-compute-auto-approve"/);
	assert.match(settings, /data-testid="paid-compute-max-request"/);
	assert.match(settings, /data-testid="paid-compute-max-conversation"/);
	assert.match(settings, /Limits apply to each request’s hard ceiling/);
	assert.match(settings, /parseUsdAmount/);
});

test("optimizer MCP recipe starts cannot bypass the typed approval broker", () => {
	const ipc = readTauri("visuals_ipc.rs");
	const commands = readTauri("lib.rs");
	const bridge = read("runtime/desktopBridge.ts");
	assert.match(ipc, /authorize_optimizer_recipe_start\(app, core, &codex, request\)/);
	assert.doesNotMatch(ipc, /optimizers\.start_recipe\(request\)/);
	assert.match(commands, /async fn authorize_optimizer_recipe_start/);
	assert.match(commands, /ApprovalKind::PaidCompute/);
	assert.match(commands, /ApprovalKind::CredentialAccess/);
	assert.match(commands, /ApprovalKind::SidecarLifecycle/);
	// The invariant is that approval events reach the transcript even though
	// they are journaled as system rather than codex events. Assert both the
	// boundary definition and its use so a renamed helper cannot become a
	// decorative, disconnected check.
	assert.match(bridge, /const isApprovalBoundary = event\.kind\.startsWith\("approval\."\)/);
	assert.match(bridge, /event\.source !== "codex" && !isApprovalBoundary/);
});

test("hosted SFT uses only the public synth-optimizers control plane", () => {
	const hostedSft = readTauri("optimizers/hosted_sft.rs");
	const sftClient = readTauri("optimizers/sft_client.rs");
	const privateGeloClient = readTauri("optimizers/hosted_client.rs");
	const service = readTauri("optimizers/service.rs");
	const commands = readTauri("lib.rs");
	assert.match(hostedSft, /SftOptimizerClient/);
	assert.doesNotMatch(hostedSft, /HostedOptimizerClient/);
	assert.doesNotMatch(hostedSft, /OPTIMIZERS_BETA|SYNTH_OPTIMIZERS_BETA/);
	assert.match(sftClient, /Workshop never contacts Optimizers-beta directly/);
	assert.doesNotMatch(privateGeloClient, /\bsubmit_toml\b|fn optimizer_events_after/);
	// Cancel must reach the canonical public SFT run through the sidecar proxy,
	// which in turn holds the only SftOptimizerClient. Bound both checks to the
	// function bodies so a stray mention elsewhere cannot satisfy them, and keep
	// refusing any path that dials :8787 / Optimizers-beta directly.
	const sidecarTraining = readTauri("optimizers/sidecar_training.rs");
	// Cancellation now also receives the typed, durable cancellation request;
	// locate the body by function boundary rather than pinning its full signature.
	const cancelStart = service.search(/pub async fn cancel\s*\(/);
	const pauseOffset = service.slice(cancelStart).search(/pub async fn pause\s*\(\s*&self,\s*id:\s*String\s*\)/);
	const cancelEnd = pauseOffset === -1 ? -1 : cancelStart + pauseOffset;
	assert.ok(cancelStart !== -1 && cancelEnd > cancelStart, "OptimizerService::cancel body not found");
	const cancelBody = service.slice(cancelStart, cancelEnd);
	assert.match(
		cancelBody,
		/matches!\(run\.algorithm_id\.as_str\(\), "sft" \| "cispo"\)[\s\S]*?SidecarTrainingClient::from_manager\(self\.manager\(\)\)[\s\S]*?client\.cancel\(&id\)\.await/,
	);
	assert.doesNotMatch(cancelBody, /8787|MlxLoopback|HostedOptimizerClient|reqwest|SftOptimizerClient/);
	const hostedStart = sidecarTraining.indexOf("async fn drive_hosted_sft_job(");
	assert.notEqual(hostedStart, -1, "drive_hosted_sft_job not found");
	const hostedEnd = sidecarTraining.indexOf("\nasync fn ", hostedStart + 1);
	const hostedSftBody = sidecarTraining.slice(hostedStart, hostedEnd === -1 ? undefined : hostedEnd);
	assert.match(
		hostedSftBody,
		/let client = SftOptimizerClient::from_env\(\)\?;[\s\S]*?job\.cancelled[\s\S]*?client\s*\.cancel\(job_id\)\s*\.await/,
	);
	assert.doesNotMatch(hostedSftBody, /HostedOptimizerClient|MlxLoopback/);
	assert.doesNotMatch(sidecarTraining, /8787|OPTIMIZERS_BETA|SYNTH_OPTIMIZERS_BETA/);
	assert.match(hostedSft, /kind:\s*"optimizer_sidecar"\.into\(\)/);
	assert.match(service, /fn primary_visual_template[\s\S]*"sft" \| "cispo" => "optimizer\.sft\.live\.v1"/);
	assert.match(commands, /"sft\.craftax\.nemotron-nano\.tinker\.v1"[\s\S]*SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN/);
	assert.match(
		commands,
		/matches!\([\s\S]*request\.recipe_id\.as_str\(\)[\s\S]*"cispo\.mlx\.v1"[\s\S]*start_recipe\(request\)[\s\S]*return Ok\(run\)/,
	);
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
