#!/usr/bin/env python3
"""Real RuneBench prompt/protocol comparison, using Workshop-issued provider routes."""
import concurrent.futures,json,pathlib,sys,time,urllib.request,os
ROOT=pathlib.Path(__file__).resolve().parents[1]
OUT=ROOT/'artifacts/trace-research-e2e/runebench';OUT.mkdir(parents=True,exist_ok=True)
leases=json.loads(pathlib.Path(sys.argv[1]).read_text());BASE=os.environ.get('SYNTH_RESEARCH_RUNEBENCH_URL','http://127.0.0.1:18104')
def call(path,body=None,method=None):
 req=urllib.request.Request(BASE+path,data=json.dumps(body).encode() if body is not None else None,headers={'Content-Type':'application/json'},method=method)
 with urllib.request.urlopen(req,timeout=1800) as r:return json.load(r)
def run(arm):
 lease=leases[arm];rid=f'research-runebench-{arm}-{time.time_ns()}'
 config=rid+'-policy'
 bundle=call('/policy-bundles/'+rid,{'prompt':{'mode':'append','content':('Prioritize measurable woodcutting XP. Report the last observed XP and your next action.' if arm==0 else 'Prioritize measurable woodcutting XP. Before each action, record a compact message with fields observation, action, expected_result; after it, acknowledge the observed result.')}},'PUT')
 call('/policy-configs',{'config_id':config,'harness':'harbor_fused','config':{'base_url':lease['containerBaseUrl']}})
 prepared=call('/rollouts/prepare',{'rollout_id':rid,'task_instance_id':'seed:780039','model':'openai/gpt-5.6-luna','reasoning_effort':'low','policy_bundle_ref':bundle['digest'],'limits':{'maximumStepsPerRollout':24}})
 (OUT/f'{rid}.prepared.json').write_text(json.dumps(prepared,indent=2))
 result=call('/rollouts',{'rollout_id':rid,'policy_ref':{'harness':'harbor_fused','config':config}})
 (OUT/f'{rid}.result.json').write_text(json.dumps(result,indent=2))
 print(rid,result.get('status'),flush=True)
 with urllib.request.urlopen(BASE+f'/rollouts/{rid}/trace/bundle',timeout=60) as response:(OUT/f'{rid}.zip').write_bytes(response.read())
 return {'arm':arm,'rolloutId':rid,'result':result,'bundle':bundle,'archive':str(OUT/f'{rid}.zip')}
rows=[]
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
 futures={pool.submit(run,arm):arm for arm in (0,1)}
 for future in concurrent.futures.as_completed(futures):
  arm=futures[future]
  try:rows.append(future.result())
  except Exception as error:rows.append({'arm':arm,'error':str(error)})
  (OUT/'receipt.json').write_text(json.dumps(sorted(rows,key=lambda row:row['arm']),indent=2))
failed=[row['arm'] for row in rows if row.get('result',{}).get('status') != 'completed']
if failed:raise RuntimeError('RuneBench arms did not complete: '+str(failed))
