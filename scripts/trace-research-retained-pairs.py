#!/usr/bin/env python3
"""Verify typed pair alignment on retained real RuneBench facade executions.

Uses the actual facade rollout IDs as its direct execution identities; this is
the Containers query boundary, not an injected native Optimizers launch test.
"""
import argparse,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT.parent/'containers/src'))
from synth_containers.tracing.research import ResearchIndex
root=ROOT/'artifacts/trace-research-e2e/runebench'
parser=argparse.ArgumentParser()
parser.add_argument('--completed', action='store_true', help='Validate the final two completed live arms')
args=parser.parse_args()
rows=json.loads((root/('receipt.json' if args.completed else 'corrected-units-receipt.json')).read_text())
episodes=[]
for row in rows:
    result=row['result'];task=result['task_instance_id']
    episodes.append({'jobId':row['rolloutId'],'trialId':task,'archivePath':row['archive'],
        'traceDigest':result['trace']['trace_digest'],'reward':result['reward']['reward'],
        'taskId':task,'seed':None,'repeat':int(task.rsplit('-',1)[1]),'environment':'runebench',
        'status':result['status'],'valid':result['status']=='completed'})
index=ResearchIndex(root/('final-paired-query.sqlite' if args.completed else 'paired-query.sqlite'))
query={'schemaVersion':'synth.trace-query.v2','evalJobIds':[e['jobId'] for e in episodes],'aggregate':'paired_reward'}
out=index.execute(query,episodes);index.close()
assert out['resultCount']==1,out['facets']['rows']
pair=out['facets']['rows'][0]
assert pair['matchStatus']=='matched' and pair['definitionDigest'] and pair['units']=='normalized_xp_per_minute'
if args.completed:
    assert all(e['valid'] for e in episodes), 'both final arms must complete'
    assert isinstance(pair['rewardDelta'], (int,float)), pair
    assert abs(pair['rewardDelta']) == abs(episodes[1]['reward']-episodes[0]['reward']), pair
else:
    assert pair['rewardDelta'] is None,'failed execution must not silently become a valid comparison arm'
(root/('final-paired-query.json' if args.completed else 'retained-paired-query.json')).write_text(json.dumps(out,indent=2))
print('Real typed reward definitions align; completed delta verified' if args.completed else 'Real typed reward definitions align; failed arm remains invalid and delta unavailable')
