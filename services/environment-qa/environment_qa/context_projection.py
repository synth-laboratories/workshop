"""Bound adjudication context without deleting cited source evidence."""
import hashlib
import json

def focus_external(files,run):
    if len(json.dumps(files).encode())<150000:return files
    citations={}
    for finding in run.get('findings',[]):
        for quote in [finding,*finding.get('supporting_evidence',[])]:
            citations.setdefault(quote.get('path'),[]).append(quote.get('evidence',''))
    for evidence in run.get('evidence',[]):
        for contract in evidence['result'].get('contracts',[]):
            for path_key,quote_key in [('source_path','evidence'),('consumer_path','consumer_evidence'),('relevance_path','relevance_evidence')]:
                citations.setdefault(contract.get(path_key),[]).append(contract.get(quote_key,''))
    result=dict(files)
    for name,body in files.items():
        if not name.startswith('external/') or len(body)<1600:continue
        lines=body.splitlines(keepends=True)
        selected=set(range(min(4,len(lines))))
        for quote in citations.get(name,[]):
            if not quote or quote not in body:continue
            start=body[:body.index(quote)].count('\n')
            selected.update(range(max(0,start-5),min(len(lines),start+quote.count('\n')+7)))
        groups=[]
        for index in sorted(selected):
            if groups and groups[-1][-1]+1==index:groups[-1].append(index)
            else:groups.append([index])
        projected='[Focused external source: all cited spans plus context retained. Uncited text omitted from this adjudication, not disproven or absent. Original source projection SHA256 '+hashlib.sha256(body.encode()).hexdigest()+']\n'
        projected+=''.join(f'[Original lines {g[0]+1}-{g[-1]+1}]\n'+''.join(lines[i] for i in g) for g in groups)
        if len(projected)<len(body):result[name]=projected
    return result
