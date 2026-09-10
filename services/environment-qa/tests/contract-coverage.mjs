import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('../web/app.js', import.meta.url), 'utf8');
const escaping = source.slice(source.indexOf('const escapeHtml ='), source.indexOf('async function api'));
const rendering = source.slice(source.indexOf('function renderContractCoverage'), source.indexOf('function renderDag'));
const context = vm.createContext({});
vm.runInContext(escaping + rendering, context);
const render = run => context.renderContractCoverage(run);
assert.equal(render({ evidence: [] }), '');
const run = { evidence: [
  { gate: 'dependency-contracts', result: { contracts: [
    { package: '<script>bad</script>', contract: 'real hook executes' },
    { package: 'API', contract: 'required key exists' },
    { package: 'third check', contract: 'selected input accepted' }
  ] } },
  { gate: 'contract-analysis', result: { coverage: {
    0: { status: 'not_checked', reason: 'hook not registered' },
    1: { status: 'violated', reason: 'key missing' }
  } } }
] };
const html = render(run);
assert.ok(html.includes('1/3 assessed from observations · 2 untested'));
assert.ok(html.includes('hook not registered'));
assert.ok(html.includes('No completed observation assessment.'));
assert.ok(html.includes('&lt;script&gt;'));
assert.ok(!html.includes('<script>'));
assert.ok(html.includes('not complete task validation'));
console.log('Contract coverage rendering: passed');
