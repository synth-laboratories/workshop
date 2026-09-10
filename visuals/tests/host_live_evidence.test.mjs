import assert from "node:assert/strict";
import test from "node:test";
import { acceptHostEvidence } from "../runtime/hostLiveEvidence.ts";

const streams = [{ streamId: "a", pollUrl: "/a" }, { streamId: "b", pollUrl: "/b" }];
const identity = { visualId: "visual", revision: 2 };
function page(counts, reward = null) {
  return { events: [], cursor: {next: 0, hasMore: false, closed: true},
    projection: {schema_version:"synth.live-eval-projection.v1", kinds:[], event_count:counts.reduce((a,b)=>a+b),
      reward, usage:null, has_live_frames:false, has_reward_txt:false},
    receipt: {schemaVersion:"synth.visual-stream-receipt.v1", ...identity, ready:true,
      declaredStreamCount:2, respondingStreamCount:counts.filter(Boolean).length,
      recovered:counts.reduce((a,b)=>a+b), gaps:[], conflicts:[], streamsMissingTransport:[],
      streams:streams.map((stream,i)=>({streamId:stream.streamId,declaredSource:stream.pollUrl,pollResponses:counts[i]}))}};
}
test("whole-visual host snapshots never sum or roll back on response reordering", () => {
  const newer = acceptHostEvidence(undefined, page([1,1], .75), streams, identity);
  assert.equal(newer.ready, true);
  assert.equal(newer.recovered, 2);
  assert.equal(acceptHostEvidence(newer, page([1,0], .1), streams, identity), newer);
  assert.equal(acceptHostEvidence(newer, page([1,1], .2), streams, identity), newer);
  assert.equal(acceptHostEvidence(newer, page([2,1], null), streams, identity).projection.reward, null);
});
test("partial streams, gaps, conflicts and truncation do not claim readiness", () => {
  assert.equal(acceptHostEvidence(undefined,page([1,0]),streams,identity).ready,false);
  for (const issue of ["gaps","conflicts"]) {
    const input=page([1,1]); input.receipt[issue]=[{}];
    const result=acceptHostEvidence(undefined,input,streams,identity);
    assert.equal(result.ready,false); assert.ok(result.error);
  }
  const input=page([1,1]); input.evidenceTruncated=true;
  const result=acceptHostEvidence(undefined,input,streams,identity);
  assert.equal(result.ready,false); assert.equal(result.truncated,true);
  assert.match(result.error,/retained prefix/);
});
test("receipt identity and transport changes refuse; browser pages have no host claims", () => {
  assert.throws(()=>acceptHostEvidence(undefined,page([1,1]),streams,{...identity,revision:3}),/revision/);
  assert.throws(()=>acceptHostEvidence(undefined,page([1,1]),[{...streams[0],pollUrl:"/changed"},streams[1]],identity),/transports/);
  assert.equal(acceptHostEvidence(undefined,{events:[],cursor:{}},streams,identity),undefined);
  const prior = acceptHostEvidence(undefined,page([1,1]),streams,identity);
  assert.throws(()=>acceptHostEvidence(prior,{events:[],cursor:{}},streams,identity),/stopped supplying/);
});
