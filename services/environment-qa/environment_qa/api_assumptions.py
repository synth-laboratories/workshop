"""Source-derived candidate return-field assumptions, not defect allegations."""
import re

def candidates(files):
    result=[]
    # Inventory concrete task-side data endpoints as well as transitive field
    # reads. Otherwise a planner can silently spend every slot on packaging.
    for path,text in files.items():
        if not path.startswith('solution/'):continue
        seen=set()
        for line in text.splitlines():
            if line.lstrip().startswith('#'):continue
            for match in re.finditer(r'https?://[^\s\"\'<>]+',line):
                endpoint=match[0].rstrip('),;')
                if endpoint in seen:continue
                seen.add(endpoint)
                result.append({'path':path,'candidate_kind':'external_endpoint','endpoint':endpoint,'evidence':line,
                    'notice':'Candidate external-data contract, not a defect. Trace reachability, any selection/filtering, and the final required consumer. Source-download locations may be out of scope. A network response that parses successfully may still violate the consumer alphabet, type, or required fields. Account for this candidate explicitly.'})
    for path,text in files.items():
        if not path.startswith('external/'):continue
        # Stateful library objects can fail during configuration/run even when
        # imports and a generic package wheel both succeed.
        for created in re.finditer(r'(?m)^\s*([A-Za-z_]\w*)\s*=\s*([A-Za-z_]\w*\.[A-Za-z_]\w*)\([^\n]*\)',text):
            variable,producer=created[1],created[2]
            calls=list(re.finditer(r'\b'+re.escape(variable)+r'\.([A-Za-z_]\w*)\(',text[created.end():]))
            methods=list(dict.fromkeys(m[1] for m in calls))
            if len(methods)<2:continue
            start=text.rfind('\n',0,created.start())+1
            end=created.end()+calls[-1].end()
            line_end=text.find('\n',end)
            if line_end!=-1:end=line_end
            result.append({'path':path,'candidate_kind':'stateful_library_protocol','variable':variable,'producer':producer,'methods':methods,
                'evidence':text[start:end][:3000],
                'notice':'Candidate ordered library protocol, not a defect. Trace actual object construction, configuration, private attributes, finalize/setup calls and run calls to the required consumer. Prefer a tiny faithful invocation of this protocol over an unrelated generic package build; replace expensive application work only. Normal file I/O and optional/unreachable objects may be out of scope.'})
        groups={}
        for match in re.finditer(r"\b([A-Za-z_]\w*)\[['\"]([^'\"\n]{1,80})['\"]\]",text):
            if match[1] in {'os','environ'}:continue
            groups.setdefault(match[1],[]).append(match)
        for variable,matches in groups.items():
            keys=sorted({m[2] for m in matches})
            if len(keys)<2:continue
            start=max(0,text.rfind('\n',0,matches[0].start()-1))
            end=text.find('\n',matches[-1].end())
            if end<0:end=len(text)
            excerpt=text[start:end]
            result.append({'path':path,'candidate_kind':'returned_fields','variable':variable,'required_keys':keys,'evidence':excerpt[:2400],
                           'notice':'Candidate field-read assumption only. Trace producer and required consumer before selecting a probe; assignment-only mappings and unreachable paths are not API contracts.'})
    return {str(i):item for i,item in enumerate(result[:24])}
