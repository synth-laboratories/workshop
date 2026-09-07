"""Mandatory smoke coverage for explicitly invoked Python build frontends."""
import re

def merge_required(required, proposed):
    """Strengthen an existing invocation check without consuming another slot."""
    result=[dict(c) for c in proposed];missing=[]
    for mandatory in required:
        frontend=mandatory['package'].split()[0]
        existing=next((c for c in result if re.search(r'\b'+re.escape(frontend)+r'\b',c.get('package',''),re.I) and (
            (c.get('source_path')==mandatory['source_path'] and mandatory['evidence'] in c.get('evidence','').splitlines()) or
            (c.get('consumer_path')==mandatory['source_path'] and mandatory['evidence'] in c.get('consumer_evidence','').splitlines()))),None)
        if existing is None:missing.append(dict(mandatory,required_frontend=True))
        else:
            existing['required_frontend']=True
            existing['probe']=mandatory['probe']+'\nAdditional source-derived check: '+existing.get('probe','')
    return missing+result

def contracts(files):
    result=[]
    for path,text in files.items():
        if not path.startswith('solution/') or not path.endswith('.sh'):continue
        lines=text.splitlines()
        for line in lines:
            if line.lstrip().startswith('#'):continue
            match=re.search(r'\bpython(?:\d+(?:\.\d+)*)?\s+-m\s+(poetry|build)\b',line)
            if not match or (match[1]=='poetry' and not re.search(r'\bbuild\b',line)):continue
            frontend=match[1]
            install=next((x for x in lines if 'pip install' in x and frontend in x),line)
            result.append(dict(package=frontend+' build frontend',source_path=path,evidence=line,
                relevance_path=path,relevance_evidence=install,consumer_path=path,consumer_evidence=line,
                consumer_requirement='The reference invokes this build frontend to produce an installable package; its actual build protocol must work with the selected tool versions.',
                contract='The explicitly invoked '+frontend+' build frontend must execute its build hooks with the dependency selections made in this source script. Check this independently of runtime library APIs.',
                probe='In an isolated temporary environment, reproduce the source script\'s build-tool installation constraints, record all selected versions, and invoke this exact frontend on a minimal empty package with the same declared build-hook protocol. First read the actual upstream pyproject.toml and build hook from the staged source evidence. Preserve the build-system requirements AND the hook registration (a build.py file alone does not register a hook). Print the reduced configuration and an unmistakable marker from INSIDE the invoked hook; a wheel without that marker does not exercise the contract. Preserve the relevant imports and backend calls while replacing expensive compilation only, and label that substitution. Record versions inside the build environment, not just the outer interpreter; preserve explicit unpinned upgrades in the source. Preserve the frontend/backend distinction and interpreter differences. Do not build the task application or compile its large libraries. Do not disable indexes or add constraints absent from the source. Capture the complete exception and failing import if the tiny build fails.'))
    return result[:2]
