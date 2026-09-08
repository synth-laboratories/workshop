import assert from 'node:assert/strict';
import test from 'node:test';
import { annotationMatches, traceAnnotations } from '../components/agent_trace.v1/annotations.ts';

const target = { trace_id: 'trace-1', trace_digest: 'sealed-1', kind: 'event', entity_id: 'event-1', json_pointer: '/result', range: { start: 0, end: 4 } };
const item = { item_id: 'projection-row', kind: 'tool.completed', source_selector: { ...target, json_pointer: undefined, range: undefined } };
const trace = { trace_id: 'trace-1', trace_digest: 'sealed-1', items: [item] };
const annotation = { id: 'note-1', target, body: 'Collision', labels: ['coordination'], author: 'human', reviewState: 'pending', evidence: [] };

test('field/range annotation attaches to its enclosing source item without losing precision', () => {
  assert.equal(annotationMatches(annotation, item, trace), true);
  assert.deepEqual(annotation.target, target);
  assert.equal(annotationMatches(annotation, { ...item, item_id: 'another-renderer-row' }, trace), true);
});
test('same entity ID in a different trace, revision or entity kind cannot receive the note', () => {
  for (const override of [{ trace_id: 'trace-2' }, { trace_digest: 'sealed-2' }, { kind: 'span' }, { entity_id: 'event-2' }]) {
    assert.equal(annotationMatches({ ...annotation, target: { ...target, ...override } }, item, trace), false);
  }
  assert.equal(annotationMatches({ ...annotation, target: { entity_id: 'event-1' } }, item, trace), false);
});
test('projected annotation retains target, evidence and review provenance', () => {
  const notes = traceAnnotations([{ item_id: 'note-1', kind: 'evidence.annotation', source_selector: target, detail: { rationale: 'Collision', labels: ['coordination'], review_state: 'accepted', author_kind: 'human', evidence: [target], supersedes_id: 'note-0' } }]);
  assert.equal(notes[0].reviewState, 'accepted');
  assert.equal(notes[0].supersedesId, 'note-0');
  assert.deepEqual(notes[0].evidence, [target]);
  assert.deepEqual(notes[0].target, target);
  assert.deepEqual(traceAnnotations([{ item_id: 'reward-1', kind: 'evidence.reward' }]), []);
});
