import assert from 'node:assert/strict';
import test from 'node:test';
import { build } from 'esbuild';
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';

test('annotation identity survives protocol and app presentation switches', async () => {
  const root = fileURLToPath(new URL('../../', import.meta.url));
  const target = { trace_id: 't', trace_digest: 'sha', kind: 'event', entity_id: 'e' };
  const projection = { trace_id: 't', trace_digest: 'sha', items: [
    { item_id: 'row', kind: 'tool.completed', title: 'Chop result', actor_id: 'Ember', source_selector: target, detail: { action: { type: 'chop', x: 1, z: 2, reason: 'Reach the nearby tree' }, result: 'Tree was felled by someone else', elapsed_ms: 42000 } },
    { item_id: 'note', kind: 'evidence.annotation', source_selector: { ...target, json_pointer: '/result' }, detail: { labels: ['collision'], rationale: 'Two agents targeted the same tree', author_kind: 'human', review_state: 'accepted' } },
    { item_id: 'stale', kind: 'evidence.annotation', source_selector: { ...target, trace_digest: 'old' }, detail: { rationale: 'Old evidence' } },
    { item_id: 'cmd', kind: 'codex.command_finished', title: 'Command result', source_selector: { ...target, entity_id: 'cmd' }, detail: { native: { params: { item: { type: 'commandExecution', command: 'echo verified', aggregatedOutput: 'verified', exitCode: 0 } } } } },
  ] };
  const bundle = await build({ stdin: { contents: `import React from 'react'; import {createRoot} from 'react-dom/client'; import {AgentTraceInspector, runeBenchTraceExtension} from './visuals/components/agent_trace.v1/AgentTraceInspector.tsx'; createRoot(document.getElementById('root')).render(<AgentTraceInspector projection={${JSON.stringify(projection)}} extensions={[runeBenchTraceExtension]} onAnnotate={target => window.annotationTarget = target}/>);`, resolveDir: root, loader: 'tsx' }, bundle: true, write: false, format: 'iife', platform: 'browser', jsx: 'automatic', define: { 'process.env.NODE_ENV': '"production"' } });
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.setContent('<div id="root"></div>');
    await page.addScriptTag({ content: bundle.outputFiles[0].text });
    await page.getByRole('button', { name: 'Chop result', exact: true }).click();
    for (const view of ['general', 'react', 'codex', 'runebench']) {
      await page.getByLabel('Trace presentation').selectOption(view);
      assert.equal(await page.getByRole('complementary', { name: 'Trace annotations' }).getByText('Two agents targeted the same tree').count(), 1);
      assert.equal(await page.getByText('1 unresolved annotation targets').count(), 1);
      await page.locator('[data-trace-item-id="row"]').getByRole('button', { name: 'Annotate', exact: true }).click();
      assert.deepEqual(await page.evaluate(() => window.annotationTarget), target);
    }
    await page.getByLabel('Trace presentation').selectOption('codex');
    assert.equal(await page.getByText('Exit code 0', { exact: true }).count(), 1);
    await page.getByLabel('Annotated only').check();
    assert.equal(await page.locator('[data-trace-item-id="cmd"]').count(), 0);
    assert.equal(await page.locator('[data-trace-item-id="row"]').count(), 1);
    assert.deepEqual(errors, []);
  } finally { await browser.close(); }
});
