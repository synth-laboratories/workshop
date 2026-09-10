import assert from 'node:assert/strict';
import test from 'node:test';
import { decisionGroups, eventActor, eventTime } from '../components/agent_trace.v1/model.ts';

test('interleaved decisions with identical native IDs remain isolated by actor and session', () => {
  const item = (id, actor, session, kind) => ({item_id:id, actor_id:actor, session_id:session, kind, detail:{decision_id:'0'}});
  const rows = [item('a-start','a','s1','model_call.started'), item('b-start','b','s2','model_call.started'), item('b-end','b','s2','model_call.completed'), item('a-end','a','s1','model_call.completed'), item('retry','a','s3','model_call.started')];
  assert.deepEqual(decisionGroups(rows).map(g => g.items.map(i => i.item_id)), [['a-start','a-end'],['b-start','b-end'],['retry']]);
});
test('evidence selection resolves its target actor and execution time without guessing from production time', () => {
  const target = {item_id:'step',kind:'tool.result',actor_id:'a',detail:{elapsed_ms:1200}};
  const annotation = {item_id:'review',kind:'evidence.annotation',source_selector:{entity_id:'step'},occurred_at:'2026-09-07T20:00:00Z'};
  assert.equal(eventActor(annotation,[target]),'a');
  assert.equal(eventTime(annotation,[target]),1200);
  assert.equal(eventTime(annotation,[]),null);
});
