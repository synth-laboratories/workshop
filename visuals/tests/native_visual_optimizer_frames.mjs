// Optimizer run frame/media acceptance against an isolated native instance.
//
// The sealed-trace path proves a frame can be resolved from a bundle. This one
// covers the other media authority: the live optimizer relay, which is what
// Craftax and annotated-eval panes actually read. The instance had zero
// `optimizer_frames` and zero `optimizer_run_media`, so nothing downstream of
// that relay had ever been exercised against real bytes.
//
// The journal is derived from the packaged eval example by adding PNG frames to
// its existing trial events under a fresh run id. The frames are explicitly
// labelled deterministic test data, not a measurement, and they are admitted
// through the ordinary `optimizers/import_local` path — no SQLite row is
// written directly, and no provider is called.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join, resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {deterministicPng} from './deterministic_png.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const root = process.argv[2];
assert.ok(root?.startsWith('/tmp/workshop-visuals-native-'), 'Usage: native_visual_optimizer_frames.mjs <native-root>');
const connection = JSON.parse(readFileSync(join(root, 'visuals-ipc.json'), 'utf8'));
assert.match(connection.url, /^http:\/\/127\.0\.0\.1:\d+$/);
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

const source = join(repo, 'packages/workshop-visuals/families/optimizers/eval/optimizer.eval.live.v1/examples/events.json');
const fixture = JSON.parse(readFileSync(source, 'utf8'));
assert.ok(Array.isArray(fixture.events) && fixture.events.length);

// Every acceptance import owns a fresh run: reusing the packaged run id resets
// its header while journal dedup skips the old events, which produces an
// invalid empty projection rather than an honest failure.
const originalRunId = fixture.events[0].optimizerRunId;
const runId = 'accept-frames-' + crypto.randomUUID();
const events = JSON.parse(JSON.stringify(fixture.events).replaceAll(originalRunId, runId));

const PRODUCER = {
  synthetic: true,
  producer: 'workshop-visuals/native_visual_optimizer_frames.mjs',
  measurement: false,
};

// Attach a frame to each trial event that already declares a seed. The relay's
// admission contract is `container_event.seed` plus a PNG data URL; nothing
// else about the packaged event is changed.
const expected = [];
for (const event of events) {
  const container = event.delta?.container_event ?? event.raw?.container_event;
  const seed = container?.seed;
  if (event.type !== 'eval.trial.event' || typeof seed !== 'number') continue;
  const image = deterministicPng(64, 48, seed * 13 + event.sequenceNumber);
  const dataUrl = 'data:image/png;base64,' + image.toString('base64');
  for (const target of [event.delta?.container_event, event.raw?.container_event]) {
    if (!target) continue;
    target.frame = {data_url: dataUrl, ...PRODUCER};
  }
  expected.push({
    seed,
    frameSequence: event.sequenceNumber,
    sha256: 'sha256:' + createHash('sha256').update(image).digest('hex'),
    bytes: image.length,
  });
}
assert.ok(expected.length >= 4, 'the packaged example must supply several seeded trial events');

const journal = join(root, 'acceptance-optimizer-frames.jsonl');
writeFileSync(journal, events
  .map((event) => JSON.stringify({...event, _seq: event.sequenceNumber, optimizer_run_id: event.optimizerRunId, algorithm_id: event.algorithmId}))
  .join('\n') + '\n');
const imported = await request('/v1/optimizers/import_local', {path: journal, openVisual: false});
assert.equal(imported.run?.id, runId);

// The inline body must not survive in the journal: the relay strips it and
// keeps the bytes once, in the run's media store, addressed by digest.
const stored = new Map();
for (const frame of expected) {
  const digest = frame.sha256.slice('sha256:'.length);
  const body = await request('/v1/cas/' + digest);
  const bytes = Buffer.from(body.base64, 'base64');
  assert.equal('sha256:' + createHash('sha256').update(bytes).digest('hex'), frame.sha256,
    `admitted frame ${digest} did not hash to the bytes that were offered`);
  assert.equal(bytes.length, frame.bytes);
  assert.equal(bytes.subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
  assert.ok(!stored.has(frame.sha256), 'each frame must be its own body, not a reused image');
  stored.set(frame.sha256, frame.frameSequence);
}

// Re-importing the same journal admits no second copy of any frame: the media
// store is content-addressed, and a replayed event is a confirmed replay.
const again = await request('/v1/optimizers/import_local', {path: journal, openVisual: false});
assert.equal(again.run?.id, runId);
for (const frame of expected) {
  const body = await request('/v1/cas/' + frame.sha256.slice('sha256:'.length));
  assert.equal('sha256:' + createHash('sha256').update(Buffer.from(body.base64, 'base64')).digest('hex'), frame.sha256);
}

// Retained is not served. The frame lane derives its rows by joining the event
// journal against the media store, so a run whose frames are catalogued can
// still expose none of them if the two sides disagree about where the digest
// lives. Read them back through the ordinary run frame API.
const workshop = async (name, args) =>
  (await request('/v1/workshop/call', {data_root: root, contract_version: 1, name, arguments: args})).result;

const delta = await workshop('optimizers_frames_latest', {optimizerRunId: runId, afterFrameSequence: 0});
assert.equal(delta.observedFrames, expected.length,
  'every retained frame must also be observable through the frame lane');
const seeds = new Set(expected.map((frame) => frame.seed));
assert.equal(delta.frames.length, seeds.size,
  'the lane coalesces to the latest frame per seed');
for (const frame of delta.frames) {
  assert.ok(seeds.has(frame.seed), 'the seed must resolve from the canonical spelling');
  assert.equal(frame.contentType, 'image/png');
}

// History and body reads are lazy and per seed; both must find the same bytes.
const seed = delta.frames[0].seed;
const history = await workshop('optimizers_frames_list', {optimizerRunId: runId, seed, limit: 10});
assert.ok(history.length >= 1);
assert.deepEqual([...history].sort((a, b) => b.frameSequence - a.frameSequence), history,
  'history is newest first');
const body = await workshop('optimizers_frame_content', {
  optimizerRunId: runId, seed, frameSequence: history[history.length - 1].frameSequence,
});
const decoded = Buffer.from(body.base64, 'base64');
assert.equal('sha256:' + createHash('sha256').update(decoded).digest('hex'), 'sha256:' + body.frame.contentDigest.replace(/^sha256:/, ''),
  'the served body must be the bytes the catalog names');
assert.equal(decoded.subarray(0, 8).toString('hex'), '89504e470d0a1a0a');

const evidence = {
  schemaVersion: 'workshop.visuals-optimizer-frames-acceptance.v1',
  ...PRODUCER,
  optimizerRunId: runId,
  sourceExample: 'families/optimizers/eval/optimizer.eval.live.v1/examples/events.json',
  frames: expected,
  distinctBodies: stored.size,
  observedFrames: delta.observedFrames,
  coalescedTo: delta.frames.length,
  historyDepth: history.length,
};
writeFileSync(join(root, 'acceptance/optimizer-frames-acceptance.json'), JSON.stringify(evidence, null, 2));
console.log(JSON.stringify({
  optimizerRunId: runId, frames: expected.length, distinctBodies: stored.size,
  observedFrames: delta.observedFrames, coalescedTo: delta.frames.length, historyDepth: history.length,
}));
