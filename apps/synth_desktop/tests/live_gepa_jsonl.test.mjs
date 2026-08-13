import assert from "node:assert/strict";
import test from "node:test";

import { parseLiveGepaJsonl } from "../liveGepaJsonl.ts";

test("defers only an incomplete unterminated final JSONL record", () => {
	assert.deepEqual(
		parseLiveGepaJsonl('{"sequence_number":1}\n{"sequence_number":'),
		[{ sequence_number: 1 }],
	);
});

test("keeps a valid unterminated final JSONL record", () => {
	assert.deepEqual(
		parseLiveGepaJsonl('{"sequence_number":1}\n{"sequence_number":2}'),
		[{ sequence_number: 1 }, { sequence_number: 2 }],
	);
});

test("rejects malformed complete JSONL records, including the final line", () => {
	assert.throws(
		() => parseLiveGepaJsonl('{"sequence_number":1}\nnot-json\n{"sequence_number":3}\n'),
		/invalid GEPA JSONL at line 2/,
	);
	assert.throws(
		() => parseLiveGepaJsonl('{"sequence_number":1}\nnot-json\n'),
		/invalid GEPA JSONL at line 2/,
	);
});
