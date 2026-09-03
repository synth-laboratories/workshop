import assert from "node:assert/strict";
import test from "node:test";

import {
	EMPTY_VISUAL_REVISION_STATE,
	bindingAuthorityKey,
	newestVisualArtifact,
	visualRevisionRefreshRequired,
	visualRevisionReducer
} from "../src/renderer/src/runtime/visualRevisionState.ts";

const artifact = (id, revision, bindings = {}) => ({
	id,
	visualId: id,
	kind: "report",
	title: `${id} r${revision}`,
	templateId: "live.test.v1",
	revision,
	bindings
});

function reduce(state, ...actions) {
	return actions.reduce(visualRevisionReducer, state);
}

test("revision acceptance is monotonic when an older request resolves last", () => {
	const state = reduce(
		EMPTY_VISUAL_REVISION_STATE,
		{ type: "select", id: "vis_1", artifact: artifact("vis_1", 13) },
		{ type: "request", id: "vis_1", minimumRevision: 13, generation: 2 },
		{ type: "request", id: "vis_1", minimumRevision: 14, generation: 3 },
		{ type: "resolve", id: "vis_1", artifact: artifact("vis_1", 14), generation: 3 },
		{ type: "resolve", id: "vis_1", artifact: artifact("vis_1", 13), generation: 2 }
	);
	assert.equal(state.acceptedRevision, 14);
	assert.equal(state.artifact?.revision, 14);
});

test("a stale response is rejected in either completion order", () => {
	const state = reduce(
		EMPTY_VISUAL_REVISION_STATE,
		{ type: "select", id: "vis_1", artifact: artifact("vis_1", 13) },
		{ type: "request", id: "vis_1", minimumRevision: 13, generation: 2 },
		{ type: "request", id: "vis_1", minimumRevision: 14, generation: 3 },
		{ type: "resolve", id: "vis_1", artifact: artifact("vis_1", 13), generation: 2 },
		{ type: "resolve", id: "vis_1", artifact: artifact("vis_1", 14), generation: 3 }
	);
	assert.equal(state.acceptedRevision, 14);
});

test("chat and standalone records share one newest-revision selection", () => {
	assert.equal(
		newestVisualArtifact("vis_1", artifact("vis_1", 13), artifact("vis_1", 14))?.revision,
		14
	);
	assert.equal(newestVisualArtifact("vis_1", artifact("vis_other", 99), artifact("vis_1", 14))?.revision, 14);
});

test("a chat Outputs snapshot forces one registry refresh after restart", () => {
	assert.equal(visualRevisionRefreshRequired({
		acceptedRevision: 1,
		minimumRevision: 1,
		open: false,
		wasOpen: true,
		authoritativeRefresh: true
	}), true);
	assert.equal(visualRevisionRefreshRequired({
		acceptedRevision: 14,
		minimumRevision: 14,
		open: false,
		wasOpen: true
	}), false);
});

test("switching IDs invalidates old work and unrelated IDs cannot commit", () => {
	const state = reduce(
		EMPTY_VISUAL_REVISION_STATE,
		{ type: "select", id: "vis_1", artifact: artifact("vis_1", 13) },
		{ type: "request", id: "vis_1", minimumRevision: 14, generation: 2 },
		{ type: "select", id: "vis_2", artifact: artifact("vis_2", 4) },
		{ type: "resolve", id: "vis_1", artifact: artifact("vis_1", 14), generation: 2 },
		{ type: "accept", id: "vis_other", artifact: artifact("vis_other", 100) }
	);
	assert.equal(state.id, "vis_2");
	assert.equal(state.artifact?.revision, 4);
});

test("closing invalidates async work and an update cannot resurrect the pane", () => {
	const state = reduce(
		EMPTY_VISUAL_REVISION_STATE,
		{ type: "select", id: "vis_1", artifact: artifact("vis_1", 13) },
		{ type: "request", id: "vis_1", minimumRevision: 14, generation: 2 },
		{ type: "close" },
		{ type: "resolve", id: "vis_1", artifact: artifact("vis_1", 14), generation: 2 },
		{ type: "accept", id: "vis_1", artifact: artifact("vis_1", 14) }
	);
	assert.equal(state.id, null);
	assert.equal(state.artifact, null);
});

test("refresh failure preserves the last valid revision and records the error", () => {
	const state = reduce(
		EMPTY_VISUAL_REVISION_STATE,
		{ type: "select", id: "vis_1", artifact: artifact("vis_1", 13) },
		{ type: "request", id: "vis_1", minimumRevision: 14, generation: 2 },
		{ type: "fail", id: "vis_1", generation: 2, error: "offline" }
	);
	assert.equal(state.artifact?.revision, 13);
	assert.equal(state.loading, false);
	assert.equal(state.error, "offline");
});

test("transport identity resets for binding changes but ignores metadata-only revisions", () => {
	const left = artifact("vis_1", 13, { slots: [{ source: "sse", poll_url: "/poll-a" }] });
	const metadataOnly = { ...left, revision: 14, metadata: { summary: "new copy" } };
	const rebound = artifact("vis_1", 14, { slots: [{ source: "sse", poll_url: "/poll-b" }] });
	assert.equal(bindingAuthorityKey(left.bindings), bindingAuthorityKey(metadataOnly.bindings));
	assert.notEqual(bindingAuthorityKey(left.bindings), bindingAuthorityKey(rebound.bindings));
});
