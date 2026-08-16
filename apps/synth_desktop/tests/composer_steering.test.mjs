/**
 * Composer steering contract.
 *
 * The reported failure: during an active turn the first Return queued a prompt
 * under **Next turns**, but a second Return from the normal composer left it
 * sitting there — the user had to click the queued row and press Return twice
 * more. Failures also rendered raw values (`[object Object]`, an internal
 * session UUID, internal persistence text) in red above the composer.
 *
 * Two causes, both pinned here: the promotion arm expired on a 2.5s stopwatch
 * tuned to a synthetic double press rather than a human one, and error text was
 * whatever the runtime happened to reject with.
 */
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
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
	IDLE_STEER_STATE,
	STEER_PROMOTION_WINDOW_MS,
	STEER_UNSUPPORTED,
	armedPromptId,
	normalizeSteerFailure,
	promotingPromptId,
	reduceSteer,
	redactIdentifiers,
	steerFailure
} = await import(compile("src/renderer/src/runtime/steering.ts", "ComposerSteering.mjs"));

const SESSION_UUID = "7f3a1c92-1d4b-4e2a-9c7f-0b1d2e3f4a5b";

/** Drive the machine through a list of events, collecting every effect. */
function run(events, initial = IDLE_STEER_STATE) {
	let state = initial;
	const effects = [];
	for (const event of events) {
		const next = reduceSteer(state, event);
		state = next.state;
		if (next.effect) effects.push(next.effect);
	}
	return { state, effects };
}

const queued = (at = 1_000, text = "use the smaller batch") => ({
	type: "queued",
	promptId: "queue-1",
	text,
	at
});

const pressed = (overrides = {}) => ({
	type: "return",
	composerText: "",
	at: 1_100,
	...overrides
});

// 1. First Return queues during an active turn.

test("the first Return arms the prompt it just queued", () => {
	const { state, effects } = run([queued()]);
	assert.equal(state.phase, "armed");
	assert.equal(armedPromptId(state), "queue-1");
	assert.deepEqual(effects, [], "queueing alone never delivers a steer");
});

test("an empty prompt never arms and never steers", () => {
	const { state, effects } = run([queued(1_000, "   "), pressed()]);
	assert.equal(state.phase, "idle");
	assert.deepEqual(effects, []);
});

// 2. Second Return from the main composer promotes the queued prompt.
// 3. Promotion works without clicking the queue row — the machine never sees
//    focus, so the same events promote from the composer or from the row.

test("a second Return promotes the newest queued prompt", () => {
	const { state, effects } = run([queued(), pressed()]);
	assert.deepEqual(effects, [
		{ kind: "promote", promptId: "queue-1", text: "use the smaller batch" }
	]);
	assert.equal(promotingPromptId(state), "queue-1");
});

test("promotion survives the delay a human needs to read Next turns", () => {
	// The old 2.5s stopwatch expired here, which is exactly why the gesture
	// only worked after clicking into the queued row.
	const { effects } = run([queued(0), pressed({ at: 8_000 })]);
	assert.equal(effects.length, 1);
	assert.ok(STEER_PROMOTION_WINDOW_MS >= 8_000);
});

test("a forgotten arm still lapses instead of steering much later", () => {
	const { state, effects } = run([
		queued(0),
		pressed({ at: STEER_PROMOTION_WINDOW_MS + 1 })
	]);
	assert.deepEqual(effects, []);
	assert.equal(state.phase, "idle");
});

test("the pre-commit composer value promotes, newly typed text does not", () => {
	// React may not have cleared the textarea before a fast physical double
	// Return; the armed text is acceptable, anything else is a new prompt.
	assert.equal(run([queued(), pressed({ composerText: "use the smaller batch" })]).effects.length, 1);
	const typedOver = run([queued(), pressed({ composerText: "something else entirely" })]);
	assert.deepEqual(typedOver.effects, []);
	assert.equal(typedOver.state.phase, "armed", "the arm survives so the new text can enqueue");
});

// 4. Held Return / key repeat delivers once.

test("held Return delivers exactly one steer", () => {
	const { effects } = run([
		queued(),
		pressed({ repeat: false }),
		pressed({ at: 1_150, repeat: true }),
		pressed({ at: 1_200, repeat: true }),
		pressed({ at: 1_250, repeat: true })
	]);
	assert.deepEqual(effects, [
		{ kind: "promote", promptId: "queue-1", text: "use the smaller batch" }
	]);
});

test("a second real press while one promotion is in flight is the same intent", () => {
	const { effects } = run([queued(), pressed(), pressed({ at: 1_400 })]);
	assert.equal(effects.length, 1);
});

// 5. Shift+Return and IME composition do not promote.

test("an IME composition commit never promotes", () => {
	const { state, effects } = run([queued(), pressed({ composing: true })]);
	assert.deepEqual(effects, []);
	assert.equal(state.phase, "armed", "the composition press changed nothing");
});

// 6. Backend failure leaves the prompt recoverable and shows a public error.

