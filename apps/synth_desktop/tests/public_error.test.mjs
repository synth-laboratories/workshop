// A rejected Tauri command is a structured object, not an Error. Coercing one
// with String() rendered a literal `[object Object]` above the container attach
// form. These pin the projection every user-facing surface now goes through.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { transformSync } from "esbuild";

const source = resolve(import.meta.dirname, "../src/renderer/src/runtime/publicError.ts");
const compiled = resolve(import.meta.dirname, "../test-results/compiled/publicError.mjs");
mkdirSync(dirname(compiled), { recursive: true });
writeFileSync(compiled, transformSync(readFileSync(source, "utf8"), {
	loader: "ts",
	format: "esm",
	target: "es2022"
}).code);
const { publicError, toPublicError } = await import(compiled);

test("an AppError renders its message, never [object Object]", () => {
	const rendered = publicError({ code: "invalid_argument", message: "Base URL must be absolute.", detail: "anyhow chain" });
	assert.equal(rendered.includes("[object Object]"), false);
	assert.match(rendered, /Base URL must be absolute\./);
	assert.match(rendered, /invalid_argument/);
});

test("developer detail alone never becomes the user-facing message", () => {
	const rendered = publicError({ code: "internal", detail: "thread 'main' panicked at src/lib.rs:42" });
	assert.equal(rendered.includes("panicked"), false);
	assert.match(rendered, /internal/);
});

test("a capability refusal keeps its code, remediation and retryability", () => {
	const projected = toPublicError({
		code: "container_capability_rejected",
		message: "The container advertises no live-eval operations.",
		remediation: "Probe the container, then attach a normalized pool.",
		retryable: false
	});
	assert.equal(projected.code, "container_capability_rejected");
	assert.equal(projected.retryable, false);
	assert.match(projected.remediation, /normalized pool/);
});

test("a typed Shoal cold-start response becomes an actionable warming state", () => {
	const projected = toPublicError({
		error: {
			code: "inference_target_not_ready",
			message: "cold target missed its deadline",
			retryable: true,
			warm_operation_id: "op-redacted"
		}
	});
	assert.equal(projected.message, "The hosted model is warming up.");
	assert.equal(projected.retryable, true);
	assert.match(projected.remediation, /Retry in a moment/);
});

test("a Shoal capacity response distinguishes cloud scheduling from model warmup", () => {
	const projected = toPublicError({
		detail: {
			error: {
				code: "inference_provider_capacity_pending",
				message: "provider pending",
				retryable: true,
				state: "provider_start_pending",
				source: "cloud",
				warm_operation_id: "warm-123",
				elapsed_ms: 30125
			}
		}
	});
	assert.equal(projected.message, "Waiting for Synth Cloud GPU capacity.");
	assert.equal(projected.state, "provider_start_pending");
	assert.equal(projected.source, "cloud");
	assert.equal(projected.warmOperationId, "warm-123");
	assert.equal(projected.elapsedMs, 30125);
	assert.match(projected.remediation, /same warm operation continues/);
});

test("secrets in boundary text are redacted", () => {
	const rendered = publicError({ message: "upstream rejected sk-abcdef0123456789 for this call" });
	assert.equal(rendered.includes("sk-abcdef0123456789"), false);
	assert.match(rendered, /\[redacted\]/);
});

test("Error instances and plain strings still project unchanged", () => {
	assert.equal(publicError(new Error("boom")), "boom");
	assert.equal(publicError("plain failure"), "plain failure");
});

test("an unreadable rejection falls back to the caller's sentence", () => {
	assert.equal(publicError(undefined, "Attaching the container failed."), "Attaching the container failed.");
	assert.equal(publicError({}, "Attaching the container failed."), "Attaching the container failed.");
});

test("long boundary text is clamped so a surface cannot be flooded", () => {
	const rendered = publicError({ message: "x".repeat(5_000) });
	assert.ok(rendered.length <= 321, `clamped to ${rendered.length}`);
});
