import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { resourceSettlementPresentation as present } from '../src/renderer/src/runtime/resourceSettlement.ts';
const { examples } = JSON.parse(readFileSync(new URL('../../../crates/synth-api-client/tests/fixtures/run_resource_settlement.json', import.meta.url), 'utf8'));
const root = { ...examples[1], settled: true, coverage_complete: true, root_confirmed: true };
for (const [field, value] of [['unknown', null], ['root_confirmed', null], ['root_confirmed', false]]) {
  test(`settled root refuses ${field}=${value}`, () => {
    assert.equal(present('root-run', { ...root, [field]: value }).state, 'unavailable');
  });
}
test('settled subtree does not require ancestor termination', () => {
  const child = { ...root, run_id: 'child', scope_kind: 'owned_subtree', edge_id: 'child-edge', root_confirmed: false };
  assert.equal(present('child', child).state, 'settled_subtree');
  assert.equal(present('root-run', child).state, 'unavailable');
});

test('explicit root confirmation and original incomplete fixture remain distinct', () => {
  assert.equal(present('root-run', root).state, 'settled_root');
  assert.equal(present('root-run', examples[1]).state, 'partial');
  assert.equal(present('legacy-run', examples[0]).state, 'untracked');
});
