"""Exact observed reference diagnostics; no inferred root cause or task verdict."""
import re
from .review import finding

def reference_diagnostics(documents):
    result=[];seen=set()
    for path,text in sorted(documents.items()):
        if not path.endswith(('oracle.txt','exception.txt')): continue
        for number,line in enumerate(text.splitlines(),1):
            if not (re.match(r'^\s*(?:error:|ERROR:|E:)',line) or
                    re.match(r'^\s*[\w.]+(?:Error|Exception):',line)):
                continue
            quote=line.strip()
            if quote in seen or len(quote)>1000: continue
            seen.add(quote)
            result.append(finding('reference_execution','warning',
                'Observed reference execution diagnostic (cause and task impact require attribution): '+quote,
                path,number,line,'reference_diagnostic_'+str(len(result))))
            if len(result)>=6: return result
    return result
