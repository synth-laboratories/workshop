// Deterministic, explicitly labeled Trace V5 acceptance producer.
//
// This is ENGINEERING TEST DATA, not an experimental measurement. Every event
// and artifact it writes carries `producer.synthetic = true` and names this
// script, so nothing downstream can mistake a sealed fixture for a paid or
// live run. No provider is called and no credential is read.
//
// It never writes rows into Workshop's SQLite directly. It drives the exact
// format-authority CLI Workshop itself requires (`synth-trace serve`, the
// detached capture supervisor), so the bundle it produces is sealed, verified
// and self-contained by the same code path a real container capture uses.
//
// Usage:
//   node visuals/tests/produce_trace_v5_fixture.mjs <output-root> [--rollouts=N] [--events=N] [--frames=N]
import {spawn, execFileSync} from 'node:child_process';
import {randomBytes, createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync, rmSync} from 'node:fs';
import {join, resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {homedir} from 'node:os';
import assert from 'node:assert/strict';
import {deterministicPng} from './deterministic_png.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const outputRoot = process.argv[2];
if (!outputRoot || !/^\/(tmp|private\/tmp)\//.test(outputRoot)) {
  throw new Error('Usage: produce_trace_v5_fixture.mjs /tmp/... [--rollouts=N] [--events=N] [--frames=N]');
}
const numeric = (flag, fallback) => {
  const raw = process.argv.find(argument => argument.startsWith(`--${flag}=`));
  if (!raw) return fallback;
  const value = Number(raw.split('=')[1]);
  assert.ok(Number.isInteger(value) && value > 0 && value <= 512, `--${flag} must be a small positive integer`);
  return value;
};
const rollouts = numeric('rollouts', 3);
const events = numeric('events', 24);
const frames = numeric('frames', 4);

// The format authority is version-pinned by Workshop itself. Resolve exactly
// what `resolve_trace_cli()` would resolve; never fall back to PATH.
const requiredVersion = readFileSync(join(repo, 'apps/synth_desktop/src-tauri/synth-containers-version.txt'), 'utf8').trim();
const cli = join(homedir(), '.synth-desktop/dev-builds/synth-containers', requiredVersion, 'current/.venv/bin/synth-trace');
const installed = execFileSync(cli, ['version'], {encoding: 'utf8'}).trim();
assert.equal(installed, requiredVersion, 'registered synth-containers must match the version Workshop pins');

mkdirSync(outputRoot, {recursive: true});
const token = randomBytes(24).toString('hex');
const service = spawn(cli, ['serve', '--output', outputRoot, '--host', '127.0.0.1', '--port', '0', '--control-token-env', 'SYNTH_TRACE_CONTROL_TOKEN'], {
  env: {...process.env, SYNTH_TRACE_CONTROL_TOKEN: token},
  stdio: ['ignore', 'pipe', 'pipe'],
});
let stderr = '';
service.stderr.on('data', data => {stderr += data;});
const baseUrl = await new Promise((resolveUrl, rejectUrl) => {
  let buffer = '';
  const timer = setTimeout(() => rejectUrl(new Error(`capture service did not start: ${stderr}`)), 30_000);
  service.stdout.on('data', data => {
    buffer += data;
    try {
      const parsed = JSON.parse(buffer);
      clearTimeout(timer);
      resolveUrl(parsed.base_url);
    } catch {}
  });
  service.on('exit', code => {clearTimeout(timer); rejectUrl(new Error(`capture service exited ${code}: ${stderr}`));});
});
assert.match(baseUrl, /^http:\/\/127\.0\.0\.1:\d+$/, 'capture service must stay on loopback');

const call = async (path, body, method = 'POST') => {
  const response = await fetch(baseUrl + path, {
    method,
    headers: {Authorization: 'Bearer ' + token, 'Content-Type': 'application/json'},
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(30_000),
  });
  const value = await response.json();
  if (!response.ok || value.error) throw new Error(JSON.stringify(value));
  return value;
};

const PRODUCER = {
  synthetic: true,
  producer: 'workshop-visuals/produce_trace_v5_fixture.mjs',
  purpose: 'v0.10 visuals acceptance: retained trace and media replay',
  measurement: false,
};

const sealed = [];
try {
  for (let index = 0; index < rollouts; index += 1) {
    const rolloutId = `visuals-acceptance-rollout-${index + 1}`;
    const opened = await call('/captures', {
      rollout_id: rolloutId,
      capture_mode: 'required',
      labels: {...PRODUCER, rollout_index: String(index + 1), suite: 'visuals-v010-acceptance'},
    });
    const captureId = opened.capture_id;
    const frameDigests = [];
    for (let step = 0; step < events; step += 1) {
      const emitsFrame = step % Math.max(1, Math.floor(events / frames)) === 0 && frameDigests.length < frames;
      let artifactId = null;
      if (emitsFrame) {
        const image = deterministicPng(48, 32, index * 97 + step);
        const artifact = await call(`/captures/${captureId}/artifacts`, {
          role: 'observation',
          media_type: 'image/png',
          logical_name: `frame-${String(frameDigests.length).padStart(3, '0')}.png`,
          content_base64: image.toString('base64'),
        });
        artifactId = artifact.artifact_id ?? artifact.id ?? null;
        frameDigests.push({
          index: frameDigests.length,
          step,
          artifactId,
          sha256: 'sha256:' + createHash('sha256').update(image).digest('hex'),
          bytes: image.length,
        });
      }
      // A frame event carries its own artifact descriptor. That is what a
      // sealed-trace consumer reads to resolve media by content digest; the
      // projection's per-event detail is the only place it can appear, because
      // the bundle's top-level artifact list is not part of this projection.
      if (emitsFrame) {
        const frame = frameDigests[frameDigests.length - 1];
        await call(`/captures/${captureId}/events`, {
          event_type: 'frame',
          payload: {
            ...PRODUCER,
            rollout_id: rolloutId,
            step,
            format: 'png',
            artifacts: [{
              artifact_id: artifactId,
              digest: frame.sha256,
              media_type: 'image/png',
              size_bytes: frame.bytes,
              logical_name: `frame-${String(frame.index).padStart(3, '0')}.png`,
              role: 'observation',
              metadata: {width: 48, height: 32},
            }],
          },
        });
      }
      await call(`/captures/${captureId}/events`, {
        event_type: 'craftax.turn',
        payload: {
          ...PRODUCER,
          rollout_id: rolloutId,
          step,
          action: ['noop', 'left', 'right', 'up', 'down', 'do'][step % 6],
          achievements: step % 7 === 6 ? ['collect_wood'] : [],
          reward: Number(((step % 5) / 10).toFixed(2)),
          frame_artifact_id: artifactId,
        },
      });
    }
    const result = await call(`/captures/${captureId}/seal`, {status: 'completed'});
    sealed.push({rolloutId, captureId, frames: frameDigests, ...result});
  }
} finally {
  service.kill('SIGINT');
}

// Prove self-containment through the format authority before Workshop ever
// sees the bundle. A bundle that does not verify here is not acceptance data.
for (const entry of sealed) {
  const verified = JSON.parse(execFileSync(cli, ['verify', entry.bundle_path], {encoding: 'utf8'}));
  assert.equal(verified.self_contained ?? verified.selfContained, true, `bundle ${entry.bundle_path} must be self-contained`);
  entry.verify = verified;
  const archive = `${entry.bundle_path}.zip`;
  rmSync(archive, {force: true});
  execFileSync(cli, ['archive', entry.bundle_path, archive]);
  entry.archive_path = archive;
}

const manifest = {
  schemaVersion: 'workshop.visuals-acceptance-trace-fixture.v1',
  ...PRODUCER,
  containersVersion: requiredVersion,
  outputRoot,
  rollouts: sealed.map(({rolloutId, captureId, trace_id, trace_v5_digest, event_count, bundle_path, archive_path, frames: frameDigests}) => ({
    rolloutId, captureId, traceId: trace_id, traceDigest: trace_v5_digest,
    eventCount: event_count, bundlePath: bundle_path, archivePath: archive_path, frames: frameDigests,
  })),
};
const manifestPath = join(outputRoot, 'trace-v5-acceptance-manifest.json');
writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
console.log(JSON.stringify({manifestPath, rollouts: manifest.rollouts.length,
  events: manifest.rollouts.reduce((total, item) => total + item.eventCount, 0),
  frames: manifest.rollouts.reduce((total, item) => total + item.frames.length, 0)}));
