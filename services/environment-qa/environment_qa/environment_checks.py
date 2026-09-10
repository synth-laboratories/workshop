"""Conservative source checks. Backend conditions are explicit, never observations."""
import ast
import re
import itertools
import operator
import tomllib
import shlex
from .review import finding

def allocation_lower_bounds(text):
    """Small bounded C-expression analyzer; unsupported expressions stay unknown."""
    values={}; found=[]
    operations={ast.Add:operator.add,ast.Sub:operator.sub,ast.Mult:operator.mul,
                ast.BitAnd:operator.and_,ast.LShift:operator.lshift}
    def evaluate(node,env):
        if isinstance(node,ast.Constant) and type(node.value) in {int,float}: return node.value
        if isinstance(node,ast.Name): return env[node.id]
        if isinstance(node,ast.BinOp) and type(node.op) in operations:
            left,right=evaluate(node.left,env),evaluate(node.right,env)
            if isinstance(node.op,ast.LShift) and not 0<=right<=40: raise ValueError('shift')
            result=operations[type(node.op)](left,right)
            if abs(result)>10**16: raise ValueError('bound')
            return result
        if isinstance(node,ast.Compare) and len(node.ops)==1:
            ops={ast.Gt:operator.gt,ast.GtE:operator.ge,ast.Lt:operator.lt,ast.LtE:operator.le,ast.Eq:operator.eq}
            return int(ops[type(node.ops[0])](evaluate(node.left,env),evaluate(node.comparators[0],env)))
        raise ValueError('unsupported')
    def possibilities(expr):
        expr=re.sub(r'(?<=\d)(?:ULL|LL|UL|L|U)\b','',expr)
        try:
            node=ast.parse(expr,mode='eval').body
            names=sorted({n.id for n in ast.walk(node) if isinstance(n,ast.Name)})
            ranges=[values[n] for n in names]
            if len(names)>8 or __import__('math').prod(len(v) for v in ranges)>256: return None
            return sorted({evaluate(node,dict(zip(names,v))) for v in itertools.product(*ranges)})
        except (ValueError,SyntaxError,KeyError,TypeError,OverflowError): return None
    for number,line in enumerate(text.splitlines(),1):
        # Only standalone scalar assignments, never execute the source.
        match=re.fullmatch(r'\s*([A-Za-z_]\w*)\s*=\s*(.+);\s*',line)
        if not match: continue
        name,expr=match.groups()
        malloc=re.fullmatch(r'malloc\((.*)\)',expr)
        if malloc:
            sizes=possibilities(malloc.group(1))
            if sizes and min(sizes)>0: found.append((number,line,min(sizes)))
            values.pop(name,None);continue
        possible=possibilities(expr)
        if possible is None:
            mask=re.search(r'&\s*(\d+)\s*$',expr)
            if mask and int(mask[1])<=15: possible=list(range(int(mask[1])+1))
        if possible is None: values.pop(name,None)
        else: values[name]=possible
    return found


