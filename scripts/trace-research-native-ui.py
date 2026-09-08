#!/usr/bin/env python3
"""Inspect retained engine evidence through a running isolated Workshop app."""
import json,pathlib,time,urllib.request
ROOT=pathlib.Path(__file__).resolve().parents[1]/'artifacts/trace-research-e2e/native-app-store'
cfg=json.loads((ROOT/'visuals-ipc.json').read_text())
def call(path,body=None):
 req=urllib.request.Request(cfg['url']+path,data=None if body is None else json.dumps(body).encode(),headers={'Authorization':'Bearer '+cfg['token'],'Content-Type':'application/json'})
 with urllib.request.urlopen(req,timeout=120) as r:return json.load(r)
query=call('/v1/traces/query',{'query':{'schemaVersion':'synth.trace-query.v2','evalJobIds':['e2e-craftax','e2e-dungeongrid'],'grain':'episodes','limit':20}})
(ROOT/'native-query.json').write_text(json.dumps(query,indent=2))
visual=call('/v1/traces/open_query',{'snapshot_id':query['snapshotId']})
(ROOT/'native-catalog.json').write_text(json.dumps(visual,indent=2))
capture=call('/v1/review-window/capture',{'visualId':visual['visualId'],'width':1200,'height':800,'outputPath':str(ROOT/'native-catalog.png')})
(ROOT/'native-catalog-capture.json').write_text(json.dumps(capture,indent=2))
traces=call('/v1/traces')['traces']
visual=call('/v1/traces/open',{'trace_id':traces[0]['traceId']})
(ROOT/'native-inspector.json').write_text(json.dumps(visual,indent=2))
# Showing a visual updates the registry; capture selects the native review pane.
call('/v1/review-window/capture',{'visualId':visual['visualId'],'width':1200,'height':800,'outputPath':str(ROOT/'native-inspector-loading.png')})
deadline=time.monotonic()+60
while True:
 observation=call('/v1/review-observations/'+visual['visualId']).get('observation')
 if observation and observation.get('semanticEventCount',0)>0:break
 if time.monotonic()>deadline:raise RuntimeError('Inspector did not render retained events')
 time.sleep(.5)
capture=call('/v1/review-window/capture',{'visualId':visual['visualId'],'width':1200,'height':800,'outputPath':str(ROOT/'native-inspector.png')})
(ROOT/'native-inspector-capture.json').write_text(json.dumps(capture,indent=2))
print('Native query, catalog, inspector and captures completed')
