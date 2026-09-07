"""Deterministic verifier assertion coverage; no task-specific defect rules."""
import ast

def assertions(files,limit=80):
    result={};omitted=0
    for path,text in sorted(files.items()):
        if not path.startswith('tests/') or not path.endswith('.py'):continue
        try:tree=ast.parse(text)
        except SyntaxError:continue
        for node in sorted((n for n in ast.walk(tree) if isinstance(n,ast.Assert)),key=lambda n:n.lineno):
            if len(result)>=limit:omitted+=1;continue
            result['assertion-'+str(len(result)+1).zfill(3)]={'path':path,'line':node.lineno,'evidence':ast.get_source_segment(text,node)}
    return {'assertions':result,'omitted':omitted,'notice':'Every listed acceptance constraint needs an instruction warrant or a necessary consequence. This inventory is not a defect finding.'}
