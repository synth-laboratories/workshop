import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';
import { chromium } from 'playwright';

test('citation selection reveals filtered annotations and comparison needs one actor', async () => {
  const root = fileURLToPath(new URL('../../..', import.meta.url));
  const result = await build({ absWorkingDir: root, bundle:true, write:false, format:'iife', jsx:'automatic',
    stdin:{ resolveDir:root, loader:'tsx', contents:`
      import React,{useState} from 'react';
      import {createRoot} from 'react-dom/client';
      import {AgentTraceInspector} from './packages/workshop-visuals/components/agent_trace.v1/AgentTraceInspector';
      const projection={trace_id:'fixture',lanes:[{lane_id:'a',actor_id:'a',display_name:'Agent A'},{lane_id:'b',actor_id:'b',display_name:'Agent B'}],items:[
        {item_id:'message',kind:'model_call.completed',actor_id:'a',title:'Recorded answer',detail:{text:'Answer'}},
        {item_id:'citation',kind:'evidence.annotation',actor_id:'b',title:'Citation target',detail:{text:'Exact retained annotation'}}
      ]};
      function App(){const [selection,setSelection]=useState();return <><button onClick={()=>setSelection({itemId:'citation',revision:1})}>Jump to citation</button><AgentTraceInspector projection={projection} annotations={[]} selection={selection}/></>}
      createRoot(document.getElementById('root')).render(<App/>);
    `}
  });
  const server=createServer((req,res)=>{res.setHeader('Content-Type',req.url==='/bundle.js'?'text/javascript':'text/html');res.end(req.url==='/bundle.js'?result.outputFiles[0].text:'<div id="root"></div><script src="/bundle.js"></script>');});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const browser=await chromium.launch({headless:true});
  try {
    const page=await browser.newPage();
    await page.route('**/*',route=>new URL(route.request().url()).hostname==='127.0.0.1'?route.continue():route.abort());
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    assert.equal(await page.getByLabel('Compare agent').isDisabled(),true);
    assert.equal(await page.getByRole('complementary',{name:'Trace annotations'}).count(),0);
    await page.getByLabel('Trace agent',{exact:true}).selectOption('a');
    assert.equal(await page.getByLabel('Compare agent').isDisabled(),false);
    await page.getByLabel('Search agent trace').fill('no match');
    await page.getByRole('button',{name:'Jump to citation',exact:true}).click();
    const target=page.locator('[data-trace-item-id="citation"]');
    await target.waitFor();
    assert.equal(await target.getAttribute('aria-current'),'true');
    assert.equal(await page.getByLabel('Search agent trace').inputValue(),'');
    assert.equal(await page.getByLabel('Trace agent',{exact:true}).inputValue(),'all');
    for(const width of [960,1280,1440]) {
      await page.setViewportSize({width,height:840});
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
    }
  } finally { await browser.close(); await new Promise(resolve=>server.close(resolve)); }
});
