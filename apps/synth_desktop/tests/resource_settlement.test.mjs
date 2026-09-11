import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import { transformSync } from 'esbuild';
const root=join(dirname(fileURLToPath(import.meta.url)),'..');
const output=join(root,'node_modules/.cache/synth-desktop-tests/resource-settlement.mjs');
mkdirSync(dirname(output),{recursive:true});
writeFileSync(output,transformSync(readFileSync(join(root,'src/renderer/src/runtime/resourceSettlement.ts'),'utf8'),{loader:'ts',format:'esm',target:'es2022'}).code);
const {resourceSettlementPresentation:present}=await import(pathToFileURL(output));
const {examples}=JSON.parse(readFileSync(join(root,'../../crates/synth-api-client/tests/fixtures/run_resource_settlement.json'),'utf8'));
test('source fixtures preserve untracked counts and incomplete global coverage',()=>{
 assert.equal(present('legacy-run',examples[0]).state,'untracked');
 assert.equal(present('legacy-run',examples[0]).pending,null);
 assert.equal(present('root-run',examples[1]).state,'partial');
});
test('root and owned-subtree confirmations stay distinct; contradictory snapshots refuse completion',()=>{
 const root={...examples[1],settled:true,coverage_complete:true};
 assert.equal(present('root-run',root).state,'settled_root');
 const child={...root,run_id:'child',scope_kind:'owned_subtree',edge_id:'edge'};
 assert.equal(present('child',child).state,'settled_subtree');
 assert.equal(present('root-run',child).state,'unavailable');
 assert.equal(present('root-run',{...root,coverage_complete:false}).state,'unavailable');
 assert.equal(present('root-run',{...root,unknown:1}).state,'unavailable');
});
test('missing evidence and uncertain cleanup do not imply zero resources',()=>{
 assert.equal(present('root-run',null).state,'unavailable');
 assert.equal(present('root-run',{...examples[1],unknown:1}).state,'unknown');
 assert.equal(present('root-run',{...examples[1],registered_tree_settled:false,pending:1}).state,'pending');
 assert.equal(present('root-run',{...examples[1],registered_tree_settled:false,pending:null,unknown:null}).state,'unknown');
});
