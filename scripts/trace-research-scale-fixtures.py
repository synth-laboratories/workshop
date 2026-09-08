#!/usr/bin/env python3
"""Explicit synthetic retained V5 load fixtures: 450 traces and a 10k-event trace."""
import json,pathlib,sys,time,resource
ROOT=pathlib.Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT.parent/'containers/src'))
from synth_containers.event_log import RolloutEventLog
from synth_containers.platform.targets import HARBOR_PUBLIC
from synth_containers.platform.trace_bundle import materialize_harbor_trace_bundle
OUT=ROOT/'artifacts/trace-research-e2e/scale';OUT.mkdir(parents=True,exist_ok=True)
started=time.monotonic();rows=[];archives=[]
for i in range(450):
 rid=f'synthetic-retained-{i}';log=RolloutEventLog(rid,f'stream:{rid}')
 for n in range(10000 if i==0 else 1):log.append('action',{'agent_id':f'actor-{n%4}','action':'fixture_move','step':n})
 log.append('reward_signal',{'value':float(i%2),'authority':'environment'});log.append('status',{'status':'completed'});log.mark_closed()
 bundle=materialize_harbor_trace_bundle(output_path=OUT/f'{rid}.zip',spec=HARBOR_PUBLIC,log=log,seal={'content_digest':'synthetic-load-fixture'},pin={'task_instance_id':f'seed:{i}'},status='completed')
 archives.append(str(bundle.archive_path));rows.append({'seed':i,'rolloutId':rid,'result':{'status':'completed','task_instance_id':f'seed:{i}','reward':float(i%2),'trace':{'bundle_trace_digest':bundle.trace_digest}}})
(OUT/'engines.json').write_text(json.dumps([{'environment':'synthetic-load','info':{'fixture':True},'rollouts':rows,'archives':archives}]))
(OUT/'generation.json').write_text(json.dumps({'fixture':True,'traces':450,'longTraceEvents':10002,'longTraceActionActors':4,'seconds':time.monotonic()-started,'peakRssBytes':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss},indent=2))
print(OUT/'engines.json')
