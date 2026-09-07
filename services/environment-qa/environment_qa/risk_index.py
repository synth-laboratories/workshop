"""Source locations needing environment review; candidates, never findings."""
import re

RULES = {
    'filesystem_boundary': (r'\b(?:os\.(?:rename|replace)|rename|mv)\s*\(', 'Check whether source and destination can be on different mounts; rename is not a cross-filesystem copy.'),
    'hard_resource_limit': (r'\bulimit\b|setrlimit', 'Check soft versus hard limits and whether the runtime permits this change; preserve backend conditions.'),
    'allocation_budget': (r'\b(?:malloc|calloc|realloc)\s*\(|\b(?:zeros|empty|ones)\s*\(', 'Derive allocation bytes and simultaneously live buffers from constants and compare with declared memory; do not invent measurements.'),
    'session_lifecycle': (r'\bspawn\b|\btelnet\b|Popen\(', 'Trace ownership, close/wait/exit paths and whether a subsequent verifier can reconnect or reuse the resource.'),
    'dependency_resolution': (r'install\.packages|pip\s+install|install_requires|git\s+clone', 'Inspect version constraints and required build tools/APIs. A specific compatibility defect requires source or observed metadata, not generic drift speculation.'),
    'external_input': (r'\b(?:curl|wget)\b|requests\.get|urlopen', 'Check status handling, data invariants and mutation assumptions. Do not invent current responses.'),
}

def risk_index(files):
    result=[]
    for path,text in sorted(files.items()):
        if path.startswith('evidence/'): continue
        for line_no,line in enumerate(text.splitlines(),1):
            for kind,(pattern,question) in RULES.items():
                if re.search(pattern,line):
                    result.append(dict(kind=kind,path=path,line=line_no,source=line[:1000],question=question))
    return result[:200]
