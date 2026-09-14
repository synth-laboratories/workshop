import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('../../../', import.meta.url));
test('QA launcher persists policy, refuses replacement and archives revocation', () => {
  fs.mkdirSync(path.join(root, 'work'), {recursive: true});
  const directory = fs.mkdtempSync(path.join(root, 'work/qa-policy-test-'));
  try {
    const source = path.join(directory, 'profile.json');
    const data = path.join(directory, 'data');
    fs.writeFileSync(source, JSON.stringify({schema_version:1, id:'qa-test', instance:'test',
      container_roots:[], recipe_roots:[], containers:[], recipes:[], providers:[],
      expires_at:new Date(Date.now()+3600000).toISOString(), max_request_usd_micros:100,
      max_total_usd_micros:200, max_rollouts:1}));
    const run = (op, instance='test') => spawnSync(process.execPath,
      [path.join(root,'scripts/qa-policy.mjs'),op,data,instance,source], {encoding:'utf8'});
    assert.notEqual(run('enable','wrong').status,0);
    assert.equal(run('enable').status,0);
    assert.notEqual(run('enable').status,0);
    assert.match(run('status').stdout,/qa-test/);
    assert.equal(run('disable').status,0);
    assert.match(run('status').stdout,/not installed/);
    assert.equal(fs.readdirSync(data).filter(n=>n.includes('.revoked-')).length,1);
  } finally {
    fs.rmSync(directory,{recursive:true,force:true});
  }
});
