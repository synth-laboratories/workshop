#!/usr/bin/env python3
"""Adapt retained RuneBench receipts for the shared native archive acceptance test.

This does not create eval authority or rewrite rewards/status. RuneBench's task
suffix is a repeat identifier, not a randomized world seed.
"""
import json,pathlib,sys
source=pathlib.Path(sys.argv[1]);output=pathlib.Path(sys.argv[2])
rows=json.loads(source.read_text());rollouts=[];archives=[]
for row in rows:
 result=row['result'];trace=dict(result['trace']);trace['bundle_trace_digest']=trace['trace_digest']
 bundle=row['bundle'];repeat=int(result['task_instance_id'].rsplit('-',1)[1])
 rollouts.append({'seed':repeat,'rolloutId':row['rolloutId'],'result':{**result,'trace':trace},'researchContext':{'environment':'runebench','taskId':result['task_instance_id'],'seed':None,'repeat':repeat,'model':result['model'],'effort':result['reasoning_effort'],'harnessRevision':'harbor-0.22.0/codex-0.145.0','promptRevision':bundle['components']['prompt'],'protocolRevision':bundle['components']['prompt'],'policyBundleDigest':bundle['digest']}})
 archives.append(row['archive'])
output.write_text(json.dumps([{'environment':'runebench','rollouts':rollouts,'archives':archives,'receiptSource':str(source.resolve()),'seedSemantics':'repeat identifier; no world randomization'}],indent=2))
