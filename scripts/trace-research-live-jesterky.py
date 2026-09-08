#!/usr/bin/env python3
"""Real Luna/low annotation of retained rollout traces with local reservations."""
import concurrent.futures,json,os,pathlib,re,sys
ROOT=pathlib.Path(__file__).resolve().parents[1];REPOS=ROOT.parent
sys.path.insert(0,str(REPOS/'containers/src'))
from synth_containers.tracing.research import retained_records
from synth_containers.tracing.annotation import AnnotationJobLimitsV1,AnnotationService,AnnotationStore,AnnotatorProgramV1,DefinitionRegistry,JesterkyRunner,LocalReservationBroker,ReservationBindingV1,RunnerKind
from synth_containers.tracing.annotation.pricing import PriceTable
from synth_containers.tracing.models.standards import AnnotationOutputContractV1,AnnotationTaskKind,AnnotationTaxonV1,TraceAnnotatorDefinitionV1
OUT=ROOT/'artifacts/trace-research-e2e/live-jesterky';OUT.mkdir(parents=True,exist_ok=True)
MODEL='openrouter/openai/gpt-5.6-luna'

def credentials():
 for line in (REPOS/'evals/.env').read_text().splitlines():
  match=re.match(r'(?:export\s+)?OPENROUTER_API_KEY\s*=\s*(.*)',line)
  if match:return {'OPENROUTER_API_KEY':match.group(1).strip().strip('\"\'')}
 raise RuntimeError('authorized project-local provider key unavailable')

def analyze(receipt):
 name=receipt['environment'];out=OUT/name;out.mkdir(exist_ok=True)
 result=receipt['rollouts'][0]['result'];wanted=result['trace']['bundle_trace_digest'];trace=None
 for archive in receipt['archives']:
  with retained_records(pathlib.Path(archive)) as records:
   for record in records:
    if record.trace.content_digest==wanted:trace=record.trace
  if trace:break
 assert trace is not None,wanted
 definition=TraceAnnotatorDefinitionV1(annotator_id='research.observed-action.v1',name='Observed actions',purpose='Describe recorded event evidence without inferring intent.',taxonomy=('trace.observed',),required_subject_scope='event',minimum_evidence=1,model=MODEL,output_contract=AnnotationOutputContractV1(task_kind=AnnotationTaskKind.CLASSIFY,annotation_types=('observation',),taxonomy=(AnnotationTaxonV1(label='trace.observed'),))).sealed()
 program=AnnotatorProgramV1(program_id='research.observed-action.program.v6',runner_kind=RunnerKind.JESTERKY,prompt='Use the trace_annotation MCP tools. Inspect only the first event ID in job.event_ids with trace_get_event, verify its whole-event selector with trace_resolve_selector, and return exactly one factual observation of that event. Do not browse the whole trace for this smoke test. Inspect the supplied recorded events. Return one or two factual findings per shard with annotation_type observation, label trace.observed, an event target from job.event_ids, and at least one exact event evidence selector. Describe what was recorded; do not infer intent or label success without evidence. Include the exact source_trace_id and source_trace_digest from the job. Use the required structured proposal schema.',paid=True).sealed()
 registry=DefinitionRegistry();registry.register(definition,program,domain='research')
 runner=JesterkyRunner(command=(str(REPOS/'jesterky/target/release/jesterky'),),default_model=MODEL,default_effort='low',price_table=PriceTable.from_dict({'models':{MODEL:{'input':0.5,'cached':0.5,'output':2.0}}},source='OpenRouter 2026-09-07 conservative upper rates including long-context cache writes'),extra_env=credentials())
 broker=LocalReservationBroker(out/'broker')
 service=AnnotationService(store=AnnotationStore(out/'store'),registry=registry,runners={runner.kind:runner},broker=broker)
 service.register_trace(trace)
 request=service.request_for(trace,definition.annotator_id,model=MODEL,reasoning_effort='low',runner_kind=RunnerKind.JESTERKY,limits=AnnotationJobLimitsV1(max_total_tokens=3000000,max_cost_usd=3.0,timeout_seconds=300))
 estimate=service.estimate(request)
 reservation=broker.issue(cap_usd_micros=3000000,binding=ReservationBindingV1(trace_digest=trace.content_digest,annotator_id=definition.annotator_id,model=MODEL,session_id='research-e2e')).reservation_id
 job=service.submit_and_run(request,reservation_id=reservation,session_id='research-e2e')
 head=service.evidence_head(trace.trace_id)
 report={'environment':name,'model':MODEL,'effort':'low','estimate':estimate.to_dict(),'job':job.to_dict(),'evidence':head.to_dict() if head else None}
 (out/f'{job.job_id}.receipt.json').write_text(json.dumps(report,indent=2,default=str))
 (out/'receipt.json').write_text(json.dumps(report,indent=2,default=str));print(name,str(job.state),str(job.error)[:500],flush=True)
 assert str(job.state)=='sealed',report['job'].get('error')
 assert head and head.annotations,'live model must produce validated evidence'
 return report

if __name__=='__main__':
 reserved=0
 for p in OUT.glob('*/store/jobs/*/workspace/jesterky-budget.json'):
  ledger=json.loads(p.read_text());c=ledger['config']
  ratio=max(0.5/c['inputUsdPerMillion'],2.0/c['outputUsdPerMillion'])
  reserved+=__import__('math').ceil(ledger['reserved_micros']*ratio)
 # Two RuneBench arms reserve another $2.50. Rates bound every text request.
 if reserved+6000000+2500000>20000000:raise RuntimeError('aggregate $20 experiment limit would be exceeded')
 receipts=json.loads((ROOT/'artifacts/trace-research-e2e/engines.json').read_text())
 with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:reports=list(pool.map(analyze,receipts[:2]))
 (OUT/'receipt.json').write_text(json.dumps(reports,indent=2,default=str))
