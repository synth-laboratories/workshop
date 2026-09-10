#!/usr/bin/env python3
"""Rebuildable swarm index and a read-only query path shared by viewer and analysis."""
import sys
import argparse, datetime as dt, hashlib, json, sqlite3, re, subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent
OUT=HERE/'results'
DB=HERE/'swarm.sqlite'
QUERIES={
 'all':"SELECT id AS run_id, evidence_kind, status, actors, score FROM episodes ORDER BY id DESC",
 'failed_chops':"SELECT run_id, actor, ms, text, ref FROM events WHERE kind='action.completed' AND action_type='chop' AND success=0 ORDER BY run_id,ms",
 'heard_teammate':"SELECT e.run_id,e.actor,e.sender,e.ms,e.text,e.ref FROM events e JOIN actors a ON a.run_id=e.run_id AND a.id=e.actor JOIN actors s ON s.run_id=e.run_id AND s.id=lower(e.sender) WHERE e.kind='message.observed' AND e.actor<>lower(e.sender) AND a.team=s.team ORDER BY e.run_id,e.ms",
 'same_tree':"SELECT a.run_id,a.actor,b.actor AS other,a.ms,a.x,a.z,a.ref FROM events a JOIN events b ON a.run_id=b.run_id AND a.actor<b.actor AND a.x=b.x AND a.z=b.z AND abs(a.ms-b.ms)<5000 WHERE a.kind='action.started' AND b.kind='action.started' AND a.action_type='chop' AND b.action_type='chop' ORDER BY a.run_id,a.ms"
}
def lines(p):
 if p.exists():
  for i,line in enumerate(p.open()):
   try:yield i+1,json.loads(line)
   except json.JSONDecodeError:continue

