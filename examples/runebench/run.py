#!/usr/bin/env python3
"""Bounded, concurrent MA runner. Policies receive only typed actor-local tools."""
import argparse, concurrent.futures, datetime as dt, fcntl, hashlib, json, os, socket, subprocess, threading, time, urllib.request, urllib.error
from pathlib import Path
HERE=Path(__file__).resolve().parent
OUT=HERE/'results'
BASE='http://127.0.0.1:8126'
MODEL='openai/gpt-5.6-luna'
LOCK=threading.Lock()

def write(p,data):
 p.write_text(json.dumps(data,indent=2))
def call(path,body=None,token=None,timeout=45):
 headers={'Content-Type':'application/json'}
 if token:headers['Authorization']='Bearer '+token
 r=urllib.request.Request(BASE+path,data=json.dumps(body).encode() if body is not None else None,headers=headers)
 with urllib.request.urlopen(r,timeout=timeout) as f:return json.load(f)
def compose(*args,env=None):
 subprocess.run(['docker','compose','-p','workshop-rune-'+hashlib.sha256(str(HERE).encode()).hexdigest()[:12],'-f',str(HERE/'compose.yaml'),*args],env=env,check=True,stdout=subprocess.DEVNULL)
def utc():return dt.datetime.now(dt.timezone.utc).isoformat()
def credential():
 # Project-local env only. No Keychain, shell expansion, or credential copying into Docker.
 for line in CREDENTIAL_FILE.read_text().splitlines():
  if line.strip().startswith('OPENROUTER_API_KEY='):return line.split('=',1)[1].strip().strip('"').strip("'")
 raise RuntimeError('Project-local OPENROUTER_API_KEY unavailable')
def reserve(batch, model):
 p=HERE/f'budget-{batch}.json'
 with p.open('a+') as f:
  fcntl.flock(f,fcntl.LOCK_EX);f.seek(0);s=f.read();b=json.loads(s) if s else {'maxUsd':5,'maxCalls':128,'reservedUsd':0,'calls':0,'actualUsd':0,'unknownCostCalls':0}
  # Bounded 20k-byte input and 8192 output; include cache-write pricing in reservations.
  amount=.20 if model.endswith('terra') else .02
  if b.get('unknownCostCalls',0) or b['calls']>=b['maxCalls'] or round(b['reservedUsd']+amount,8)>b['maxUsd']:raise RuntimeError('Aggregate budget exhausted')
  b['calls']+=1;b['reservedUsd']=round(b['reservedUsd']+amount,8);f.seek(0);f.truncate();json.dump(b,f);f.flush()
 return p

