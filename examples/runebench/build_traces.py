"""Build V5 authority and shared viewer bindings from existing runs, without inference."""
import json, sys
from pathlib import Path
HERE = Path(__file__).resolve().parent

from synth_containers.tracing.adapters.runebench import runebench_projection
from synth_containers.tracing.validation.validator import validate_trace, validate_evidence
from synth_containers.tracing.annotation_store import AnnotationStore
from synth_containers.tracing.projections.rollout_inspector import rollout_inspector_from_sealed
from synth_containers.tracing.projections.visual import visual_from_sealed
from synth_containers.tracing.validation.rehydrate import trace_document_from_payload, evidence_bundle_from_payload

def build():
    projections = {}
    store = AnnotationStore(HERE / 'annotation-evidence')
    output = HERE / 'trace-v5'
    output.mkdir(exist_ok=True)
    for run in sorted((HERE / 'results').iterdir()):
        if not (run / 'episode/job_result.json').exists(): continue
        document, evidence, packet = runebench_projection(run)
        evidence = store.load(document, evidence)
        packet = rollout_inspector_from_sealed(document, evidence)
        findings = validate_trace(document) + validate_evidence(document, evidence)[0]
        errors = [f.to_dict() for f in findings if str(f.severity) == 'error']
        if errors: raise ValueError(errors)
        for name, record in [('trace', document), ('evidence', evidence), ('projection', packet)]:
            (output / f'{run.name}.{name}.json').write_text(json.dumps(record.to_dict(), separators=(',', ':')))
        projections[run.name] = packet.visual.to_dict()
    # Larger Craftax decision capture loads on demand through /annotations.
    for key in ['craftax-retained']:
        if (output / f'{key}.trace.json').exists():
            doc = trace_document_from_payload(json.loads((output / f'{key}.trace.json').read_text()))
            base = evidence_bundle_from_payload(json.loads((output / f'{key}.evidence.json').read_text()))
            projections[key] = visual_from_sealed(doc, store.load(doc, base)).to_dict()
    (HERE / 'trace-bindings.json').write_text(json.dumps({'traceProjections': projections}, separators=(',', ':')))
    print(json.dumps({'runs': len(projections), 'items': sum(len(p['items']) for p in projections.values())}))
if __name__ == '__main__': build()
