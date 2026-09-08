#!/usr/bin/env python3
"""Run actual local gold engines through the Containers rollout/capture API."""
import concurrent.futures, json, os, pathlib, socket, subprocess, sys, threading, time, urllib.request
ROOT=pathlib.Path(__file__).resolve().parents[1];REPOS=ROOT.parent
OUT=pathlib.Path(os.environ.get('TRACE_RESEARCH_OUTPUT',str(ROOT/'artifacts/trace-research-e2e')));OUT.mkdir(exist_ok=True,parents=True)
for path in [REPOS/'containers/src',REPOS/'evals',REPOS/'evals/containers/images/craftax-gamebench-rust',REPOS/'evals/containers/images/dungeongrid-gold']:
 sys.path.insert(0,str(path))
os.environ['SYNTH_CRAFTAX_URL']='http://127.0.0.1:18188';os.environ['SYNTH_CRAFTAX_MAX_STEPS']='12'
os.environ['SYNTH_DUNGEONGRID_URL']='http://127.0.0.1:8792';os.environ['SYNTH_DUNGEONGRID_MAX_STEPS']='12'
os.environ['SYNTH_ANNOTATION']='off'
os.environ['SYNTH_DUNGEONGRID_CISPO']='off'
os.environ['SYNTH_DUNGEONGRID_SCENARIOS']=str(REPOS/'gamebench/tasks/dungeongrid-singleplayer/defaults/scenarios')
import uvicorn
from synth_containers.platform import create_compat_app
from craftax_gold.targets import CRAFTAX_CODE_POLICY
from dungeongrid_gold.targets import DUNGEONGRID_CODE_POLICY

def call(base,path,body=None):
 req=urllib.request.Request(base+path,data=json.dumps(body).encode() if body is not None else None,headers={'Content-Type':'application/json'})
 with urllib.request.urlopen(req,timeout=180) as r:return json.load(r)

def ready(base):
 for _ in range(100):
  try:call(base,'/health');return
  except Exception as error:
   last=error.read().decode() if hasattr(error,'read') else str(error);time.sleep(.1)
 raise RuntimeError(f'{base} did not become healthy: {last}')

def run_target(name,target):
 root=OUT/name;root.mkdir(exist_ok=True)
 app=create_compat_app(target,storage_root=root/'storage')
 sock=socket.socket();sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 server=uvicorn.Server(uvicorn.Config(app,log_level='error'))
 thread=threading.Thread(target=lambda:server.run(sockets=[sock]),daemon=True);thread.start();base=f'http://127.0.0.1:{port}';ready(base)
 info=call(base,'/info');results=[]
 try:
  for seed in [0,1]:
   rid=f'research-{name}-{seed}-{time.time_ns()}'
   body={'rollout_id':rid,'task_instance_id':f'seed:{seed}','telemetry':{'enabled':True,'transport':'sse','retention':'run'}}
   prepared=call(base,'/rollouts/prepare',body)
   result=call(base,'/rollouts',{**body,'slot':'stream','submission_mode':'sync','policy_ref':{'harness':'isolated_policy_process','config':None}})
   results.append({'seed':seed,'rolloutId':rid,'result':result,'prepared':prepared})
   print(name,seed,result.get('status'),flush=True)
  archives=sorted(str(p) for p in (root/'storage').rglob('*.zip'))
  receipt={'environment':name,'info':info,'rollouts':results,'archives':archives,'bundleErrors':app.state.platform.trace_bundle_errors}
  (root/'receipt.json').write_text(json.dumps(receipt,indent=2));assert archives,receipt['bundleErrors']
  assert all(x['result']['status']=='completed' for x in results),results
  return receipt
 finally:server.should_exit=True;thread.join(5)

if __name__=='__main__':
 log=(OUT/'craftax-engine.log').open('w')
 engine=subprocess.Popen([str(REPOS/'gamebench/tasks/craftax-singleplayer/gold_rust/target/release/craftax_gold'),'--host','127.0.0.1','--port','18188'],cwd=REPOS/'gamebench',stdout=log,stderr=log)
 try:
  ready(os.environ['SYNTH_CRAFTAX_URL']);ready(os.environ['SYNTH_DUNGEONGRID_URL'])
  with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
   receipts=list(pool.map(lambda x:run_target(*x),[('craftax',CRAFTAX_CODE_POLICY),('dungeongrid',DUNGEONGRID_CODE_POLICY)]))
  (OUT/'engines.json').write_text(json.dumps(receipts,indent=2))
 finally:engine.terminate();engine.wait(timeout=10);log.close()
