import assert from "node:assert/strict";
import test from "node:test";
import { compactModelLabel } from "../src/renderer/src/runtime/modelPresentation.ts";

test("compressed model labels retain recognizable short slugs", () => {
	assert.equal(compactModelLabel("GPT-5.6 Luna"), "Luna");
	assert.equal(compactModelLabel("GPT-5.6 Sol"), "Sol");
	assert.equal(compactModelLabel("poolside/laguna-xs-2.1"), "XS 2.1");
	assert.equal(compactModelLabel("Laguna XS 2.1"), "XS 2.1");
});
