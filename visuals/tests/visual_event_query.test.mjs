import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {InMemoryCorpus} from '@synth/visuals-sdk';
const fixture=JSON.parse(readFileSync(new URL('./fixtures/visual-event-query.json',import.meta.url),'utf8'));
test('event occurrence, explicit logical order, and relation predicates share native conformance fixtures',()=>{
 const corpus=new InMemoryCorpus({id:'events',schema:'test.v1',rows:fixture.rows});
 for(const {where,ids} of fixture.cases){
  assert.deepEqual(corpus.query({schemaVersion:'synth.visuals-core.v1',where}).rows.map(row=>row.id),ids);
 }
});

test('aggregate ties use portable typed scalar order, including Unicode and missing values',()=>{
 const data=JSON.parse(readFileSync(new URL('./fixtures/visual-aggregate-order.json',import.meta.url),'utf8'));
 const corpus=new InMemoryCorpus({id:'ordering',schema:'test.v1',rows:data.rows});
 const cohort=corpus.cohort('All',{schemaVersion:'synth.visuals-core.v1',where:{op:'all'}});
 assert.deepEqual(corpus.aggregate(cohort,'value').buckets.map(bucket=>bucket.value===undefined?{missing:true}:bucket.value),data.values);
});
