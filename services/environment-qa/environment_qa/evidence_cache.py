"""Verified reuse of measured oracle evidence; never prior opinions or labels."""
import hashlib
import re
from .core import verify_seal
from .runtime import diagnostic_preview
from .diagnostics import reference_diagnostics

def oracle_evidence(prior_store,prior,bundle):
    if not verify_seal(prior) or prior['bundle']['sha256']!=bundle['sha256']:
        raise ValueError('Cached evidence seal or task digest mismatch')
    documents={}; records=[]
    available=sorted((e for e in prior['evidence'] if re.fullmatch(r'oracle-\d+',e['gate']) and e['result'].get('artifacts')),
                     key=lambda e:int(e['gate'].split('-')[-1]))
    for ev in available[:1]:
        result=ev['result']
        for artifact in result.get('artifacts',[]):
            relative=artifact['path']
            if relative.rsplit('/',1)[-1] not in {'oracle.txt','test-stdout.txt','test-stderr.txt','exception.txt','result.json'}: continue
            path=(prior_store.root/relative).resolve()
            if not path.is_relative_to(prior_store.root.resolve()) or path.is_symlink():
                raise ValueError('Unsafe cached artifact path')
            data=path.read_bytes()
            if hashlib.sha256(data).hexdigest()!=artifact['sha256']: raise ValueError('Cached artifact changed')
            name='cached-runtime/'+str(len(records)).zfill(2)+'-'+relative.rsplit('/',1)[-1]
            documents[name]=diagnostic_preview(data)
            records.append({'document':name,'original_artifact':artifact})
    return {'findings':reference_diagnostics(documents),'documents':documents,'records':records,
            'prior_run_id':prior['id'],'prior_seal':prior['seal'],'selected_gate':available[0]['gate'] if available else None,
            'limitations':['Warm evidence reuse: earliest artifact-backed oracle attempt only, selected by index rather than outcome; not a new execution. No previous findings or public reference labels supplied. Original backend, timeout and dependency state apply; missing output is not success.']}
