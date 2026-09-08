#!/usr/bin/env python3
"""Compare actual native jobs using declared rewards and pinned environments."""
import json,urllib.request,urllib.error,math
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]/'artifacts/trace-research-e2e/native-launch'
cfg=json.loads((ROOT/'native-store/visuals-ipc.json').read_text())
def call(path,body):
    request=urllib.request.Request(cfg['url']+path,data=json.dumps(body).encode(),headers={'Authorization':'Bearer '+cfg['token'],'Content-Type':'application/json'})
    try:
        with urllib.request.urlopen(request,timeout=120) as response:return json.load(response)
    except urllib.error.HTTPError as error:raise RuntimeError(error.read().decode()) from None
arms=[json.loads((ROOT/f'typed-reward-arm-{a}/native-snapshot.json').read_text()) for a in ['a','b']]
reports=[]
for family in ['craftax','dungeongrid']:
    jobs=[next(row['jobId'] for row in arm['facets']['rows'] if row['environment']==family) for arm in arms]
    base={'schemaVersion':'synth.trace-query.v2','evalJobIds':jobs,'grain':'episodes','limit':100}
    episodes=call('/v1/traces/query',{'query':base})
    assert episodes['resultCount']==4
    rows=episodes['rows']
    assert all(row['definitionDigest'].startswith('sha256:') and row['environmentVersion'].startswith('sha256:') and row['units']==family+'_reward' for row in rows)
    assert len({row['environmentVersion'] for row in rows})==1
    paired=call('/v1/traces/query',{'query':{**base,'aggregate':'paired_reward'}})
    assert paired['resultCount']==2
    for row in paired['rows']:
        expected=[next(r['reward'] for r in rows if r['jobId']==job and r['seed']==row['seed']) for job in jobs]
        assert row['matchStatus']=='matched' and row['rewards']==expected
        assert math.isclose(row['rewardDelta'],expected[1]-expected[0],abs_tol=1e-12)
    aggregate=call('/v1/traces/query',{'query':{**base,'aggregate':'reward'}})
    assert aggregate['resultCount']==1 and aggregate['rows'][0]['measuredCount']==4 and aggregate['rows'][0]['missingCount']==0
    rewards=call('/v1/traces/query',{'query':{**base,'grain':'rewards'}})
    assert rewards['resultCount']==4 and sorted(r['reward'] for r in rewards['rows'])==sorted(r['reward'] for r in rows)
    source=call('/v1/traces/source',{'snapshot_id':episodes['snapshotId'],'result_id':episodes['resultIds'][0],'source_limit':512})
    assert source['resolved']
    visual=call('/v1/traces/open_query',{'snapshot_id':paired['snapshotId']})
    call('/v1/review-window/capture',{'visualId':visual['visualId'],'width':1200,'height':800,'outputPath':str(ROOT/f'native-store/{family}-typed-pairs.png')})
    reports.append({'environment':family,'jobs':jobs,'episodes':episodes,'paired':paired,'aggregate':aggregate,'sourceResolved':True,'visualId':visual['visualId']})
(ROOT/'typed-pair-acceptance.json').write_text(json.dumps({'status':'passed','nativeJobs':4,'rollouts':8,'pairs':4,'providerCalls':0,'reports':reports},indent=2))
print('Four actual typed native pairs, complete denominators, rewards and source reads passed')
