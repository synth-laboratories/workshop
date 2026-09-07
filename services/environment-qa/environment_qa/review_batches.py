"""Conservative aggregation for exhaustive, size-bounded finding reviews."""

def combine(results, role, context_ref, input_digest, packet_bytes):
    dispositions=[d for r in results for d in r.get('dispositions',[])]
    ids=[d['finding_id'] for d in dispositions]
    if len(ids)!=len(set(ids)):raise ValueError('Duplicate finding across review batches')
    assessments=[r.get('assessment') for r in results if r.get('assessment')]
    assessment=None
    if assessments:
        verdicts=[a['verdict'] for a in assessments]
        assessment={'verdict':'fail' if 'fail' in verdicts else 'inconclusive' if 'inconclusive' in verdicts else 'pass',
                    'rationale':'All finding batches reviewed. '+' '.join(a['rationale'] for a in assessments),
                    'unresolved':list(dict.fromkeys(x for a in assessments for x in a['unresolved']))}
    return {'findings':[],'dispositions':dispositions,'assessment':assessment,'coverage':[],
            'limitations':[l for r in results for l in r.get('limitations',[])]+[
                'Finding ledger reviewed exhaustively in bounded batches. Cross-batch duplicate dismissal was not performed; every allegation remains represented.'],
            'role':role,'context_ref':context_ref,'input_digest':input_digest,
            'batch_review':{'original_packet_bytes_excluding_fixed_fields':packet_bytes,
                            'contexts':[r['context_ref'] for r in results],
                            'all_batches_completed':True}}
