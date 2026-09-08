#!/usr/bin/env python3
"""No-provider ordinary annotations over one retained rollout from each eval."""
import hashlib,json,pathlib,sys
ROOT=pathlib.Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT.parent/'containers/src'))
from synth_containers.tracing.research import retained_records
from synth_containers.tracing.annotation import AnnotationService,AnnotationStore,DefinitionRegistry,register_builtin_annotators,AnnotatorProgramV1,RunnerKind
from synth_containers.tracing.annotation.builtin import ENVIRONMENT_STEP_STATUS_ID
from synth_containers.tracing.annotation.proposal import empty_proposal
from synth_containers.tracing.models.standards import AnnotationOutputContractV1,AnnotationTaskKind,AnnotationTaxonV1,TraceAnnotatorDefinitionV1,ProducerKind
DEFINITION=TraceAnnotatorDefinitionV1(annotator_id='research.recorded-event.v1',name='Recorded event',purpose='Confirm the first recorded event is present.',taxonomy=('trace.recorded',),required_subject_scope='event',minimum_evidence=1,confidence_semantics='deterministic',output_contract=AnnotationOutputContractV1(task_kind=AnnotationTaskKind.CLASSIFY,annotation_types=('observation',),taxonomy=(AnnotationTaxonV1(label='trace.recorded'),),allowed_producer_kinds=(ProducerKind.DETERMINISTIC,))).sealed()
PROGRAM=AnnotatorProgramV1(program_id='research.recorded-event.program.v1',runner_kind=RunnerKind.DETERMINISTIC,program_ref='research.recorded-event').sealed()
def recorded_event(document,context):
 proposal=empty_proposal(trace_id=document.trace_id,trace_digest=document.content_digest)
 event=document.events[0];selector={'kind':'event','entity_id':event.event_id}
 proposal['findings'].append({'target':selector,'annotation_type':'observation','labels':['trace.recorded'],'payload':{},'confidence':1.0,'rationale':'The cited event is present in the sealed trace.','evidence':[selector]})
 return proposal
def main():
 out=ROOT/'artifacts/trace-research-e2e/ordinary-annotations';out.mkdir(exist_ok=True)
 reports=[]
 for engine in json.loads((ROOT/'artifacts/trace-research-e2e/engines.json').read_text())[:2]:
  selected=engine['rollouts'][0];wanted=selected['result']['trace']['bundle_trace_digest'];trace=None
  hashes={path:hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest() for path in engine['archives']}
  for archive in engine['archives']:
   with retained_records(pathlib.Path(archive)) as records:
    for record in records:
     if record.trace.content_digest==wanted:trace=record.trace
   if trace:break
  assert trace is not None
  registry=DefinitionRegistry();register_builtin_annotators(registry);registry.register(DEFINITION,PROGRAM,domain="research",deterministic_program=recorded_event)
  service=AnnotationService(store=AnnotationStore(out/engine['environment']/'store'),registry=registry)
  service.register_trace(trace);request=service.request_for(trace,DEFINITION.annotator_id)
  assert str(request.runner_kind)=='deterministic'
  job=service.submit_and_run(request);assert str(job.state)=='sealed',job.error
  evidence=service.evidence_head(trace.trace_id);assert evidence is not None and evidence.annotations
  report={'environment':engine['environment'],'selectedRolloutId':selected['rolloutId'],'selectedTraceDigest':wanted,'job':job.to_dict(),'evidence':evidence.to_dict(),'providerCalls':0,'jesterkyEnabled':False}
  (out/engine['environment']/'receipt.json').write_text(json.dumps(report,indent=2,default=str));reports.append(report)
  assert hashes=={path:hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest() for path in engine['archives']}
  print(engine['environment'],str(job.state),len(evidence.annotations),flush=True)
 (out/'receipt.json').write_text(json.dumps({'status':'passed','selectedRollouts':2,'unselectedRollouts':2,'providerCalls':0,'jesterkyEnabled':False,'archivesUnchanged':True,'jobs':reports},indent=2,default=str))

if __name__=='__main__':main()
