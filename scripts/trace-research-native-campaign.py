#!/usr/bin/env python3
"""Existing native eval jobs -> selected ordinary campaigns -> reward query.

Uses the running isolated app's production IPC, never writes evaluation or
annotation rows. The deterministic annotator is installed by the engine harness.
"""
import json,time,urllib.request,urllib.error
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]/'artifacts/trace-research-e2e/native-launch'
cfg=json.loads((ROOT/'native-store/visuals-ipc.json').read_text())
def call(path,body):
    request=urllib.request.Request(cfg['url']+path,data=json.dumps(body).encode(),headers={'Authorization':'Bearer '+cfg['token'],'Content-Type':'application/json'})
    try:
        with urllib.request.urlopen(request,timeout=120) as response:return json.load(response)
    except urllib.error.HTTPError as error:raise RuntimeError(error.read().decode()) from None
jobs=json.loads((ROOT/'native-launch-acceptance.json').read_text())['jobs']
query={'schemaVersion':'synth.trace-query.v2','evalJobIds':jobs,'grain':'episodes','limit':100}
before=call('/v1/traces/query',{'query':query})
rows=before['rows'];assert len(rows)==4
selected=[next(row for row in rows if row['environment']==environment and row['seed']==0) for environment in ('craftax','dungeongrid')]
campaigns=[]
for row in selected:
    body={'container_id':row['containerId'],'traces':[{'kind':'trace_v5','id':row['traceId'],'digest':row['traceDigest']}],
          'annotators':['research.recorded-event.v1'],'label':'Native independent rollout acceptance'}
    estimate=call('/v1/annotations/annotation_campaign',{**body,'estimate_only':True})
    assert estimate['estimate']['paid_new']==0 and estimate['estimate']['job_count']==1, estimate
    campaign=call('/v1/annotations/annotation_campaign',body)
    assert not campaign['refused'] and len(campaign['jobs'])==1
    deadline=time.monotonic()+60
    while True:
        job=call('/v1/annotations/annotation_get',{'container_id':row['containerId'],'job_id':campaign['jobs'][0]})
        state=job.get('job',job).get('state')
        if state in ('sealed','failed','cancelled'):break
        assert time.monotonic()<deadline,'ordinary campaign did not settle'
        time.sleep(.25)
    assert state=='sealed',job
    campaigns.append(campaign)
filtered={**query,'annotationWhere':[{'field':'label','op':'contains','value':'trace.recorded'}]}
deadline=time.monotonic()+90
while True:
    after=call('/v1/traces/query',{'query':filtered})
    if after['resultCount']==2:break
    assert time.monotonic()<deadline,'native campaign reconciliation did not project findings'
    time.sleep(2)
assert {row['traceDigest'] for row in after['rows']}=={row['traceDigest'] for row in selected}
reward_before={row['traceDigest']:row['reward'] for row in rows}
assert all(row['reward']==reward_before[row['traceDigest']] for row in after['rows'])
report={'status':'passed','launch':'native annotation_campaign IPC','campaigns':campaigns,'jobs':jobs,'selectedRollouts':2,
        'unselectedRollouts':2,'providerCalls':0,'jesterky':False,'rewardsUnchanged':True,'snapshotId':after['snapshotId']}
(ROOT/'native-campaign-acceptance.json').write_text(json.dumps(report,indent=2))
print('Native ordinary campaigns and automatic annotation/reward projection passed')
