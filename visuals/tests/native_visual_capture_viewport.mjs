// Capture below the initial viewport.
//
// `WKWebView takeSnapshot` photographs the webview's own surface, so a cut
// shows what a reviewer sees and nothing below the fold. `capture.pixels` is
// taken at the mounted pane's own fixed size, so it cannot reach further down;
// the review window is the surface whose viewport is adjustable, and this
// asserts that adjusting it really does photograph more of the same committed
// state rather than rescaling one fixed frame.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const root = process.argv[2];
assert.ok(root?.startsWith('/tmp/workshop-visuals-native-'), 'Usage: native_visual_capture_viewport.mjs <native-root>');
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

/** PNG pixel size, straight out of IHDR — no decoder needed. */
function pngSize(path) {
  const bytes = readFileSync(path);
  assert.equal(bytes.subarray(0, 8).toString('hex'), '89504e470d0a1a0a', `${path} is not a PNG`);
  assert.equal(bytes.subarray(12, 16).toString('ascii'), 'IHDR');
  return {width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20), bytes};
}

// A pane known to overflow a short viewport: the trace inspector renders its
// header, window controls, tabs and a full event timeline.
const id = 'accept-trace-rollout_inspector-v1';
const visual = (await request('/v1/visuals/' + id)).visual;
await request(`/v1/visuals/${id}/show`, {presentation: 'pane'});
const engine = (operation, fields = {}) =>
  request(`/v1/visuals/${id}/engine`, {operation, revision: visual.currentRevision, ...fields});
const capture = async () => {
  let last;
  for (let attempt = 0; attempt < 8; attempt += 1) {
    try {return await engine('capture.pixels');} catch (error) {last = error;}
    await new Promise((r) => setTimeout(r, 750));
  }
  throw last;
};
for (let attempt = 0; attempt < 80; attempt += 1) {
  try {if ((await engine('inspect')).state?.scene) break;} catch {}
  await new Promise((r) => setTimeout(r, 250));
}

// The pane's own cut is fixed to the mounted surface: record its size so the
// review comparison below is clearly about a different, adjustable surface.
const paneCut = await capture();
assert.equal(paneCut.pixelCut.paint.verified, true);
const pane = pngSize(paneCut.path);

const measured = [];
for (const [label, height] of [['short', 520], ['tall', 1400]]) {
  const path = join(root, `visual-review-captures/viewport-${label}.png`);
  mkdirSync(join(root, 'visual-review-captures'), {recursive: true});
  await request('/v1/review-window/capture', {visualId: id, width: 1280, height, outputPath: path});
  const {width, height: pixelHeight, bytes} = pngSize(path);
  measured.push({label, requestedHeight: height, width, pixelHeight, path,
    sha256: 'sha256:' + createHash('sha256').update(bytes).digest('hex')});
}

const [short, tall] = measured;
assert.ok(tall.pixelHeight > short.pixelHeight,
  `a taller review viewport must photograph more of the pane (${short.pixelHeight} → ${tall.pixelHeight})`);
assert.notEqual(tall.sha256, short.sha256, 'the two viewports must not produce the same image');
// Both cuts are of the same committed state: only the viewport changed.
assert.equal(short.width, tall.width, 'only the height was changed');

writeFileSync(join(root, 'acceptance/capture-viewport-acceptance.json'), JSON.stringify({
  schemaVersion: 'workshop.visuals-capture-viewport-acceptance.v1',
  visualId: id,
  revision: visual.currentRevision,
  paneCut: {width: pane.width, height: pane.height, path: paneCut.path},
  reviewCaptures: measured,
}, null, 2));
console.log(JSON.stringify({visualId: id, short: short.pixelHeight, tall: tall.pixelHeight,
  captures: measured.map((entry) => entry.path)}));
