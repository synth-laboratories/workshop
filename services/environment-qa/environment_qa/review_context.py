"""Give reviewers DAG ancestors, never race-dependent downstream peer outputs."""
import copy

def ancestral(run, gate):
    nodes={g['id']:g for g in run.get('gates',[])}
    if gate['id'] not in nodes:return run
    allowed=set();pending=list(nodes[gate['id']]['depends_on'])
    while pending:
        id=pending.pop()
        if id in allowed:continue
        allowed.add(id);pending.extend(nodes[id]['depends_on'])
    # Explicitly seeded evidence from another sealed run has no current DAG
    # node. It remains available to the replay stage.
    def visible(id):return id not in nodes or id in allowed
    result=dict(run,evidence=[e for e in run['evidence'] if visible(e['gate'])])
    findings=[]
    for original in run['findings']:
        if not visible(original.get('gate_id')):continue
        item=copy.deepcopy(original)
        assessments=item.get('assessments',[])
        retained=[a for a in assessments if visible(a.get('gate_id'))]
        if len(retained)!=len(assessments):
            item['assessments']=retained;item['disposition']='proposed'
            attribution=None
            for a in retained:
                status=a['status']
                if a.get('gate_id')=='critic' and attribution is not None and ((attribution=='dismissed')!=(status=='dismissed')):
                    status='unresolved'
                item['disposition']=status
                if a.get('gate_id')=='attribution':attribution=a['status']
        findings.append(item)
    result['findings']=findings
    return result
