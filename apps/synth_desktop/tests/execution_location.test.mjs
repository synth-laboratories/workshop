import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import { transformSync } from 'esbuild';
const root=join(dirname(fileURLToPath(import.meta.url)),'..');
const output=join(root,'node_modules/.cache/synth-desktop-tests/execution-location.mjs');
mkdirSync(dirname(output),{recursive:true});
writeFileSync(output,transformSync(readFileSync(join(root,'src/renderer/src/runtime/executionLocation.ts'),'utf8'),{loader:'ts',format:'esm',target:'es2022'}).code);
const {executionLocationPresentation:present}=await import(pathToFileURL(output));
const scope={generation:0,availability:'qualification_required'};
test('hosted providers stay Local and signed-out state never disables Local execution',()=>{
 for (const selectedTargetId of ['synth-cloud-gpt-5','openrouter-model','chatgpt-codex','local-laguna']) {
  const view=present({selectedTargetId,scope,creationTransportReady:false});
  assert.equal(view.location,'local'); assert.equal(view.cloudCreationEnabled,false);
 }
 assert.equal(present({selectedTargetId:'intern-sync',sessionKind:'codex',scope,creationTransportReady:false}).location,'local');
});
test('actor ownership overrides provider and Cloud needs both identity and transport admission',()=>{
 assert.equal(present({selectedTargetId:'local-laguna',sessionKind:'intern',scope,creationTransportReady:false}).location,'cloud');
 assert.equal(present({selectedTargetId:'local-laguna',sessionKind:'other',scope,creationTransportReady:false}).location,'unknown');
 for (const availability of ['signed_out','qualification_required','ready']) {
  assert.equal(present({selectedTargetId:'intern-sync',scope:{...scope,availability},creationTransportReady:false}).cloudCreationEnabled,false);
 }
 assert.equal(present({selectedTargetId:'intern-sync',scope:{...scope,availability:'ready'},creationTransportReady:true}).cloudCreationEnabled,true);
});