def check_environment(path,allocation_sink=None):
    findings=[]
    docker=path/'environment/Dockerfile'
    copied={}
    if docker.exists():
        for line in docker.read_text(errors='replace').splitlines():
            if not line.startswith('COPY ') or '[' in line or '--' in line: continue
            try: parts=shlex.split(line)[1:]
            except ValueError: continue
            if len(parts)!=2 or not parts[1].startswith('/'): continue
            source=path/'environment'/parts[0]
            if not source.resolve().is_relative_to((path/'environment').resolve()): continue
            if source.is_dir():
                for child in source.rglob('*'):
                    if child.is_file() and child.resolve().is_relative_to((path/'environment').resolve()): copied[parts[1].rstrip('/')+'/'+str(child.relative_to(source))]=child
            if source.is_file():
                target=parts[1]
                if target.endswith('/') or '.' not in target.rsplit('/',1)[-1]: target=target.rstrip('/')+'/'+source.name
                copied[target]=source
        for target,source in copied.items():
            if source.suffix!='.py': continue
            text=source.read_text(errors='replace')
            try: tree=ast.parse(text)
            except SyntaxError: continue
            for node in ast.walk(tree):
                if not isinstance(node,ast.Constant) or not isinstance(node.value,str) or not node.value.startswith('/') or not node.value.endswith('.py'): continue
                alternatives=[p for p in copied if p.rsplit('/',1)[-1]==node.value.rsplit('/',1)[-1]]
                if node.value not in copied and alternatives:
                    quote=ast.get_source_segment(text,node)
                    findings.append(finding('instruction_verifier_alignment','warning',
                        f'Image-provided helper {target} references {node.value}, but Docker COPY provisions that script at {alternatives}; verifier-time uploads do not establish availability during the agent self-test phase',
                        str(source.relative_to(path)),node.lineno,quote,'agent_selftest_path_staging_mismatch'))
    config=tomllib.loads((path/'task.toml').read_text()) if (path/'task.toml').exists() else {}
    environment=config.get('environment',{})
    # Collected for the task-configuration lane, which anchors on the declaration
    # rather than on the allocating line.
    observed_minimums=allocation_sink if allocation_sink is not None else []
    budget=environment.get('memory_mb',0)*1024**2
    if not budget:
        memory=str(environment.get('memory',''))
        match=re.fullmatch(r'(\d+(?:\.\d+)?)\s*([GM])(?:i?B)?',memory,re.I)
        if match: budget=float(match[1])*(1024**3 if match[2].upper()=='G' else 1024**2)
    for file in sorted(path.rglob('*')):
        if not file.is_file() or file.suffix not in {'.py','.sh'}: continue
        text=file.read_text(errors='replace'); relative=str(file.relative_to(path))
        if budget:
            for number,line,minimum in allocation_lower_bounds(text):
                observed_minimums.append((relative,number,minimum))
                if minimum>budget:
                    claim=(f'Static allocation estimate is at least {minimum:g} bytes under the tracked scalar '
                           f'assignments, exceeding the declared {budget:g}-byte memory limit')
                    item=finding('resource_validity','warning',claim,relative,number,line,'allocation_exceeds_declared_memory')
                    # The caveat belongs to the check's confidence, never to the claim: a matcher
                    # reads titles as declared allegations, and "swap or overcommit" named there
                    # reads as an assertion of a masking mechanism this check never measured.
                    item.update(causal_claim=claim,
                        failure_condition='Requires the allocating path to execute and the runtime to enforce the declared limit. Control flow and touched pages were not traced.',
                        affected_behavior='Allocation can fail or be killed where the declared limit is enforced.',
                        limitations=['This static check does not measure resident memory, swap, or overcommit, and therefore does not establish whether any backing-store or permissive-accounting mechanism masks the shortfall.'])
                    findings.append(item)
        if file.suffix=='.py':
            try: tree=ast.parse(text)
            except SyntaxError: continue
            comparators=set()
            for fn in (n for n in ast.walk(tree) if isinstance(n,ast.FunctionDef)):
                for assertion in (n for n in ast.walk(fn) if isinstance(n,ast.Assert)):
                    test=assertion.test
                    if not isinstance(test,ast.Compare) or len(test.ops)!=1 or not isinstance(test.ops[0],ast.Eq): continue
                    left,right=test.left,test.comparators[0]
                    if (isinstance(left,ast.Call) and isinstance(right,ast.Call) and
                        isinstance(left.func,ast.Name) and isinstance(right.func,ast.Name) and
                        left.func.id==right.func.id and 'hash' in left.func.id.lower()): comparators.add(fn.name)
            if relative.startswith('tests/'):
                for fn in (n for n in ast.walk(tree) if isinstance(n,ast.FunctionDef) and n.name.startswith('test')):
                    constants={n.targets[0].id:n.value.value for n in ast.walk(fn) if isinstance(n,ast.Assign) and len(n.targets)==1 and isinstance(n.targets[0],ast.Name) and isinstance(n.value,ast.Constant) and isinstance(n.value.value,str)}
                    for call in (n for n in ast.walk(fn) if isinstance(n,ast.Call) and isinstance(n.func,ast.Name) and n.func.id in comparators and len(n.args)==2):
                        values=[n.value if isinstance(n,ast.Constant) else constants.get(n.id) if isinstance(n,ast.Name) else None for n in call.args]
                        for target in values:
                            if target in copied and len(set(values))==2:
                                findings.append(finding('solution_leakage','warning',
                                    f'Verifier uses {target} as an answer-equivalence reference and Docker COPY stages that file in the image; if setup retains agent read access, copying it can reveal the expected output without the intended work',
                                    relative,call.lineno,ast.get_source_segment(text,call),'image_staged_answer_reference'))
            for node in ast.walk(tree):
                if isinstance(node,ast.Assert) and isinstance(node.test,ast.Compare):
                    names={n.id for n in ast.walk(node.test) if isinstance(n,ast.Name)}
                    if any('speedup' in n.lower() for n in names) and any(isinstance(op,(ast.Gt,ast.GtE,ast.Lt,ast.LtE)) for op in node.test.ops):
                        findings.append(finding('performance_portability','warning',
                            'Fixed speedup acceptance threshold can change with interpreter/library baseline performance and host timing; verify calibration under the supported runtime versions before treating a failed ratio as an implementation defect',
                            relative,node.lineno,ast.get_source_segment(text,node),'speedup_threshold_runtime_sensitive'))
                if not isinstance(node,ast.Call) or not isinstance(node.func,ast.Attribute): continue
                if not isinstance(node.func.value,ast.Name) or node.func.value.id!='os' or node.func.attr not in {'rename','replace'} or len(node.args)<2: continue
                values=[a.value if isinstance(a,ast.Constant) and isinstance(a.value,str) else None for a in node.args[:2]]
                if not all(values) or values[0]==values[1]: continue
                # Separate directory roots can be separate mount points. This
                # does not assert that the current Docker layout has that property.
                roots=[v.split('/')[1] if v.startswith('/') else '<cwd>' for v in values]
                if roots[0]==roots[1]: continue
                quote=ast.get_source_segment(text,node)
                findings.append(finding('backend_portability','warning',
                    'Rename requires a shared filesystem: this move raises EXDEV if the source and destination are on separate mounts',
                    relative,node.lineno,quote,'rename_requires_same_filesystem'))
        for line_no,line in enumerate(text.splitlines(),1):
            if re.match(r'^\s*ulimit\s+-[a-zA-Z]*c\s+unlimited\s*(?:#.*)?$',line):
                findings.append(finding('backend_portability','warning',
                    'Requesting unlimited core dumps fails when the runtime hard core-size limit is capped; verify whether the script propagates that failure',
                    relative,line_no,line,'core_limit_exceeds_runtime_hard_limit'))
    return findings
