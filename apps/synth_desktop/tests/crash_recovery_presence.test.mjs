/**
 * The renderer half of the stale-Working fix.
 *
 * The invariant under test: a chat may render Working only when a live turn
 * owned by the current Workshop instance exists. Persisted `status: "running"`
 * — which is all a crash leaves behind — must render as Recovering, keep its
 * controls usable, and offer a restart that starts a new attempt.
 */
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { tmpdir } from "node:os";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(tmpdir(), "synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

function compile(relative, outName) {
	const source = join(appRoot, relative);
	const compiled = join(compiledDir, outName);
	writeFileSync(
		compiled,
		transformSync(readFileSync(source, "utf8"), {
			loader: "ts",
			format: "esm",
			target: "es2022",
			sourcefile: source
		}).code
	);
	return pathToFileURL(compiled).href;
}

const {
	applyLocalSessionStatus,
	applyRuntimeEvent,
	applyTurnAccepted,
	pruneLiveTurns,
	selectChatBusy,
	selectChatPresence,
	selectWorkingChatIds,
	sessionRecoveryNotice
} = await import(compile("src/renderer/src/stores/applyRuntimeEvent.ts", "applyRuntimeEventRecovery.mjs"));

compile("src/renderer/src/runtime/lagunaPolicies.ts", "lagunaPolicies.ts");
const { restoreCodexSession } = await import(
	compile("src/renderer/src/runtime/nativeCodex.ts", "nativeCodexRecovery.mjs")
);

function notice(overrides = {}) {
	return {
		sessionId: "sess-1",
		runId: "turn-1",
		reason: "workshop_restarted",
		previousOwnerInstanceId: "inst_dead",
		lastHeartbeatAt: "2026-08-16T21:34:01.000Z",
		recoveryAttempt: 1,
		restartable: true,
		needsAttention: false,
		lastActivity: { kind: "item/completed", label: "container_list", at: "2026-08-16T21:34:01.000Z" },
		lastUserMessage: { text: "Run the Craftax eval for seed 201", clientMessageId: "user-1" },
		recoveredAt: "2026-08-16T21:40:00.000Z",
		...overrides
	};
}

function session(overrides = {}) {
	return {
		id: "sess-1",
		title: "Craftax seed 201",
		target: { kind: "remote", provider: "openrouter", model: "gpt-5.6-luna" },
		createdAt: "2026-08-16T21:00:00.000Z",
		updatedAt: "2026-08-16T21:40:00.000Z",
		status: "ready",
		latestCursor: 0,
		metadata: {},
		...overrides
	};
}

function state(sessions, liveTurns = {}) {
	return { sessions, eventsBySession: {}, liveTurns };
}

function event(overrides = {}) {
	return {
		schemaVersion: "synth.desktop-runtime-event.v1",
		sessionId: "sess-1",
		sequence: 1,
		eventKind: "run.started",
		payload: {},
		createdAt: "2026-08-16T21:00:01.000Z",
		source: "local",
		...overrides
	};
}

test("persisted running without a live owner never renders Working", () => {
	const stale = session({ status: "running", metadata: { recovery: notice() } });
	assert.deepEqual([...selectWorkingChatIds([stale], {})], []);
	assert.equal(selectChatPresence(stale, {}), "recovering");
});

test("Working requires both a running status and a live turn", () => {
	const running = session({ status: "running" });
	assert.deepEqual([...selectWorkingChatIds([running], { "sess-1": "turn-9" })], ["sess-1"]);
	assert.equal(selectChatPresence(running, { "sess-1": "turn-9" }), "working");
	// The same live claim on a chat that is not running proves nothing.
	assert.deepEqual([...selectWorkingChatIds([session({ status: "ready" })], { "sess-1": "turn-9" })], []);
});

test("presence separates recovering, interrupted and needs attention", () => {
	assert.equal(
		selectChatPresence(session({ status: "running", metadata: { recovery: notice() } }), {}),
		"recovering"
	);
	assert.equal(
		selectChatPresence(session({ status: "interrupted", metadata: { recovery: notice() } }), {}),
		"interrupted"
	);
	assert.equal(
		selectChatPresence(
			session({ status: "interrupted", metadata: { recovery: notice({ needsAttention: true, restartable: false }) } }),
			{}
		),
		"needsAttention"
	);
	// An unknown external settlement outranks even an apparently live turn:
	// acting on it could duplicate paid work.
	assert.equal(
		selectChatPresence(
			session({ status: "running", metadata: { recovery: notice({ needsAttention: true, restartable: false }) } }),
			{ "sess-1": "turn-9" }
		),
		"needsAttention"
	);
	assert.equal(selectChatPresence(session({ status: "ready" }), {}), "idle");
	assert.equal(selectChatPresence(null, {}), "idle");
});

test("only a genuinely working chat locks its controls", () => {
	assert.equal(selectChatBusy("working"), true);
	assert.equal(selectChatBusy("starting"), true);
	// Archive must not stay disabled forever on an ownerless turn.
	assert.equal(selectChatBusy("recovering"), false);
	assert.equal(selectChatBusy("interrupted"), false);
	assert.equal(selectChatBusy("needsAttention"), false);
	assert.equal(selectChatBusy("idle"), false);
});

test("hydrating sessions never grants liveness", () => {
	// pruneLiveTurns is what replaceSessions applies at boot and refresh.
	assert.deepEqual(pruneLiveTurns({}, [session({ status: "running" })]), {});
	// A claim for a chat that no longer exists is dropped, not carried.
	assert.deepEqual(pruneLiveTurns({ "sess-gone": "turn-1" }, [session()]), {});
	assert.deepEqual(pruneLiveTurns({ "sess-1": "turn-1" }, [session()]), { "sess-1": "turn-1" });
});

test("a restored Codex record is never running, and carries its notice", () => {
	const restored = restoreCodexSession({
		sessionId: "sess-1",
		threadId: "thread-201",
		workspace: "/workspace",
		model: "gpt-5.6-luna",
		providerName: "openrouter",
		providerTitle: "OpenRouter Responses",
		baseUrl: "https://openrouter.ai/api/v1",
		status: "running",
		title: "Craftax seed 201",
		approvalPolicy: "never",
		sandbox: "workspace-write",
		recovery: notice()
	});
	assert.equal(restored.status, "interrupted");
	assert.equal(sessionRecoveryNotice(restored)?.reason, "workshop_restarted");
	// Its execution target survives the crash: no silent fallback to Laguna.
	assert.deepEqual(restored.target, {
		kind: "remote",
		provider: "openrouter",
		model: "gpt-5.6-luna",
		adapter: null
	});
});

test("a live turn grants ownership and every terminal revokes it", () => {
	const target = { kind: "remote", provider: "openrouter", model: "gpt-5.6-luna" };
	let next = applyTurnAccepted(state([session()]), "sess-1", { target, turnId: "turn-1" });
	assert.equal(next.liveTurns["sess-1"], "turn-1");
	assert.equal(selectChatPresence(next.sessions[0], next.liveTurns), "working");

	for (const eventKind of ["run.completed", "run.failed", "run.cancelled", "session/unhealthy"]) {
		const ended = applyRuntimeEvent(next, event({ sequence: 2, eventKind }));
		assert.equal(ended.liveTurns["sess-1"], undefined, eventKind);
	}

	const interrupted = applyLocalSessionStatus(next, "sess-1", "interrupted", { onlyIf: "running" });
	assert.equal(interrupted.liveTurns["sess-1"], undefined);
});

test("a fenced run.started grants nothing", () => {
	const stale = state([session({ status: "interrupted", metadata: { recovery: notice() } })]);
	const echoed = applyRuntimeEvent(stale, event({ sequence: 7, eventKind: "run.started" }), {
		fenced: true
	});
	assert.equal(echoed.liveTurns["sess-1"], undefined);
	assert.equal(selectChatPresence(echoed.sessions[0], echoed.liveTurns), "interrupted");
});

test("a durable recovery event revokes a claim the renderer still held", () => {
	const target = { kind: "remote", provider: "openrouter", model: "gpt-5.6-luna" };
	const live = applyTurnAccepted(state([session()]), "sess-1", { target, turnId: "turn-1" });
	const recovered = applyRuntimeEvent(
		live,
		event({ sequence: 3, eventKind: "session/recovery_required", payload: notice() })
	);
	assert.equal(recovered.liveTurns["sess-1"], undefined);
});

test("restarting clears the notice and starts a new attempt", () => {
	const target = { kind: "remote", provider: "openrouter", model: "gpt-5.6-luna" };
	const crashed = state([session({ status: "interrupted", metadata: { recovery: notice() } })]);
	assert.equal(sessionRecoveryNotice(crashed.sessions[0])?.recoveryAttempt, 1);

	const restarted = applyTurnAccepted(crashed, "sess-1", { target, turnId: "turn-2" });

	assert.equal(restarted.sessions[0].status, "running");
	assert.equal(restarted.liveTurns["sess-1"], "turn-2");
	assert.equal(sessionRecoveryNotice(restarted.sessions[0]), null);
	assert.equal(selectChatPresence(restarted.sessions[0], restarted.liveTurns), "working");
});

test("five chats abandoned by a crash all present as recovering and stay archivable", () => {
	const sessions = [201, 202, 203, 204, 205].map((seed) =>
		session({
			id: `sess-${seed}`,
			title: `Craftax seed ${seed}`,
			status: "running",
			metadata: { recovery: notice({ sessionId: `sess-${seed}`, runId: `turn-${seed}` }) }
		})
	);

	assert.deepEqual([...selectWorkingChatIds(sessions, {})], []);
	for (const candidate of sessions) {
		assert.equal(selectChatPresence(candidate, {}), "recovering", candidate.id);
		assert.equal(selectChatBusy(selectChatPresence(candidate, {})), false, candidate.id);
	}
});

test("a malformed recovery bag is ignored rather than half-rendered", () => {
	assert.equal(sessionRecoveryNotice(session({ metadata: { recovery: "yes" } })), null);
	assert.equal(sessionRecoveryNotice(session({ metadata: { recovery: { reason: "workshop_restarted" } } })), null);
	assert.equal(sessionRecoveryNotice(session()), null);
	assert.equal(sessionRecoveryNotice(null), null);
});
