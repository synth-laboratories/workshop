import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve, dirname } from 'node:path';
import { gzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { build } from './build_deps.mjs';

const root = dirname(fileURLToPath(import.meta.url));
const bindings = JSON.parse(readFileSync(resolve(root, 'trace-bindings.json'), 'utf8'));
// Keep sealed traces untouched. Enrich only the rebuildable query projection
// with loopback source links, never absolute filesystem paths.
const raw = JSON.parse(readFileSync(resolve(root, 'swarm-data.json'), 'utf8'));
if (raw.revision !== bindings.swarmData.revision) throw new Error('Query/trace projection revisions differ; rebuild the index explicitly.');
for (const [name, result] of Object.entries(bindings.swarmData.queries)) {
  const originals = raw.queries[name].rows;
  result.rows.forEach((hit, index) => {
    const original = originals[index];
    if (!original || hit.run_id !== original.run_id || hit.actor !== original.actor || hit.ms !== original.ms) throw new Error('Query row identity mismatch');
    const match = String(original.ref || '').match(/\/(episode\/)?events\.jsonl:(\d+)$/);
    if (match) hit.evidenceUrl = `http://127.0.0.1:8128/evidence?run=${encodeURIComponent(hit.run_id)}&scope=${match[1]?'episode':'run'}&line=${match[2]}`;
  });
}
const bytes = Buffer.from(JSON.stringify(bindings));
// Always compile authored source; do not require a destructive index rebuild
// just to pick up a UI change.
writeFileSync(resolve(root, 'viewer.built.tsx'), readFileSync(resolve(root, 'viewer.tsx')));
const capability = execFileSync(process.env.RUNEBENCH_PYTHON || resolve(root,'.venv/bin/python'), ['-c', 'from evidence_api import capability; print(capability())'], { cwd: root, encoding: 'utf8' }).trim();
const data = { traceArchive: { encoding: 'gzip+base64', sha256: createHash('sha256').update(bytes).digest('hex'), data: gzipSync(bytes).toString('base64') }, annotationService: { url: 'http://127.0.0.1:8128/annotations', capability } };
mkdirSync(resolve(root, 'web'), { recursive: true });
writeFileSync(resolve(root, 'visual-request.json'), JSON.stringify({template_id:'sourced.visual.v1',title:'RuneBench · four-agent review',content:readFileSync(resolve(root,'viewer.tsx'),'utf8'),bindings:{schemaVersion:'synth.visual-bindings.v1',inputs:[{input:'data',kind:'inline',data}]} }), {mode:0o600});
// Optional update payload: use only an ID and revision returned by Workshop.
if (process.argv[2]) {
  const revision = Number(process.argv[3]);
  if (!/^vis_[a-zA-Z0-9]+$/.test(process.argv[2]) || !Number.isSafeInteger(revision) || revision < 1) throw new Error('Expected an existing visual ID and positive revision');
  const request = JSON.parse(readFileSync(resolve(root, 'visual-request.json'), 'utf8'));
  delete request.template_id;
  writeFileSync(resolve(root, 'visual-update.json'), JSON.stringify({...request,visual_id:process.argv[2],expected_revision:revision}), {mode:0o600});
}
const result = await build({ stdin: { contents: `import React from 'react'; import {createRoot} from 'react-dom/client'; import Shell from './viewer.built.tsx'; createRoot(document.getElementById('root')).render(<Shell data={${JSON.stringify(data)}}/>);`, resolveDir: root, loader: 'tsx' }, bundle: true, write: false, platform: 'browser', format: 'iife', jsx: 'automatic', minify: true, define: { 'process.env.NODE_ENV': '"production"' }, alias: { '@synth/visuals/components/agent_trace.v1': resolve(root,'../../packages/workshop-visuals/components/agent_trace.v1/AgentTraceInspector.tsx'), 'react-dom': resolve(root,'../../node_modules/react-dom'), react: resolve(root,'../../node_modules/react') } });
writeFileSync(resolve(root, 'web/index.html'), `<!doctype html><html><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>RuneBench trace review</title><style>body{margin:0}</style><div id="root"></div><script>${result.outputFiles[0].text.replaceAll('</script', '<\\/script')}</script></html>`);
console.log('Built standalone viewer with shared components and evidence binding.');
