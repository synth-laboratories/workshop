/**
 * P0-1 lock: generated/protocol.ts is the only desktop command/type authority.
 *
 * - desktopBridge.ts must not call invokeCommand<…>
 * - bridge/types.ts must not locally declare a type/interface whose name also
 *   exists in generated/protocol.ts (re-exports from generated are allowed)
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const renderer = join(appRoot, "src/renderer/src");

function read(rel) {
	return readFileSync(join(renderer, rel), "utf8");
}

function declaredTypeNames(source) {
	const names = new Set();
	for (const match of source.matchAll(/^export (?:type|interface) (\w+)/gm)) {
		names.add(match[1]);
	}
	return names;
}

test("desktopBridge has zero raw invokeCommand< calls", () => {
	const bridge = read("runtime/desktopBridge.ts");
	const matches = bridge.match(/invokeCommand</g) ?? [];
	assert.equal(matches.length, 0, `expected 0 invokeCommand<; found ${matches.length}`);
});

test("bridge/types.ts does not locally declare generated protocol names", () => {
	const types = read("bridge/types.ts");
	const protocol = read("generated/protocol.ts");
	const local = declaredTypeNames(types);
	const generated = declaredTypeNames(protocol);
	const overlap = [...local].filter((name) => generated.has(name)).sort();
	assert.deepEqual(
		overlap,
		[],
		`duplicate type names in bridge/types.ts and generated/protocol.ts: ${overlap.join(", ")}`
	);
});
