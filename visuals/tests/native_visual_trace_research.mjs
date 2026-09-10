// Native trace-research pane acceptance against a populated sealed Trace V5.
//
// Opens an imported trace through the ordinary `traces/open` path, then drives
// the mounted inspector: a human section change and a nested event selection,
// a stale action that must be refused, a verified pixel capture, replay back to
// the opening state, restore of the captured checkpoint, and finally the real
// native MCP process reading the same session — so human and agent are shown to
// share one committed state rather than two.
//
// Requires the fixture from `produce_trace_v5_fixture.mjs` to have been imported
// by `native_visual_trace_media.mjs`.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFileSync, writeFileSync} from 'node:fs';
import {join, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';

const root = process.argv[2];
const fixtureRoot = process.argv[3];
assert.ok(root?.startsWith('/tmp/workshop-visuals-native-'), 'Usage: native_visual_trace_research.mjs <native-root> <fixture-root>');
const helper = fileURLToPath(new URL('./native_visual_mcp.mjs', import.meta.url));
const connection = JSON.parse(readFileSync(join(root, 'visuals-ipc.json'), 'utf8'));
assert.match(connection.url, /^http:\/\/127\.0\.0\.1:\d+$/);
const request = async (path, body, method = body === undefined ? 'GET' : 'POST') => {
  const response = await fetch(connection.url + path, {
    method,
    headers: {Authorization: 'Bearer ' + connection.token, 'Content-Type': 'application/json'},
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(90_000),
  });
  const value = await response.json();
  if (!response.ok) throw new Error(`${path}: ${JSON.stringify(value).slice(0, 300)}`);
  return value;
};

const manifest = JSON.parse(readFileSync(join(fixtureRoot, 'trace-v5-acceptance-manifest.json'), 'utf8'));
const target = manifest.rollouts[0];
const rows = (await request('/v1/traces')).traces;
const row = rows.find((candidate) => candidate.digest === target.traceDigest);
assert.ok(row, 'the acceptance trace must be imported first');
assert.equal(row.inspectability, 'Inspect');

const opened = await request('/v1/traces/open', {trace_id: row.traceId});
const id = opened.visual.id;
const revision = opened.visual.currentRevision;
const engine = (operation, fields = {}) => request(`/v1/visuals/${id}/engine`, {operation, revision, ...fields});
// The paint barrier refuses a cut while the pane is still settling. That is the
// guarantee, not a flake: retry the request rather than weakening the barrier.
const capture = async () => {
  let last;
  for (let attempt = 0; attempt < 6; attempt += 1) {
    try {return await engine('capture.pixels');} catch (error) {last = error;}
    await new Promise((resolve) => setTimeout(resolve, 750));
  }
  throw last;
};

// `show` alone must bring the pane up. This used to be a silent no-op for a
// workspace visual, so the wait is also the regression test for that.
let initial;
for (let attempt = 0; attempt < 80; attempt += 1) {
  try {
    const session = await engine('inspect');
    if (session.state?.scene) {initial = session.state; break;}
  } catch {}
  await new Promise((resolve) => setTimeout(resolve, 250));
}
assert.ok(initial, 'the trace inspector did not mount within the acceptance deadline');

// The pane is bound to this sealed trace and nothing else.
const traceScoped = initial.controls.map((control) => control.id).filter((control) => control.startsWith('agent.'));
assert.ok(traceScoped.length > 0, 'the inspector must register trace-scoped controls');
const scope = traceScoped[0].split('.')[1];
const section = `agent.${scope}.section`;
const selected = `agent.${scope}.selected`;
const sectionControl = initial.controls.find((control) => control.id === section);
assert.ok(sectionControl?.options?.length > 1, 'the inspector must offer more than one section');
// A committed session survives restarts and earlier runs, so this asserts a
// round trip from whatever state the pane opens in, never a pristine one.
// Prefer the populated section: acceptance on an empty one photographs an
// honest "nothing here" rather than the events this trace actually carries.
const nextSection = ['events', 'rollout', ...sectionControl.options]
  .find((option) => sectionControl.options.includes(option) && option !== initial.values[section]);
assert.ok(nextSection);

const window = await request('/v1/traces/window', {trace_digest: row.digest, offset: 0, limit: 200});
assert.equal(window.view_window.total, target.eventCount);
const frame = window.visual.items.find((item) =>
  item.kind === 'frame' && item.item_id !== initial.values[selected]);
assert.ok(frame, 'the acceptance trace carries a frame event that is not already selected');

const recording = await engine('record.start');

// Human: switch section, then select one nested event inside it.
const sectionAction = {
  id: crypto.randomUUID(),
  kind: 'presentation.set',
  target: {id: section},
  expectedStateVersion: initial.stateVersion,
  payload: {value: nextSection},
};
let state = (await engine('act', {action: sectionAction})).state;
assert.equal(state.values[section], nextSection);
// The same action replayed against the version it already consumed is stale.
await assert.rejects(() => engine('act', {action: {...sectionAction, id: crypto.randomUUID()}}), /stale/);

state = (await engine('act', {
  action: {
    id: crypto.randomUUID(),
    kind: 'presentation.set',
    target: {id: selected},
    expectedStateVersion: state.stateVersion,
    payload: {value: frame.item_id},
  },
})).state;
assert.equal(state.values[selected], frame.item_id, 'the nested selection is committed state');

const captured = await capture();
assert.equal(captured.pixelCut.paint.verified, true);
assert.equal(captured.pixelCut.checkpoint.state.values[selected], frame.item_id,
  'the cut must carry the selection it photographed');
await engine('record.stop');

// Replay to the opening event returns the pane to how it opened.
const beforeReplay = await engine('inspect');
const replayed = await engine('record.seek', {
  recordingId: recording.recordingId,
  sequence: 0,
  expectedStateVersion: beforeReplay.state.stateVersion,
});
assert.deepEqual(replayed.state.values, initial.values, 'replay must restore the opening state exactly');

// Restoring the checkpoint brings back both the section and the nested item.
const restored = await engine('restore', {
  checkpointId: captured.pixelCut.checkpoint.id,
  expectedStateVersion: replayed.state.stateVersion,
});
assert.equal(restored.state.values[section], nextSection);
assert.equal(restored.state.values[selected], frame.item_id);
const restoredCapture = await capture();
assert.equal(restoredCapture.pixelCut.paint.verified, true);

// The real MCP process — not a mock — must read the same committed state.
const mcp = JSON.parse(execFileSync(process.execPath,
  [helper, root, id, 'inspect', JSON.stringify({revision})], {encoding: 'utf8', timeout: 60_000}));
assert.equal(mcp.state.values[selected], frame.item_id, 'human and MCP must share one session, not two');
assert.equal(mcp.state.values[section], nextSection);

const evidence = {
  schemaVersion: 'workshop.visuals-trace-research-acceptance.v1',
  visualId: id,
  revision,
  traceDigest: row.digest,
  eventCount: window.view_window.total,
  pinnedSnapshot: window.view_window.snapshotDigest,
  selectedEvent: frame.item_id,
  section: nextSection,
  recordingId: recording.recordingId,
  staleRejected: true,
  replayRestored: true,
  checkpointRestored: true,
  mcpAgrees: true,
  captures: [captured.path, restoredCapture.path],
};
writeFileSync(join(root, 'acceptance/trace-research-acceptance.json'), JSON.stringify(evidence, null, 2));
console.log(JSON.stringify({
  visualId: id, revision, eventCount: evidence.eventCount,
  selectedEvent: evidence.selectedEvent, recordingId: evidence.recordingId,
  captures: evidence.captures,
}));
