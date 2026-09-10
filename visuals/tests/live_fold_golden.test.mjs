import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import test from "node:test";
import {emptyLiveIngest, ingestLiveEnvelopeBatch} from "../runtime/liveStream.ts";
import {projectLiveEval} from "../runtime/liveEvalReducer.ts";

const root=resolve(import.meta.dirname,"../..");
const golden=JSON.parse(readFileSync(resolve(root,"visuals/fixtures/live_fold_golden.json"),"utf8"));
assert.equal(golden.schema,"synth.live-fold-golden.v1");
for(const expected of golden.cases) {
  const document=expected.source.file ? JSON.parse(readFileSync(resolve(root,expected.source.file),"utf8")) : expected.source.inline;
  const events=Array.isArray(document)?document:document.events;
  for(const chunked of [false,true]) test(`golden ${chunked?"paged":"batch"}: ${expected.name}`,()=>{
    let state=emptyLiveIngest();
    for(const batch of chunked?events.map(event=>[event]):[events]) state=ingestLiveEnvelopeBatch(state,batch);
    assert.equal(events.length,expected.deliveredCount);
    assert.equal(state.delivered,expected.deliveredCount);
    assert.deepEqual([...state.ids],expected.accepted.map(row=>row.identity));
    assert.equal(state.ids.size,expected.acceptedCount);
    assert.equal(state.events.length,expected.evidenceCount);
    assert.equal(state.ready,expected.ready);
    assert.deepEqual(state.gaps.sort((a,b)=>a.scope.localeCompare(b.scope)||a.after-b.after),expected.gaps);
    assert.deepEqual(state.conflicts,expected.conflicts);
    assert.deepEqual(Object.fromEntries(state.lastSequenceByScope),expected.lastSequenceByScope);
    const p=projectLiveEval(state.events);
    assert.deepEqual({kinds:p.kinds,hasLiveFrames:p.has_live_frames,hasRewardTxt:p.has_reward_txt,
      reward:p.reward,usage:p.usage,eventCount:p.events.length},expected.projection);
  });
}
