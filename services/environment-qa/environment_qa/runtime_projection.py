"""Bound adjudication logs while retaining every cited measurement and errors."""
import hashlib
import re

def focus(files, run):
    quotes=[q.get('evidence','') for f in run['findings'] for q in [f,*f.get('supporting_evidence',[])]]
    for ev in run.get('evidence',[]):
        if ev['gate']=='contract-analysis':
            for assessment in ev['result'].get('coverage',{}).values():
                quotes.extend(c.get('evidence','') for c in assessment.get('citations',[]))
    result=dict(files)
    for path,body in files.items():
        if not path.startswith(('runtime/','cached-runtime/')) or not path.endswith(('.stdout.txt','.stderr.txt','test-stdout.txt','test-stderr.txt','oracle.txt')) or len(body)<3000:continue
        lines=body.splitlines(keepends=True)
        selected=set(range(min(3,len(lines))))|set(range(max(0,len(lines)-10),len(lines)))
        for quote in quotes:
            if not quote or quote not in body:continue
            offset=0
            while True:
                index=body.find(quote,offset)
                if index<0:break
                start=body[:index].count('\n')
                selected.update(range(max(0,start-2),min(len(lines),start+quote.count('\n')+3)))
                offset=index+len(quote)
        for i,line in enumerate(lines):
            if re.search(r'Traceback|\b(?:ERROR|FAIL|FAILED|PASS|PASSED)\b|\b\w*(?:Error|Exception):|QA_',line):
                selected.update(range(max(0,i-2),min(len(lines),i+3)))
        groups=[]
        for index in sorted(selected):
            if groups and groups[-1][-1]+1==index:groups[-1].append(index)
            else:groups.append([index])
        projected='[Adjudication runtime projection: all cited measurements, diagnostic lines and boundary context retained. Omitted text is not disproven or absent. Full original artifacts remain sealed. Supplied-text SHA256 '+hashlib.sha256(body.encode()).hexdigest()+']\n'
        projected+=''.join(f'[Supplied lines {g[0]+1}-{g[-1]+1}]\n'+''.join(lines[i] for i in g) for g in groups)
        if len(projected)<len(body):result[path]=projected
    return result