test("a rejection keeps the prompt and reports a normalized public error", () => {
	const failure = normalizeSteerFailure(
		new Error(`session ${SESSION_UUID} has no active turn to steer`)
	);
	const { state } = run([
		queued(),
		pressed(),
		{ type: "rejected", promptId: "queue-1", failure }
	]);
	assert.equal(state.phase, "failed");
	assert.equal(steerFailure(state).code, "steer_turn_finished");
	assert.equal(promotingPromptId(state), null);
	// The prompt was never acknowledged, so it is still in Next turns.
	const reconciled = reduceSteer(state, { type: "queueReconciled", promptIds: ["queue-1"] });
	assert.equal(reconciled.state.phase, "failed");
});

test("a prompt is retired only by the acknowledgement that names it", () => {
	const { state } = run([queued(), pressed()]);
	const other = reduceSteer(state, { type: "acknowledged", promptId: "queue-2" });
	assert.equal(promotingPromptId(other.state), "queue-1", "someone else's ack retires nothing");
	const own = reduceSteer(state, { type: "acknowledged", promptId: "queue-1" });
	assert.equal(own.state.phase, "idle");
});

// 7. A structured object error never renders `[object Object]`.

test("no rejection shape can render [object Object]", () => {
	const shapes = [
		{ code: "internal", message: "boom", detail: { nested: true } },
		{ error: { code: "conflict", message: "database is locked" } },
		{ unexpected: { deeply: { nested: 1 } } },
		[1, 2, 3],
		null,
		undefined,
		42,
		"plain string rejection",
		new Error("")
	];
	for (const shape of shapes) {
		const failure = normalizeSteerFailure(shape);
		assert.equal(typeof failure.message, "string", `shape=${JSON.stringify(shape)}`);
		assert.ok(failure.message.trim().length > 0);
		assert.ok(
			!failure.message.includes("[object Object]"),
			`leaked object rendering for ${JSON.stringify(shape)}`
		);
		assert.equal(typeof failure.code, "string");
		assert.equal(typeof failure.detail, "string");
	}
});

// 8. Internal session UUIDs never appear in the core composer.

test("internal identifiers stay in diagnostics, never in the message", () => {
	const failure = normalizeSteerFailure(
		new Error(`session ${SESSION_UUID} has no active turn to steer`)
	);
	assert.ok(!failure.message.includes(SESSION_UUID));
	assert.ok(failure.detail.includes(SESSION_UUID), "diagnostics keep the full original");

	// Even an unrecognized runtime message is redacted on the way out.
	const opaque = normalizeSteerFailure({
		code: "internal",
		message: `persist failed for session ${SESSION_UUID} at row 0123456789abcdef01`
	});
	assert.ok(!opaque.message.includes(SESSION_UUID));
	assert.ok(!opaque.message.includes("0123456789abcdef01"));
	assert.equal(redactIdentifiers(SESSION_UUID), "…");
});

test("known failures carry a stable code and an actionable sentence", () => {
	const cases = [
		[new Error("session x has no active turn to steer"), "steer_turn_finished"],
		[new Error("steer text must not be empty"), "steer_empty"],
		[{ code: "unauthorized", message: "not signed in" }, "steer_unauthorized"],
		[{}, "steer_unavailable"],
		[STEER_UNSUPPORTED, "steer_unsupported"]
	];
	for (const [reason, code] of cases) {
		const failure = normalizeSteerFailure(reason);
		assert.equal(failure.code, code);
		assert.ok(/\.$/.test(failure.message.trim()), `no closing guidance for ${code}`);
	}
	// An already-normalized failure is passed through, not re-wrapped.
	assert.equal(normalizeSteerFailure(STEER_UNSUPPORTED), STEER_UNSUPPORTED);
	assert.match(STEER_UNSUPPORTED.message, /not supported/);
});

// 9. Disconnect / reconnect preserves or safely retries one queued steer
//    without duplication.

test("a reconnect that drops the prompt disarms instead of steering a ghost", () => {
	const { state } = run([queued()]);
	const reconnected = reduceSteer(state, { type: "queueReconciled", promptIds: ["queue-9"] });
	assert.equal(reconnected.state.phase, "idle");
	assert.deepEqual(reduceSteer(reconnected.state, pressed({ at: 2_000 })).effect, null);
});

test("a reconnect during promotion keeps waiting for the acknowledgement", () => {
	const { state } = run([queued(), pressed()]);
	const reconnected = reduceSteer(state, { type: "queueReconciled", promptIds: [] });
	assert.equal(promotingPromptId(reconnected.state), "queue-1");
	// And it still cannot be delivered a second time.
	assert.equal(reduceSteer(reconnected.state, pressed({ at: 2_000 })).effect, null);
});

test("the turn ending falls back to the next-turn path without losing the prompt", () => {
	const ended = reduceSteer(run([queued()]).state, { type: "turnEnded" });
	assert.equal(ended.state.phase, "idle");
	assert.equal(ended.effect, null);
	// A promotion already in flight is not abandoned by a turn-end race.
	const inFlight = reduceSteer(run([queued(), pressed()]).state, { type: "turnEnded" });
	assert.equal(promotingPromptId(inFlight.state), "queue-1");
});
