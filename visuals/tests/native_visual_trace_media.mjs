// Sealed Trace V5 import and retained-media acceptance against an isolated
// native instance.
//
// What this proves, and only this:
//   * a self-contained sealed bundle imports through the ordinary native path
//     (`data_traces_ingest`), trusted and validated by the format authority;
//   * the CAS blobs the bundle carries are typed from the sealed trace's own
//     artifact declarations, so a capture with frames reports `hasMedia`;
//   * every declared frame is resolvable byte-exact from Workshop's blob CAS,
//     which is what lets a replayed pane show a frame without polling any live
//     endpoint, and what keeps one cut from being paired with another's image;
//   * the trace research window pages over one pinned snapshot.
//
// It does not prove that a `trace_v5`-bound visual mounts and captures pixels;
// see the handoff for that open gate.
//
// Fixture data comes from `produce_trace_v5_fixture.mjs`, which is an explicitly
// labelled deterministic test producer, not an experimental measurement.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const root = process.argv[2];
const fixtureRoot = process.argv[3];
assert.ok(root?.startsWith('/tmp/workshop-visuals-native-'), 'Usage: native_visual_trace_media.mjs <native-root> <fixture-root>');
assert.ok(fixtureRoot?.startsWith('/tmp/'), 'The fixture root must be the producer output directory');

const connection = JSON.parse(readFileSync(join(root, 'visuals-ipc.json'), 'utf8'));
assert.match(connection.url, /^http:\/\/127\.0\.0\.1:\d+$/, 'Expected isolated loopback connection');
const request = async (path, body, method = body === undefined ? 'GET' : 'POST') => {
  const response = await fetch(connection.url + path, {
    method,
    headers: {Authorization: 'Bearer ' + connection.token, 'Content-Type': 'application/json'},
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(120_000),
  });
  const value = await response.json();
  if (!response.ok) throw new Error(`${path}: ${JSON.stringify(value).slice(0, 400)}`);
  return value;
};
// The same bridge the Workshop MCP uses; nothing here writes SQLite directly.
const workshop = async (name, args) =>
  (await request('/v1/workshop/call', {data_root: root, contract_version: 1, name, arguments: args})).result;

const manifest = JSON.parse(readFileSync(join(fixtureRoot, 'trace-v5-acceptance-manifest.json'), 'utf8'));
assert.equal(manifest.synthetic, true, 'Acceptance media must be labelled synthetic test data');
assert.ok(manifest.rollouts.length > 0);

const imported = [];
for (const rollout of manifest.rollouts) {
  const result = await workshop('data_traces_ingest', {
    request: {
      sourcePath: rollout.archivePath,
      sourceKind: 'acceptance_fixture_bundle',
      title: 'Visuals acceptance · ' + rollout.rolloutId,
    },
  });
  assert.equal(result.compatibilityLevel, 'native', `${rollout.rolloutId} must import as native Trace V5`);
  assert.equal(result.validation.valid, true);
  assert.equal(result.validation.self_contained, true);
  const [trace] = result.traces;
  assert.ok(trace, 'ingest returned no trace record');
  assert.equal(trace.digest, rollout.traceDigest, 'imported digest must be the sealed digest');
  // The bundle manifest types every CAS blob as application/octet-stream; only
  // the sealed trace says a blob is a frame. A capture with frames that reports
  // no media is invisible to the media filter and to media-bound visuals.
  assert.equal(trace.metadata.hasMedia, true, `${rollout.rolloutId} carries PNG frames and must report media`);
  assert.equal(trace.metadata.eventCount, rollout.eventCount);
  imported.push({rollout, trace});
}

// Re-importing the same bundle must leave the read model agreeing with itself.
// The index is what the media filter reads; an upsert that refreshed the record
// but not the index left a populated capture reporting no media.
for (const {rollout, trace} of imported) {
  const again = await workshop('data_traces_ingest', {
    request: {sourcePath: rollout.archivePath, sourceKind: 'acceptance_fixture_bundle'},
  });
  assert.equal(again.duplicate, true, 'a byte-identical bundle re-imports as a duplicate');
  assert.equal(again.traces[0].digest, trace.digest);
  assert.equal(again.traces[0].metadata.hasMedia, true, 're-import must not leave a stale media flag');
}

// Offline media: every declared frame resolves from Workshop's own blob CAS,
// byte-exact, and no two frames share a body.
const bodies = new Map();
for (const {rollout} of imported) {
  for (const frame of rollout.frames) {
    const digest = frame.sha256.slice('sha256:'.length);
    const body = await request('/v1/cas/' + digest);
    const bytes = Buffer.from(body.base64, 'base64');
    assert.equal('sha256:' + createHash('sha256').update(bytes).digest('hex'), frame.sha256,
      `CAS body for ${digest} did not hash to its declared digest`);
    assert.equal(bytes.length, frame.bytes);
    assert.equal(bytes.subarray(0, 8).toString('hex'), '89504e470d0a1a0a', 'a declared image/png frame must be a PNG');
    assert.ok(!bodies.has(frame.sha256), 'each frame must be its own body, not a reused image');
    bodies.set(frame.sha256, rollout.rolloutId);
  }
}
assert.equal(bodies.size, manifest.rollouts.reduce((total, item) => total + item.frames.length, 0));

// Research paging reads one pinned snapshot, not a moving projection.
const first = imported[0];
const head = await request('/v1/traces/window', {trace_digest: first.trace.digest, offset: 0, limit: 5});
assert.equal(head.schema_version, 'synth.trace-projection.rollout-inspector-window.v1');
assert.equal(head.trace_digest, first.trace.digest);
assert.equal(head.view_window.total, first.rollout.eventCount);
assert.equal(head.visual.items.length, 5);
assert.equal(head.visual.summary.artifact_count, first.rollout.frames.length);
const next = await request('/v1/traces/window', {
  trace_digest: first.trace.digest,
  snapshot_digest: head.view_window.snapshotDigest,
  offset: head.view_window.nextOffset,
  limit: 5,
});
assert.equal(next.view_window.snapshotDigest, head.view_window.snapshotDigest, 'paging must stay on one pinned snapshot');
assert.equal(next.view_window.offset, head.view_window.nextOffset);
const back = await request('/v1/traces/window', {
  trace_digest: first.trace.digest,
  snapshot_digest: head.view_window.snapshotDigest,
  offset: 0,
  limit: 5,
});
assert.deepEqual(back.visual.items, head.visual.items, 'a previous-window read must return the same pinned events');

const evidence = {
  schemaVersion: 'workshop.visuals-trace-media-acceptance.v1',
  producedBy: manifest.producer,
  synthetic: true,
  containersVersion: manifest.containersVersion,
  traces: imported.map(({rollout, trace}) => ({
    rolloutId: rollout.rolloutId,
    digest: trace.digest,
    eventCount: trace.metadata.eventCount,
    hasMedia: trace.metadata.hasMedia,
    frames: rollout.frames.map((frame) => frame.sha256),
  })),
  casFramesResolved: bodies.size,
  pinnedSnapshot: head.view_window.snapshotDigest,
};
writeFileSync(join(root, 'acceptance/trace-media-acceptance.json'), JSON.stringify(evidence, null, 2));
console.log(JSON.stringify({
  traces: evidence.traces.length,
  casFramesResolved: evidence.casFramesResolved,
  pinnedSnapshot: evidence.pinnedSnapshot,
}));
