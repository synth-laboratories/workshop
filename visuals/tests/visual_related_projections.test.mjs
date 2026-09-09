import assert from 'node:assert/strict';
import test from 'node:test';
import {resolveSealedTrialProjections,resolveComparisonProjection} from '../../packages/workshop-visuals/runtime/relatedProjections.ts';
const terminal=(trial,digest)=>({type:'eval.trial.terminal',delta:{trial_id:trial},item:{raw:{sealedTrace:{inspectable:true,traces:[{digest}]}}}});
const result=digest=>({traceDigest:digest,projectionKind:'rollout-inspector',projectionSchema:'synth.trace-projection.rollout-inspector.v1',payload:{digest}});
test('sealed projections deduplicate references and bound read concurrency',async()=>{
 let pending=0,max=0;const calls=[];
 const events=[terminal('a','a'),terminal('a','a'),terminal('b','a'),...Array.from({length:10},(_,i)=>terminal('trial-'+i,'trace-'+i))];
 const rows=await resolveSealedTrialProjections(events,async digest=>{calls.push(digest);pending++;max=Math.max(max,pending);await new Promise(resolve=>setTimeout(resolve,1));pending--;return result(digest);},new AbortController().signal);
 assert.equal(rows.length,12);assert.equal(calls.length,11);assert.ok(max<=4);
 assert.deepEqual(rows[0].projection,rows[1].projection);
});
test('sealed projection identity drift and cancellation fail closed',async()=>{
 await assert.rejects(()=>resolveSealedTrialProjections([terminal('a','a')],async()=>result('wrong'),new AbortController().signal),/identity/);
 const controller=new AbortController();let calls=0;
 await assert.rejects(()=>resolveSealedTrialProjections([terminal('a','a')],async digest=>{calls++;controller.abort();return result(digest);},controller.signal),/cancelled/);
 assert.equal(calls,1);
});
test('optional comparison selects a deterministic sibling and rejects stale results',async()=>{
 const ports={list:async()=>[{id:'recipe_one_self'},{id:'recipe_one_b',createdAt:'2026-01-01'},{id:'recipe_one_a',createdAt:'2026-01-01'},{id:'other_run'}],view:async id=>({id})};
 assert.equal((await resolveComparisonProjection('recipe_one_self',ports,new AbortController().signal)).run.id,'recipe_one_a');
 const controller=new AbortController();
 await assert.rejects(()=>resolveComparisonProjection('recipe_one_self',{...ports,view:async id=>{controller.abort();return {id};}},controller.signal),/cancelled/);
});
