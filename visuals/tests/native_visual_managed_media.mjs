// Managed HTML media acceptance: the frame lane, end to end, in the pane.
//
// `native_visual_managed.mjs` covers a managed document's committed state.
// This one covers the half that reads media: a managed pane bound to an
// optimizer run polls the frame lane, receives chunked PNG bodies over the
// sandbox boundary, retains a per-seed history, serves a *selected historical*
// frame, and keeps all of that across capture, restore and replay.
//
// It requires `native_visual_optimizer_frames.mjs` to have admitted a run, and
// reads that run's receipt rather than inventing frames of its own.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join, resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const root = process.argv[2];
assert.ok(root?.startsWith('/tmp/workshop-visuals-native-'), 'Usage: native_visual_managed_media.mjs <native-root>');
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

const receipt = JSON.parse(readFileSync(join(root, 'acceptance/optimizer-frames-acceptance.json'), 'utf8'));
const runId = receipt.optimizerRunId;
const bySeed = new Map();
for (const frame of receipt.frames) {
  bySeed.set(frame.seed, [...(bySeed.get(frame.seed) ?? []), frame]);
}
for (const frames of bySeed.values()) frames.sort((a, b) => a.frameSequence - b.frameSequence);
assert.ok([...bySeed.values()].some((frames) => frames.length > 1),
  'the frame receipt must contain a seed with more than one frame');

const {template} = await request('/v1/visuals/templates/import', {
  sourcePath: join(repo, 'visuals/tests/fixtures/accept.managed-media.v1'),
});
const id = 'accept-managed-media-' + crypto.randomUUID();
const created = (await request('/v1/visuals', {
  id,
  templateId: template.id,
  title: 'Managed media native acceptance',
  workspaceOwned: true,
  bindings: {
    schemaVersion: 'synth.visual-bindings.v1',
    inputs: [{input: 'data', kind: 'inline', schema: 'synth.visual.managed_payload.v1', data: {run: {id: runId}}}],
  },
  metadata: {acceptanceFixture: true},
})).visual;
await request(`/v1/visuals/${id}/show`, {presentation: 'pane'});
const engine = (operation, fields = {}) =>
  request(`/v1/visuals/${id}/engine`, {operation, revision: created.currentRevision, ...fields});
const capture = async () => {
  let last;
  for (let attempt = 0; attempt < 8; attempt += 1) {
    try {return await engine('capture.pixels');} catch (error) {last = error;}
    await new Promise((r) => setTimeout(r, 750));
  }
  throw last;
};

// The pane must reach a live frame on its own: the managed document never
// fetches, so anything it shows came over the native media port.
let live;
for (let attempt = 0; attempt < 160; attempt += 1) {
  try {
    const session = await engine('inspect');
    if (session.state?.values['frame.shownDigest']) {live = session.state; break;}
  } catch {}
  await new Promise((r) => setTimeout(r, 250));
}
assert.ok(live, 'the managed pane never received a live frame from the media port');
assert.equal(live.values['frame.shownMode'], 'live');
// The lane coalesces per seed and the pane shows the last one delivered; which
// seed that is belongs to the relay, not to this test. Take the seed the pane
// reports and hold the relay to *that* seed's frames.
const seed = live.values['frame.shownSeed'];
const seedFrames = bySeed.get(seed);
assert.ok(seedFrames?.length > 1, `the shown seed ${seed} must have a frame history in the receipt`);
const latestForSeed = seedFrames[seedFrames.length - 1];
assert.equal(live.values['frame.shownSequence'], latestForSeed.frameSequence,
  'the live lane coalesces to the newest frame for the seed');
assert.equal('sha256:' + String(live.values['frame.shownDigest']).replace(/^sha256:/, ''), latestForSeed.sha256,
  'the displayed body must be the frame the lane names, not a neighbour');

// History is retained per seed, and is a list rather than one reference.
let withHistory = live;
for (let attempt = 0; attempt < 80 && !(withHistory.values['frame.historyDepth'] > 0); attempt += 1) {
  await new Promise((r) => setTimeout(r, 250));
  withHistory = (await engine('inspect')).state;
}
assert.ok(withHistory.values['frame.historyDepth'] >= seedFrames.length,
  `the retained history must cover every frame for the seed (saw ${withHistory.values['frame.historyDepth']})`);

const liveCut = await capture();
assert.equal(liveCut.pixelCut.paint.verified, true);
assert.equal(liveCut.pixelCut.checkpoint.state.values['frame.shownDigest'], live.values['frame.shownDigest'],
  'a cut must carry the digest of the image it photographed');

