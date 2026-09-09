import assert from 'node:assert/strict';
import test from 'node:test';
import { build } from 'esbuild';
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';

test('terminal Harbor results survive an unavailable retained producer', async () => {
  const root = fileURLToPath(new URL('../../', import.meta.url));
  const bundle = await build({ stdin: { contents: `
    import React from 'react';
    import {createRoot} from 'react-dom/client';
    import {Shell} from './visuals/families/first_class_example_containers/live.harbor_eval.v1/shell.tsx';
    const experiment={schemaVersion:'synth.experiment.overview.v1',status:'completed',
      aggregate:{lifecycle:'terminal',work:{planned:1,succeeded:1},meanReward:200},
      results:{rollouts:[{id:'rollout',label:'woodcutting',status:'completed',reward:200,traceId:'tracev5_retained'}]}};
    const replay={streams:[{streamId:'recorded',pollUrl:'http://127.0.0.1:1/events'}],
      poll:async()=>{throw new Error('recorded producer unavailable')}};
    createRoot(document.getElementById('root')).render(<Shell experiment={experiment} replay={replay}/>);
  `, resolveDir: root, loader: 'tsx' }, bundle: true, write: false, format: 'iife', platform: 'browser', jsx: 'automatic', outfile: 'harbor.js' });
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.setContent('<div id="root"></div>');
    await page.addScriptTag({ content: bundle.outputFiles.find(f => f.path.endsWith('.js')).text });
    await page.getByTestId('harbor-restored').waitFor();
    await page.getByRole('alert').filter({hasText:'recorded producer unavailable'}).waitFor();
    const body = await page.locator('body').innerText();
    assert.match(body, /1\/1 completed/);
    assert.match(body, /reward 200\.00 · trace tracev5_retained/);
    assert.doesNotMatch(body, /connecting|Waiting for trial/);
    assert.equal(await page.getByTestId('harbor-replay-controls').count(), 0);
  } finally { await browser.close(); }
});
