/**
 * Hard invariant: placement "after" activity must not chronologically precede
 * the assistant text it hangs under (the screenshot misorder: tools under the
 * answer that used them).
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

const source = join(appRoot, "src/renderer/src/runtime/activityPlacementInvariant.ts");
const compiled = join(compiledDir, "activityPlacementInvariant.mjs");
writeFileSync(
	compiled,
	transformSync(readFileSync(source, "utf8"), {
		loader: "ts",
		format: "esm",
		target: "es2022",
		sourcefile: source
	}).code
);

const {
	activityLineSequence,
	assertLocalActivityPlacementInvariant
} = await import(pathToFileURL(compiled).href);

test("activityLineSequence reads explicit sequence or id suffix", () => {
	assert.equal(activityLineSequence({ id: "activity-12", label: "x", sequence: 12 }), 12);
	assert.equal(activityLineSequence({ id: "activity-7", label: "x" }), 7);
	assert.equal(activityLineSequence({ id: "context-compaction-3", label: "x" }), 3);
	assert.equal(activityLineSequence({ id: "la1", label: "fixture" }), undefined);
});

test("preamble + later tools (placement after) satisfies the invariant", () => {
	assert.doesNotThrow(() =>
		assertLocalActivityPlacementInvariant(
			[{ id: "msg-preamble", role: "assistant", body: "Looking into it.", at: "t0" }],
			{
				"msg-preamble": [
					{
						id: "activity-20",
						label: "pwd",
						kind: "command",
						placement: "after",
						sequence: 20
					}
				]
			},
			new Map([["msg-preamble", 10]])
		)
	);
});

test("tools under a later-merged final answer violate the invariant loudly", () => {
	assert.throws(
		() =>
			assertLocalActivityPlacementInvariant(
				[
					{
						id: "msg-1",
						role: "assistant",
						body: "Opened the latest visual… Want me to also pop the Luna comparison?",
						at: "t0"
					}
				],
				{
					"msg-1": [
						{
							id: "activity-14",
							label: "synth_visuals.visual_manage",
							kind: "command",
							placement: "after",
							sequence: 14
						},
						{
							id: "activity-18",
							label: "cat gallery",
							kind: "command",
							placement: "after",
							sequence: 18
						}
					]
				},
				// Final answer content landed on the same bubble after the tools.
				new Map([["msg-1", 42]])
			),
		(error) => {
			assert.ok(error instanceof Error);
			assert.match(error.message, /Activity placement invariant violated/);
			assert.match(error.message, /activity-14/);
			assert.match(error.message, /seq 14/);
			assert.match(error.message, /seq 42/);
			assert.match(error.message, /must not render below it/);
			return true;
		}
	);
});

test("before-placement tools are ignored by the invariant", () => {
	assert.doesNotThrow(() =>
		assertLocalActivityPlacementInvariant(
			[{ id: "msg-1", role: "assistant", body: "Done.", at: "t0" }],
			{
				"msg-1": [
					{
						id: "activity-5",
						label: "pwd",
						kind: "command",
						sequence: 5
					}
				]
			},
			new Map([["msg-1", 40]])
		)
	);
});