// Select an *older* frame for the same seed. The host serves it from the media
// port as a history read; the pane must show that body, not the live one.
const historical = seedFrames[0];
assert.notEqual(historical.frameSequence, latestForSeed.frameSequence);
const recording = await engine('record.start');
let state = (await engine('act', {
  action: {
    id: crypto.randomUUID(),
    kind: 'presentation.patch',
    expectedStateVersion: withHistory.stateVersion,
    payload: {values: {'frame.selectedSeed': seed, 'frame.selectedSequence': historical.frameSequence}},
  },
})).state;
assert.equal(state.values['frame.selectedSequence'], historical.frameSequence,
  'the selection must commit before the document can be asked to honour it');
for (let attempt = 0; attempt < 120; attempt += 1) {
  if (state.values['frame.shownSequence'] === historical.frameSequence) break;
  await new Promise((r) => setTimeout(r, 250));
  state = (await engine('inspect')).state;
}
if (state.values['frame.shownSequence'] !== historical.frameSequence) {
  const diagnostic = await capture().catch(() => null);
  console.error('historical selection never served; pane capture:', diagnostic?.path ?? 'unavailable');
}
assert.equal(state.values['frame.shownSequence'], historical.frameSequence,
  'the selected historical frame was never served');
assert.equal(state.values['frame.shownMode'], 'history');
assert.equal('sha256:' + String(state.values['frame.shownDigest']).replace(/^sha256:/, ''), historical.sha256,
  'a historical selection must pair with its own image');

const historicalCut = await capture();
assert.equal(historicalCut.pixelCut.paint.verified, true);
assert.equal(historicalCut.pixelCut.checkpoint.state.values['frame.shownDigest'], state.values['frame.shownDigest']);
assert.notEqual(historicalCut.pixelCut.checkpoint.state.values['frame.shownDigest'],
  liveCut.pixelCut.checkpoint.state.values['frame.shownDigest'],
  'two cuts of different frames must not share one image');
await engine('record.stop');

// Restoring the live cut brings the pane back to the frame it photographed.
const restored = await engine('restore', {
  checkpointId: liveCut.pixelCut.checkpoint.id,
  expectedStateVersion: (await engine('inspect')).state.stateVersion,
});
assert.equal(restored.state.values['frame.shownDigest'], live.values['frame.shownDigest']);
const restoredCut = await capture();
assert.equal(restoredCut.pixelCut.paint.verified, true);

// Two selections in flight at once: the later one must win. A historical frame
// read is asynchronous, so an earlier reply landing after a newer selection
// would leave the pane showing an image the committed state no longer names --
// a cut paired with the wrong frame, which is exactly what a reviewer cannot
// detect by looking.
const raced = seedFrames[1];
assert.notEqual(raced.frameSequence, historical.frameSequence);
let racing = (await engine('act', {
  action: {
    id: crypto.randomUUID(),
    kind: 'presentation.patch',
    expectedStateVersion: (await engine('inspect')).state.stateVersion,
    payload: {values: {'frame.selectedSeed': seed, 'frame.selectedSequence': historical.frameSequence}},
  },
})).state;
racing = (await engine('act', {
  action: {
    id: crypto.randomUUID(),
    kind: 'presentation.patch',
    expectedStateVersion: racing.stateVersion,
    payload: {values: {'frame.selectedSeed': seed, 'frame.selectedSequence': raced.frameSequence}},
  },
})).state;
for (let attempt = 0; attempt < 120; attempt += 1) {
  if (racing.values['frame.shownSequence'] === raced.frameSequence) break;
  await new Promise((r) => setTimeout(r, 250));
  racing = (await engine('inspect')).state;
}
assert.equal(racing.values['frame.shownSequence'], raced.frameSequence,
  'the later selection must win the race, not whichever read replied last');
assert.equal('sha256:' + String(racing.values['frame.shownDigest']).replace(/^sha256:/, ''), raced.sha256);
// Hold still and confirm nothing arrives late to overwrite it.
await new Promise((r) => setTimeout(r, 3000));
const settled = (await engine('inspect')).state;
assert.equal(settled.values['frame.shownSequence'], raced.frameSequence,
  'a superseded read must not land after the selection that replaced it');

const evidence = {
  schemaVersion: 'workshop.visuals-managed-media-acceptance.v1',
  visualId: id,
  optimizerRunId: runId,
  seed,
  liveFrame: {sequence: latestForSeed.frameSequence, sha256: latestForSeed.sha256},
  historicalFrame: {sequence: historical.frameSequence, sha256: historical.sha256},
  historyDepth: withHistory.values['frame.historyDepth'],
  recordingId: recording.recordingId,
  racedSelection: {superseded: historical.frameSequence, winner: raced.frameSequence},
  captures: [liveCut.path, historicalCut.path, restoredCut.path],
};
writeFileSync(join(root, 'acceptance/managed-media-acceptance.json'), JSON.stringify(evidence, null, 2));
console.log(JSON.stringify({
  visualId: id, seed, live: latestForSeed.frameSequence, historical: historical.frameSequence,
  historyDepth: evidence.historyDepth, captures: evidence.captures,
}));
