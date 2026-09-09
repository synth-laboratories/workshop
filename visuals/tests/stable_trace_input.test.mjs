import test from 'node:test';
import assert from 'node:assert/strict';
import {build} from 'esbuild';
import {chromium} from 'playwright';
import {fileURLToPath} from 'node:url';

test('unchanged trace inputs do not mutate; edits use the latest callback',async()=>{
 const bundle=await build({stdin:{contents:`import React from 'react';import {createRoot} from 'react-dom/client';import {flushSync} from 'react-dom';import {StableTraceInput} from './packages/workshop-visuals/families/first_class_example_containers/_shared/StableTraceInput';
 const root=createRoot(document.getElementById('root'));
 window.renderInput=(value,version)=>flushSync(()=>root.render(<StableTraceInput value={value} aria-label="Search" style={{width:100}} onChange={e=>window.received=[version,e.target.value]}/>));
 window.renderInput('before',1);window.changes=[];new MutationObserver(rows=>window.changes.push(...rows.map(r=>r.attributeName))).observe(document.getElementById('root'),{subtree:true,attributes:true,childList:true});`,resolveDir:fileURLToPath(new URL('../../',import.meta.url)),loader:'tsx'},bundle:true,write:false,format:'iife',jsx:'automatic'});
 const browser=await chromium.launch();
 try{const page=await browser.newPage();await page.setContent('<div id="root"></div>');await page.addScriptTag({content:bundle.outputFiles[0].text});
 await page.evaluate(()=>window.renderInput('before',2));
 assert.deepEqual(await page.evaluate(()=>window.changes),[]);
 await page.getByRole('textbox',{name:'Search'}).fill('edited');
 assert.deepEqual(await page.evaluate(()=>window.received),[2,'edited']);
 await page.evaluate(()=>window.renderInput('after',3));
 assert.equal(await page.getByRole('textbox',{name:'Search'}).inputValue(),'after');
 }finally{await browser.close();}
});
