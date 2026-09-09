import test from 'node:test';
import assert from 'node:assert/strict';
import {createFixtureIndex} from '../../packages/workshop-visuals/runtime/fixtureIndex.ts';
test('fixture paths never substitute another family sharing the same basename',()=>{
 const index=createFixtureIndex([['/packages/workshop-visuals/families/sft/examples/events.json',{algorithm:'sft'}],['/packages/workshop-visuals/families/gepa/examples/events.json',{algorithm:'gepa'}],['/packages/workshop-visuals/fixtures/unique.json',{x:1}]]);
 assert.deepEqual(index.load('families/sft/examples/events.json'),{algorithm:'sft'});
 assert.throws(()=>index.load('families/missing/examples/events.json'),/No packaged fixture/);
 assert.throws(()=>index.load('events.json'),/Ambiguous/);
 const copy=index.load('unique.json');copy.x=2;assert.equal(index.load('unique.json').x,1);
});