def build():
 subprocess.run([sys.executable,str(HERE/'build_traces.py')],check=True)
 db=sqlite3.connect(DB)
 db.executescript('''DROP TABLE IF EXISTS episodes;DROP TABLE IF EXISTS actors;DROP TABLE IF EXISTS events;DROP TABLE IF EXISTS samples;
 CREATE TABLE episodes(id TEXT PRIMARY KEY,evidence_kind TEXT,status TEXT,actors INTEGER,score REAL);
 CREATE TABLE actors(run_id TEXT,id TEXT,team TEXT,role TEXT,score REAL,PRIMARY KEY(run_id,id));
 CREATE TABLE events(run_id TEXT,actor TEXT,ms REAL,kind TEXT,action_type TEXT,success INTEGER,sender TEXT,text TEXT,x INTEGER,z INTEGER,ref TEXT,payload TEXT);
 CREATE TABLE samples(run_id TEXT,actor TEXT,ms REAL,xp REAL,x INTEGER,z INTEGER,ref TEXT);
 CREATE INDEX event_kind ON events(kind,run_id);CREATE INDEX sample_actor ON samples(run_id,actor,ms);''')
 runs=[];revision=hashlib.sha256()
 for run in sorted(OUT.iterdir()):
  mp=run/'matrix_run_manifest.json';jp=run/'episode/job_result.json'
  if not mp.exists() or not jp.exists():continue
  manifest=json.loads(mp.read_text());result=json.loads(jp.read_text());config=manifest['config'];sc=config['scenario'];actors=sc['actors'];kind=config['evidence_kind'];job=run/'episode'
  if kind == 'scripted': config = {**config, 'model': None, 'reasoning_effort': None}
  revision.update(mp.read_bytes());revision.update(jp.read_bytes())
  r={'id':run.name,'title':sc['title'],'kind':kind,'status':result['status'],'model':config.get('model'),'effort':config.get('reasoning_effort'),'batch':config.get('batch'),'usage':result.get('usage',{}),'durationMs':(result.get('duration_seconds') or 0)*1000,'actors':actors,'teams':result.get('teams',{}),'scores':result.get('actor_scores',{}),'cost':result.get('usage',{}).get('reportedCostUsd'),'calls':result.get('usage',{}).get('calls',0),'events':[],'samples':[],'frames':{},'cutoff':result.get('cutoff'),'source':str(run),'policyErrors':result.get('policy_errors',[])}
  native=[json.loads(line) for line in (run/'events.jsonl').read_text().splitlines()] if (run/'events.jsonl').exists() else []
  requests={e['payload'].get('decisionId'):e['payload'] for e in native if e['payload'].get('kind')=='model.requested'}
  completions=[e['payload'] for e in native if e['payload'].get('kind')=='model.completed']
  latencies=[e['elapsedMs']-requests[e['decisionId']]['elapsedMs'] for e in completions if e.get('decisionId') in requests]
  trace_path=HERE/'trace-v5'/f'{run.name}.trace.json'
  if trace_path.exists():r['traceDigest']=json.loads(trace_path.read_text()).get('content_digest')
  r['comparison']={'firstResponseMs':min((e['elapsedMs'] for e in completions),default=0),'promptTokens':sum(e.get('usage',{}).get('prompt_tokens',0) for e in completions),'completionTokens':sum(e.get('usage',{}).get('completion_tokens',0) for e in completions),'reasoningTokens':sum((e.get('usage',{}).get('completion_tokens_details') or {}).get('reasoning_tokens',0) or 0 for e in completions),'meanResponseMs':sum(latencies)/len(latencies) if latencies else None,'reasoningResponses':sum(bool(e.get('reasoning') or e.get('reasoning_details')) for e in completions)}
  db.execute('INSERT INTO episodes VALUES(?,?,?,?,?)',(run.name,kind,result['status'],len(actors),result.get('benchmark_score')))
  for a in actors:db.execute('INSERT INTO actors VALUES(?,?,?,?,?)',(run.name,a['id'],a['team'],a['role'],r['scores'].get(a['id'])))
  seen=set()
  def add_event(actor,ms,k,p,ref):
   action=p.get('action',{});res=p.get('result');success=res.get('success') if isinstance(res,dict) else None
   target=p.get('target') or action
   text=(res.get('message','') if isinstance(res,dict) else '') or p.get('text') or action.get('text') or action.get('reason') or (res.get('message','') if isinstance(res,dict) else '') or p.get('content','') or p.get('error','')
   e={'actor':actor,'ms':round(ms),'kind':k,'action':action,'success':success,'text':text,'sender':p.get('sender'),'ref':ref,'target':p.get('target'),'result':res,'decisionId':p.get('decisionId') or action.get('decisionId')}
   r['events'].append(e)
   db.execute('INSERT INTO events VALUES(?,?,?,?,?,?,?,?,?,?,?,?)',(run.name,actor,ms,k,action.get('type'),success,p.get('sender'),text,target.get('x'),target.get('z'),ref,json.dumps(p)))
  for ep in [run/'events.jsonl',job/'events.jsonl']:
   for n,e in lines(ep):
    p=e.get('payload',{});add_event(e.get('actor_id'),p.get('elapsedMs',0),p.get('kind',e.get('kind')),p,f'{ep}:{n}')
  baseline=result.get('cutoff',{}).get('baseline',{}).get('actors',{})
  # Authority samples are the engine's complete ticks, not delayed observer skills.
  for n,s in lines(job/'engine-states.jsonl'):
   for a,v in s['actors'].items():
    if v:
     xp=v['xp']-baseline.get(a,{}).get('xp',0);sample={'actor':a,'ms':round(s['elapsedMs']),'xp':xp,'x':v['x'],'z':v['z'],'authority':'engine'}
     db.execute('INSERT INTO samples VALUES(?,?,?,?,?,?,?)',(run.name,a,s['elapsedMs'],xp,v['x'],v['z'],f'{job}/engine-states.jsonl:{n}'))
     # Downsample projection to 2-second buckets; SQL retains each tick.
     if not r['samples'] or not any(t['actor']==a and t['ms']//2000==sample['ms']//2000 for t in r['samples'][-len(actors)*2:]):r['samples'].append(sample)
  origin=None
  for n,s in lines(job/'states.jsonl'):
   if not all(v and v.get('inGame') for v in s.get('bots',{}).values()):continue
   timestamp=dt.datetime.fromisoformat(s['timestamp']).timestamp()*1000
   if origin is None:origin=timestamp
   ms=s.get('elapsedMs',timestamp-origin)
   if not baseline:
    for a,v in s['bots'].items():
     p=v.get('player',{});r['samples'].append({'actor':a,'ms':round(ms),'xp':None,'x':p.get('worldX'),'z':p.get('worldZ'),'authority':'actor-observation'})
   # Preserve a compact, actor-local observation at each sample for replay inspection.
   for a,v in s['bots'].items():
    frames=r['frames'].setdefault(a,[])
    if len(frames)==0 or ms-frames[-1]['ms']>=3900:
     frames.append({'ms':round(ms),'inventory':[{k:i.get(k) for k in ['slot','id','name','count']} for i in v.get('inventory',[])],'messages':[{k:m.get(k) for k in ['sender','text','tick']} for m in v.get('gameMessages',[])[-4:]]})
    if not baseline:
     for m in v.get('gameMessages',[]):
      if not m.get('sender'):continue
      key=(a,m.get('sender'),m.get('tick'),m.get('text'))
      if key in seen:continue
      seen.add(key);add_event(a,ms,'message.observed',m,f'{job}/states.jsonl:{n}')
  r['events'].sort(key=lambda e:e['ms']);r['samples'].sort(key=lambda s:s['ms'])
  r['durationMs']=r['durationMs'] or max([e['ms'] for e in r['events']]+[s['ms'] for s in r['samples']]+[1])
  r['media']={a['id']:{'url':f'http://127.0.0.1:8128/media/{run.name}/{a["id"]}.mp4','offsetMs':next((e['ms'] for e in r['events'] if e['kind']=='media.started' and e['actor']==a['id']),0)} for a in actors if (job/f'{a["id"]}.mp4').exists()}
  for actor,media in r['media'].items():
   log=job/f'record-{actor}.log'
   m=re.search(r'Duration: N/A, start: ([0-9.]+)',log.read_text(errors='replace')) if log.exists() else None
   origin=result.get('cutoff',{}).get('baseline',{}).get('at')
   if m and origin:
    media['offsetMs']=round(float(m[1])*1000-dt.datetime.fromisoformat(origin).timestamp()*1000)
    media['clockBasis']='ffmpeg-first-input-wall-time-minus-engine-baseline'
   else:media['clockBasis']='spawn-time-estimate'
  runs.append(r)
 db.commit();db.close()
 metadata={'revision':revision.hexdigest(),'runs':runs,'queries':{k:query(v) for k,v in QUERIES.items()},'querySql':QUERIES}
 (HERE/'swarm-data.json').write_text(json.dumps(metadata,separators=(',',':')))
 # Keep raw filesystem references in SQL/data; the pane only needs stable event keys.
 def pane_projection(value):
  if isinstance(value,dict):return {('timeline' if k=='events' else k):pane_projection(v) for k,v in value.items() if k not in ['ref','source']}
  if isinstance(value,list):return [pane_projection(v) for v in value]
  return value
 for r in metadata['runs']:
  for e in r['events']:
   ref=e['ref'];file,line=ref.rsplit(':',1)
   if file.endswith('events.jsonl'):
    scope='episode' if Path(file).parent.name=='episode' else 'run'
    e['evidence']=f'http://127.0.0.1:8128/evidence?run={r["id"]}&scope={scope}&line={line}'
 pane=pane_projection(metadata)
 for r in pane['runs']:
  r['timeline']=[{k:v for k,v in e.items() if k not in ['result','target'] and v is not None} for e in r['timeline'] if e['kind'] in ['policy.action','policy.result','action.started','action.completed','message.observed']]
  for actor,frames in r['frames'].items():
   compact=[]
   for f in frames:
    if not compact or f['ms']-compact[-1]['ms']>=14900:
     compact.append({'ms':f['ms'],'inventory':[{k:v for k,v in i.items() if k!='id'} for i in f['inventory']],'messages':f['messages'][-2:]})
   r['frames'][actor]=compact
  if r.get('cutoff'):
   final=r['cutoff']['final']
   for actor,v in final['actors'].items():
    if v:r['samples'].append({'actor':actor,'ms':round(final['elapsedMs']),'xp':r['scores'][actor],'x':v['x'],'z':v['z'],'authority':'engine'})
 bindings=json.loads((HERE/'trace-bindings.json').read_text());bindings['swarmData']=pane
 (HERE/'trace-bindings.json').write_text(json.dumps(bindings,separators=(',',':')))
 source=(HERE/'viewer.tsx').read_text()
 if len(source.encode())>262144:raise ValueError('Pane exceeds source budget; full evidence remains in SQL')
 (HERE/'viewer.built.tsx').write_text(source)
 print(json.dumps({'episodes':len(runs),'events':sum(len(r['events']) for r in runs),'revision':metadata['revision'],'queries':{k:len(v['rows']) for k,v in metadata['queries'].items()}}))

def query(sql):
 db=sqlite3.connect(f'file:{DB}?mode=ro',uri=True);db.execute('PRAGMA query_only=ON');db.row_factory=sqlite3.Row
 # Hard CPU budget even for user-authored read queries.
 steps=0
 def guard():
  nonlocal steps;steps+=1;return 1 if steps>10000 else 0
 db.set_progress_handler(guard,1000)
 allowed={sqlite3.SQLITE_SELECT,sqlite3.SQLITE_READ,sqlite3.SQLITE_FUNCTION,sqlite3.SQLITE_RECURSIVE}
 db.set_authorizer(lambda op,*args:sqlite3.SQLITE_OK if op in allowed else sqlite3.SQLITE_DENY)
 try:
  rows=[dict(r) for r in db.execute(sql).fetchmany(1000)]
  return {'queryId':hashlib.sha256(sql.encode()).hexdigest()[:12],'sql':sql,'rows':rows,'limit':1000}
 finally:db.close()
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--build',action='store_true');p.add_argument('--sql');p.add_argument('--query',choices=QUERIES);a=p.parse_args()
 if a.build:build()
 else:print(json.dumps(query(a.sql or QUERIES[a.query or 'all']),indent=2))
