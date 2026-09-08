#!/usr/bin/env python3
"""Optional grouped/paired query visuals over actual native eval results."""
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
base={'schemaVersion':'synth.trace-query.v2','evalJobIds':jobs,'grain':'episodes','limit':100}
original=json.loads((ROOT/'native-snapshot.json').read_text())
saved=call('/v1/traces/page',{'snapshot_id':original['snapshotId'],'offset':0,'limit':100})
assert saved['resultDigest']==original['resultDigest']
assert all(row['analysisState']=='not_requested' for row in saved['rows'])
findings=call('/v1/traces/query',{'query':{**base,'grain':'annotations','where':[{'field':'label','op':'contains','value':'trace.recorded'}]}})
assert findings['resultCount']==2
source=call('/v1/traces/source',{'snapshot_id':findings['snapshotId'],'result_id':findings['resultIds'][0],'source_limit':512})
assert source['resolved']
grouped=call('/v1/traces/query',{'query':{**base,'aggregate':'reward','groupBy':['jobId','environment']}})
assert grouped['resultCount']==2
assert all(row['measuredCount']==2 and row['missingCount']==0 for row in grouped['rows'])
paired=call('/v1/traces/query',{'query':{**base,'aggregate':'paired_reward'}})
assert paired['resultCount']==4
assert all(row['matchStatus'] in ('unmatched','unknown_reward_semantics') and row.get('rewardDelta') is None for row in paired['rows'])
# Distinct environments and unknown definitions must remain explicit; never
# invent aligned seeds or a comparable reward definition across environments.
visual=call('/v1/traces/open_query',{'snapshot_id':paired['snapshotId']})
call('/v1/review-window/capture',{'visualId':visual['visualId'],'width':1200,'height':800,'outputPath':str(ROOT/'native-store/native-paired-comparison.png')})
report={'status':'passed','jobs':jobs,'grouped':grouped,'paired':paired,'visualId':visual['visualId'],
        'oldSnapshotUnchanged':True,'annotationSourceResolved':True,'providerCalls':0,
        'scope':'real grouped rewards and explicit non-comparable pairs; matched reward pairs covered separately'}
(ROOT/'native-comparison-acceptance.json').write_text(json.dumps(report,indent=2))
print('Native grouped rewards, honest unmatched pairs, optional visual, and annotation source passed')
