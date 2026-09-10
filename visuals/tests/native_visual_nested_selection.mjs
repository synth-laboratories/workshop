// Nested/composed selection acceptance for the families the generic sweep
// cannot exercise.
//
// `compose.cursor` and `eval.cursor` are object controls whose value is an
// identity into the bound stream, not a scalar the generic harness can nudge.
// The sweep therefore reports "no safely toggleable registered control" and the
// row stays open — a static-looking pane that is in fact the one place nested
// selection, retained identity and re-resolution are exercised at all.
//
// The identity is computed with the same `envelopeIdentity` the shell uses, so
// this asserts the real contract rather than a shape invented by the test: a
// cursor is retained as identity only, and must re-resolve to exactly one event.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join, resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const root = process.argv[2];
assert.ok(root?.startsWith('/tmp/workshop-visuals-native-'), 'Usage: native_visual_nested_selection.mjs <native-root>');
const {envelopeIdentity} = await import(join(repo, 'packages/workshop-visuals/runtime/liveStream.ts'));
const {resolveEventCursor} = await import(join(repo, 'packages/workshop-visuals/runtime/presentationSchemas.ts'));

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

const TARGETS = [
  {
    visualId: 'accept-live-eval_stream-v1',
    control: 'eval.cursor',
    example: 'packages/workshop-visuals/families/first_class_example_containers/live.eval_stream.v1/examples/fixture_binding.json',
    cursor: (identity) => ({identity}),
  },
  {
    visualId: 'accept-compose-visual-v1',
    control: 'compose.cursor',
    example: 'packages/workshop-visuals/families/first_class_example_containers/live.eval_stream.v1/examples/fixture_binding.json',
    // A composed pane also names which placement the selection belongs to, so
    // one identity cannot silently move between panes.
    cursor: (identity) => ({identity, placementId: 'stream'}),
  },
];

const results = [];
for (const target of TARGETS) {
  const engineFor = (visual) => (operation, fields = {}) =>
    request(`/v1/visuals/${target.visualId}/engine`, {operation, revision: visual.currentRevision, ...fields});
  const visual = (await request('/v1/visuals/' + target.visualId)).visual;
  const engine = engineFor(visual);
  const capture = async () => {
    let last;
    for (let attempt = 0; attempt < 6; attempt += 1) {
      try {return await engine('capture.pixels');} catch (error) {last = error;}
      await new Promise((r) => setTimeout(r, 750));
    }
    throw last;
  };

  await request(`/v1/visuals/${target.visualId}/show`, {presentation: 'pane'});
  let initial;
  for (let attempt = 0; attempt < 80; attempt += 1) {
    try {
      const session = await engine('inspect');
      if (session.state?.scene) {initial = session.state; break;}
    } catch {}
    await new Promise((r) => setTimeout(r, 250));
  }
  assert.ok(initial, `${target.visualId} did not mount`);
  const control = initial.controls.find((entry) => entry.id === target.control);
  assert.ok(control, `${target.visualId} must register ${target.control}`);
  assert.equal(control.type, 'object', 'a nested cursor is an object control, which is why the sweep skips it');

  // The identity comes from the bound stream through the shell's own resolver.
  const events = JSON.parse(readFileSync(join(repo, target.example), 'utf8')).inputs[0].data.events;
  const identity = envelopeIdentity(events[2], 2);
  assert.equal(resolveEventCursor(events, identity), events[2],
    'an identity that does not re-resolve to exactly one event is not a usable cursor');

  const recording = await engine('record.start');
  const action = {
    id: crypto.randomUUID(),
    kind: 'presentation.set',
    target: {id: target.control},
    expectedStateVersion: initial.stateVersion,
    payload: {value: target.cursor(identity)},
  };
  const changed = await engine('act', {action});
  assert.deepEqual(changed.state.values[target.control], target.cursor(identity),
    'the nested selection is committed state, retained as identity only');
  await assert.rejects(() => engine('act', {action: {...action, id: crypto.randomUUID()}}), /stale/);

  const selected = await capture();
  assert.equal(selected.pixelCut.paint.verified, true);
  assert.deepEqual(selected.pixelCut.checkpoint.state.values[target.control], target.cursor(identity));
  await engine('record.stop');

  const beforeReplay = await engine('inspect');
  const replayed = await engine('record.seek', {
    recordingId: recording.recordingId,
    sequence: 0,
    expectedStateVersion: beforeReplay.state.stateVersion,
  });
  assert.deepEqual(replayed.state.values, initial.values, 'replay must restore the opening state exactly');

  const restored = await engine('restore', {
    checkpointId: selected.pixelCut.checkpoint.id,
    expectedStateVersion: replayed.state.stateVersion,
  });
  assert.deepEqual(restored.state.values[target.control], target.cursor(identity),
    'a restored nested cursor keeps its identity');

  // An identity the stream cannot resolve must stay unresolved rather than
  // silently selecting a neighbour.
  const ambiguous = await engine('act', {
    action: {
      id: crypto.randomUUID(),
      kind: 'presentation.set',
      target: {id: target.control},
      expectedStateVersion: restored.state.stateVersion,
      payload: {value: target.cursor('no-such-event:999')},
    },
  });
  assert.equal(resolveEventCursor(events, ambiguous.state.values[target.control].identity), null,
    'a missing identity resolves to nothing, not to a neighbouring event');
  const unresolved = await capture();
  assert.equal(unresolved.pixelCut.paint.verified, true,
    'an unresolved selection is still a rendered fact and must remain reviewable');

  results.push({
    visualId: target.visualId,
    control: target.control,
    identity,
    recordingId: recording.recordingId,
    captures: [selected.path, unresolved.path],
  });
  console.log(`${target.visualId} :: ${target.control} -> ${identity}`);
}

writeFileSync(join(root, 'acceptance/nested-selection-acceptance.json'), JSON.stringify({
  schemaVersion: 'workshop.visuals-nested-selection-acceptance.v1',
  targets: results,
}, null, 2));
console.log(JSON.stringify({targets: results.length}));