def settle(p,usage):
 with p.open('r+') as f:
  fcntl.flock(f,fcntl.LOCK_EX);b=json.load(f)
  if usage.get('cost') is None:b['unknownCostCalls']+=1
  else:b['actualUsd']+=float(usage['cost'])
  f.seek(0);f.truncate();json.dump(b,f);f.flush()

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--provider',choices=['openrouter']);ap.add_argument('--env-file',type=Path);ap.add_argument('--scripted',action='store_true');ap.add_argument('--duration',type=int,default=150);ap.add_argument('--calls',type=int,default=10);ap.add_argument('--batch',default='demo-20260907');ap.add_argument('--effort',choices=['low','medium','high'],default='low');ap.add_argument('--model',choices=['luna','terra'],default='luna');args=ap.parse_args();args.scripted = args.scripted or args.provider is None
 global CREDENTIAL_FILE
 CREDENTIAL_FILE=args.env_file
 if not args.scripted and CREDENTIAL_FILE is None:ap.error('--provider openrouter requires an explicit --env-file; never paste keys into chat')
 model='openai/gpt-5.6-'+args.model
 if not 10<=args.duration<=600 or not 1<=args.calls<=16:raise ValueError('Invalid duration or call cap')
 if not args.batch.replace('-','').isalnum():raise ValueError('Invalid batch')
 with socket.socket() as port_check:
  port_check.bind(('127.0.0.1',8126))
 stamp=dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')+('-scripted' if args.scripted else '-'+args.model+'-'+args.effort)
 run=OUT/stamp;job=run/'episode';job.mkdir(parents=True,exist_ok=False)
 scenario=json.loads((HERE/'scenario.json').read_text());scenario['duration_seconds']=args.duration
 kind='scripted' if args.scripted else 'provider-backed'
 scenario['evidence_kind']=kind
 manifest={'schema_version':'runebench.ma.manifest.v1','config':{'id':'runebench-ma','scenario':scenario,'evidence_kind':kind,'model':None if args.scripted else model,'reasoning_effort':args.effort,'max_completion_tokens':8192,'max_calls_per_actor':args.calls,'batch':args.batch},'head_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=HERE,text=True).strip(),'tree_clean':False,'source_sha256':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in HERE.iterdir() if p.suffix in ['.ts','.py','.json','.yaml'] and not p.name.startswith('budget-')},'evals_authority':{'head_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=HERE,text=True).strip(),'working_tree_tracked_clean':False},'created_at':utc(),'randomization':'Fixed starting saves; engine RNG is not seeded. Repeats are not paired-seed causal trials.'}
 write(run/'matrix_run_manifest.json',manifest)
 env=dict(os.environ,MA_ARTIFACTS=str(job),MA_RUN_ID=stamp,MA_EVIDENCE_KIND=kind)
 container_name='workshop-runebench-'+stamp
 seq=0;armed_at=None
 def event(kind,actor,payload):
  nonlocal seq
  with LOCK:
   seq+=1
   record={'schema_version':'evals.event-stream.v1','kind':'trace.raw','source':'local','occurred_at':utc(),'run_id':stamp,'sequence':str(seq),'actor_id':actor,'payload':{'kind':kind,'elapsedMs':(time.monotonic()-armed_at)*1000 if armed_at else 0,**payload}}
   with (run/'events.jsonl').open('a') as f:f.write(json.dumps(record)+'\n')
 key=None if args.scripted else credential()
 usages=[];errors=[];answered=set()
 def policy(actor,token):
  name=actor['id'];history=[]
  prompt=f'''You control {name}, {actor['role']} on team {actor['team']} in a local RuneScape benchmark. Your team competes to gain the most total Woodcutting XP before a hard wall deadline. All players start with level 10 Woodcutting and bronze axes. XP multiplier 1, server tick 300ms. Both lead and scout can chop: lead prioritize throughput; scout locate free trees, report useful locations to your teammate, then chop too. Communicate briefly in public chat when useful, but do not spend the whole race chatting. Only your observation is supplied; other players' private state is unavailable. Nearby normal Trees are legal; do not target oaks. Crowded trees may be felled by others. If inventory is full drop logs. A chop can continue automatically until a tree is depleted. Avoid needless interruptions. Return exactly one JSON object with type (say/chop/walk/drop/wait), optional text (max 80 chars), x,z (observed tree coordinates for chop or within 32 tiles for walk), slot (logs only for drop), and reason (one concise sentence). No code. Tools: chop attempts one normal tree and waits for a log; walk navigates; wait waits 2 game ticks; say sends public chat; drop discards one inventory slot of logs. Prefer a useful action now. Teammate roster: {json.dumps([a for a in scenario['actors'] if a['team']==actor['team']])}.'''
  for turn in range(args.calls):
   try:
    obs=call(f'/actors/{name}/observation',token=token)
    if obs['phase']!='running' or obs['remainingMs']<3000:break
    # Bound request size using a compact projection, retaining actor-local provenance.
    compact={**obs,'nearbyLocs':obs.get('nearbyLocs',[])[:24],'gameMessages':obs.get('gameMessages',[])[-10:]}
    decision_id=f'{name}-{turn}'
    if args.scripted:
     if turn==0:action={'type':'say','text':f'{actor["team"]} {actor["role"]} chopping nearby trees','reason':'Scripted communication probe'}
     else:action={'type':'chop','reason':'Scripted lifecycle probe'}
    else:
     messages=[{'role':'system','content':prompt},*history[-4:],{'role':'user','content':json.dumps(compact,separators=(',',':'))}]
     body={'model':model,'messages':messages,'max_completion_tokens':8192,'reasoning':{'effort':args.effort},'response_format':{'type':'json_object'},'provider':{'max_price':{'prompt':2 if args.model=='terra' else .2,'completion':12 if args.model=='terra' else 1.2},'allow_fallbacks':False}}
     data=json.dumps(body).encode()
     if len(data)>20000:raise RuntimeError('Request exceeds 20k-byte input bound')
     bp=reserve(args.batch,model)
     event('model.requested',name,{'decisionId':decision_id,'observation':compact,'inputBytes':len(data),'model':model,'reasoningEffort':args.effort,'messages':messages})
     req=urllib.request.Request('https://openrouter.ai/api/v1/chat/completions',data=data,headers={'Authorization':'Bearer '+key,'Content-Type':'application/json'})
     response=None
     try:
      with urllib.request.urlopen(req,timeout=min(65,max(1,obs['remainingMs']/1000))) as f:response=json.load(f)
      usage=response.get('usage',{});settle(bp,usage)
     except Exception:
      settle(bp,{})
      raise
     with LOCK:usages.append(usage);answered.add(name)
     message=response['choices'][0]['message'];content=message.get('content') or ''
     event('model.completed',name,{'decisionId':decision_id,'responseId':response.get('id'),'model':response.get('model'),'content':content,'reasoningEffort':args.effort,'reasoning':message.get('reasoning'),'reasoning_details':message.get('reasoning_details'),'usage':usage,'finishReason':response['choices'][0].get('finish_reason')})
     action=json.loads(content)
     history.extend([{'role':'user','content':f'Turn {turn} observation received.'},{'role':'assistant','content':content}])
    action['decisionId']=decision_id
    event('policy.action',name,{'decisionId':decision_id,'action':action,'evidence_kind':kind})
    result=call(f'/actors/{name}/action',action,token,timeout=40)
    if args.scripted:
     with LOCK:answered.add(name)
    event('policy.result',name,{'decisionId':decision_id,'result':result})
    history.append({'role':'user','content':'Action result: '+json.dumps(result)[:1500]})
    time.sleep(.3)
   except urllib.error.HTTPError as e:
    detail=e.read(1200).decode(errors='replace')
    if key:detail=detail.replace(key,'[redacted]')
    event('policy.error',name,{'turn':turn,'status':e.code,'detail':detail})
    if e.code==409:break
    errors.append({'actor':name,'turn':turn,'error':detail});break
   except Exception as e:
    message=str(e).replace(key,'[redacted]') if key else str(e)
    event('policy.error',name,{'turn':turn,'error':message});errors.append({'actor':name,'turn':turn,'error':message});break
  event('policy.finished',name,{})
 started=time.monotonic();result=None
 try:
  compose('up','-d','--build','--force-recreate',env=env)
  ready_until=time.monotonic()+900
  while True:
   try:
    if call('/health',timeout=3)['ready']:break
   except Exception:pass
   state=subprocess.run(['docker','inspect','--format','{{.State.Status}}',container_name],capture_output=True,text=True,timeout=10)
   if state.returncode or state.stdout.strip() in ['exited','dead']:raise RuntimeError('MA container exited before readiness; inspect episode engine/client logs')
   if any('Fatal error:' in p.read_text(errors='replace') for p in job.glob('client-*.log')):
    raise RuntimeError('A game browser exited before readiness; inspect episode/client-*.log')
   if time.monotonic()>ready_until:raise RuntimeError('All-player readiness timed out')
   time.sleep(2)
  receipt=call('/arm',{'duration_seconds':args.duration},timeout=180);armed_at=time.monotonic()
  isolated=False
  try:call('/actors/mab/observation',token=receipt['tokens']['maa'])
  except urllib.error.HTTPError as e:isolated=e.code==403
  if not isolated:raise RuntimeError('Actor capability isolation failed')
  event('eval.started',None,{'evidence_kind':kind,'scenario':scenario})
  # Four trusted policy loops; models can emit only typed actions, never shell/code.
  with concurrent.futures.ThreadPoolExecutor(max_workers=len(scenario['actors'])) as pool:
   futures=[pool.submit(policy,a,receipt['tokens'][a['id']]) for a in scenario['actors']]
   end=time.monotonic()+args.duration+10
   while time.monotonic()<end:
    if (job/'cutoff.json').exists():break
    time.sleep(1)
   for f in futures:f.result()
  if not (job/'cutoff.json').exists():raise RuntimeError('Engine cutoff missing')
  cutoff=json.loads((job/'cutoff.json').read_text());before=hashlib.sha256((job/'cutoff.json').read_bytes()).hexdigest()
  # Actual post-cutoff dispatch must be rejected and must not alter the score receipt.
  rejected=False
  try:call('/actors/maa/action',{'type':'say','text':'late cutoff probe'},receipt['tokens']['maa'])
  except urllib.error.HTTPError as e:rejected=e.code==409
  time.sleep(2)
  stable=before==hashlib.sha256((job/'cutoff.json').read_bytes()).hexdigest()
  coverage=len(answered)==len(scenario['actors'])
  valid=cutoff['valid'] and rejected and stable and isolated and coverage and cutoff['final']['elapsedMs']<=cutoff['durationMs']
  result={'status':'evaluated' if valid else 'error','benchmark_status':'completed' if valid else 'invalid','benchmark_score':max(cutoff['teams'].values()) if valid else None,'gates':{'all_players_ready':True,'actor_capability_isolation':isolated,'all_provider_actors_responded':coverage,'authoritative_cutoff':cutoff['valid'],'last_tick_before_deadline':cutoff['final']['elapsedMs']<=cutoff['durationMs'],'late_action_rejected':rejected,'cutoff_receipt_immutable':stable},'duration_seconds':args.duration,'exit_code':0 if valid else 1,'evidence_kind':kind,'model':None if args.scripted else model,'teams':cutoff['teams'],'actor_scores':cutoff['scores'],'usage':{'calls':len(usages),'reportedCostUsd':sum(float(u.get('cost') or 0) for u in usages),'unknownCostCalls':sum(u.get('cost') is None for u in usages)},'policy_errors':errors,'cutoff':cutoff,'run_id':stamp}
 except Exception as e:
  result={'status':'error','benchmark_status':'not_evaluated','benchmark_score':None,'gates':{},'duration_seconds':time.monotonic()-started,'exit_code':1,'error':str(e),'evidence_kind':kind,'run_id':stamp}
 finally:
  try:
   logs=subprocess.run(['docker','logs',container_name],capture_output=True,text=True,timeout=10)
   (job/'container.log').write_text(logs.stdout+logs.stderr)
   compose('stop',env=env)
  except Exception as e:
   if result is not None:result['cleanup_error']=str(e)
  if result:
   result['bench']='runebench';result['task']=scenario['id'];result['lane']=kind
   if isinstance(result.get('gates'),dict):result['gates']=[{'name':k,'passed':v} for k,v in result['gates'].items()]
   write(job/'job_result.json',result)

  event('eval.run.terminal',None,{'result':result})
 print(json.dumps({'run':str(run),'result':result},indent=2))
 return result['exit_code']
if __name__=='__main__':
 with (HERE/'.run.lock').open('a') as lock:
  fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
  raise SystemExit(main())
