#!/usr/bin/env python3
"""Retained local engine endpoints for native optimizer launch acceptance."""
import json,signal,socket,subprocess,threading,time,os,hashlib
from dataclasses import replace
import importlib.util
from pathlib import Path
spec=importlib.util.spec_from_file_location('engines',Path(__file__).with_name('trace-research-live-engines.py'));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
ROOT=m.ROOT;REPOS=m.REPOS;ready=m.ready;uvicorn=m.uvicorn;create_compat_app=m.create_compat_app;CRAFTAX_CODE_POLICY=m.CRAFTAX_CODE_POLICY;DUNGEONGRID_CODE_POLICY=m.DUNGEONGRID_CODE_POLICY

# Mount ordinary analysis without enabling automatic post-rollout work.
os.environ['SYNTH_ANNOTATION']='on'
annotation_spec=importlib.util.spec_from_file_location('ordinary',Path(__file__).with_name('trace-research-ordinary-annotations.py'));ordinary=importlib.util.module_from_spec(annotation_spec);annotation_spec.loader.exec_module(ordinary)

root=ROOT/'artifacts/trace-research-e2e/native-launch';root.mkdir(exist_ok=True)
log=(root/'engine.log').open('w')
dungeon_binary=REPOS/'evals/workshop/runtime-builds/dungeongrid/release/dungeongrid_gold'
os.environ['SYNTH_DUNGEONGRID_URL']='http://127.0.0.1:18189'
engines=[]
# Fingerprint the exact owned engine bytes and scenario inputs used by this run.
def environment_version(name):
 binary=REPOS/'gamebench/tasks/craftax-singleplayer/gold_rust/target/release/craftax_gold' if name=='craftax' else dungeon_binary
 files=[binary]
 if name=='dungeongrid':files+=sorted((REPOS/'gamebench/tasks/dungeongrid-singleplayer/defaults/scenarios').rglob('*.json'))
 hashes={str(p.relative_to(REPOS)):'sha256:'+hashlib.file_digest(p.open('rb'),'sha256').hexdigest() for p in files}
 digest='sha256:'+hashlib.sha256(json.dumps(hashes,sort_keys=True).encode()).hexdigest()
 (root/f'{name}-environment-version.json').write_text(json.dumps({'environmentVersion':digest,'files':hashes},indent=2))
 return digest
servers=[];done=threading.Event();signal.signal(signal.SIGTERM,lambda *_:done.set());signal.signal(signal.SIGINT,lambda *_:done.set())
try:
 for binary,port in [(REPOS/'gamebench/tasks/craftax-singleplayer/gold_rust/target/release/craftax_gold',18188),(dungeon_binary,18189)]:
  engines.append(subprocess.Popen([str(binary),'--host','127.0.0.1','--port',str(port)],cwd=REPOS/'gamebench',stdout=log,stderr=log))
 ready('http://127.0.0.1:18188');ready('http://127.0.0.1:18189')
 receipts=[]
 for name,target,port in [('craftax',CRAFTAX_CODE_POLICY,18191),('dungeongrid',DUNGEONGRID_CODE_POLICY,18192)]:
  target=replace(target,environment_version=environment_version(name))
  app=create_compat_app(target,storage_root=root/name/'storage');__import__('synth_containers.tracing.annotation.container',fromlist=['mount_annotation']).mount_annotation(app,storage_root=root/name/'storage').registry.register(ordinary.DEFINITION,ordinary.PROGRAM,domain='research',deterministic_program=ordinary.recorded_event);sock=socket.socket();sock.bind(('127.0.0.1',port));server=uvicorn.Server(uvicorn.Config(app,log_level='error'));thread=threading.Thread(target=lambda s=server,k=sock:s.run(sockets=[k]),daemon=True);thread.start();servers.append((server,thread));base=f'http://127.0.0.1:{port}';ready(base);info=m.call(base,'/info');receipts.append({'environment':name,'baseUrl':base,'info':info})
 (root/'servers.json').write_text(json.dumps(receipts,indent=2));print('Native launch engines ready',flush=True);done.wait()
finally:
 for server,thread in servers:server.should_exit=True;thread.join(5)
 for engine in engines:engine.terminate();engine.wait(timeout=10)
 log.close()
