"""Conditional ownership checks for repeated clients of exclusive consoles.

No benchmark IDs or reference labels. This deliberately recognizes only a
small literal shell/Expect subset; it does not prove process survival.
"""
import re
import ast
from .review import finding

QEMU_REFERENCE = 'https://www.qemu.org/docs/master/system/invocation.html'


def host(value):
    return 'loopback' if value in {'localhost', '127.0.0.1'} else value


def verifier_sessions(path, text, order):
    """Recognize an Expect literal written by Python and an Expect invocation."""
    try:tree=ast.parse(text)
    except SyntaxError:return []
    literals={node.targets[0].id:node.value.value for node in ast.walk(tree)
              if isinstance(node,ast.Assign) and len(node.targets)==1 and isinstance(node.targets[0],ast.Name)
              and isinstance(node.value,ast.Constant) and isinstance(node.value.value,str)}
    written={node.args[0].id for node in ast.walk(tree) if isinstance(node,ast.Call) and isinstance(node.func,ast.Attribute)
             and node.func.attr=='write' and node.args and isinstance(node.args[0],ast.Name)}
    invocation=None
    for node in ast.walk(tree):
        if not isinstance(node,ast.Call) or not node.args:continue
        if not isinstance(node.func,ast.Attribute) or node.func.attr not in {'popen','run','call','check_output','check_call'}:continue
        argument=node.args[0]
        command=argument.value if isinstance(argument,ast.Constant) and isinstance(argument.value,str) else ''
        if isinstance(argument,(ast.List,ast.Tuple)) and all(isinstance(v,ast.Constant) and isinstance(v.value,str) for v in argument.elts):
            command=' '.join(v.value for v in argument.elts)
        if re.match(r'^\s*expect\s+-f\s+',command):invocation=ast.get_source_segment(text,node);break
    if not invocation:return []
    sessions=[]
    for name in sorted(written & literals.keys()):
        body=literals[name]
        variables=dict(re.findall(r'^\s*set\s+(\w+)\s+["\']?([\w.:-]+)["\']?\s*$',body,re.M))
        for line in body.splitlines():
            match=re.match(r'^\s*spawn\s+telnet\s+(\$?\w[\w.:-]*)\s+(\$?\w[\w.-]*)\s*$',line)
            quote=line.strip()
            if not match or quote not in text:continue
            values=[variables.get(value[1:],'') if value.startswith('$') else value for value in match.groups()]
            sessions.append({'endpoint':(host(values[0]),values[1]),'path':path,'line':text[:text.index(quote)].count('\n')+1,
                             'evidence':quote,'invocation':invocation,'order':order,'cleanup':False})
    return sessions


def check(path, text, verifiers=None):
    if not re.search(r'^\s*qemu-system-[\w-]+\b', text, re.M):
        return []
    servers = {}
    for match in re.finditer(r'-serial\s+(?:mon:)?(?:tcp|telnet):([^:\s,]*):(\d+),([^\s]+)', text):
        if not any(flag in {'server', 'server=on'} for flag in match[3].split(',')):
            continue
        servers[(host(match[1]), match[2])] = match.group(0)
    if not servers:
        return []
    lines = text.splitlines(keepends=True)
    sessions = []
    index = 0
    while index < len(lines):
        header = lines[index].strip()
        delimiter = re.search(r"<<-?\s*['\"]?(\w+)", header) if header.startswith('cat ') else None
        target = re.search(r'>\s*([\w./-]+)', header) if delimiter else None
        if not delimiter or not target:
            index += 1
            continue
        start = index + 1
        end = start
        while end < len(lines) and lines[end].strip() != delimiter[1]:
            end += 1
        if end == len(lines):
            break
        body = ''.join(lines[start:end])
        variables = dict(re.findall(r'^\s*set\s+(\w+)\s+["\']?([\w.:-]+)["\']?\s*$', body, re.M))
        invocation = re.search(r'^\s*expect\s+-f\s+' + re.escape(target[1]) + r'\s*$', text, re.M)
        for offset, line in enumerate(lines[start:end]):
            spawn = re.match(r'^\s*spawn\s+telnet\s+(\$?\w[\w.:-]*)\s+(\$?\w[\w.-]*)\s*$', line)
            if not spawn or not invocation:
                continue
            values = [variables.get(value[1:], '') if value.startswith('$') else value for value in spawn.groups()]
            endpoint = (host(values[0]), values[1])
            suffix = ''.join(lines[start + offset + 1:end])
            cleanup = bool(re.search(r'^\s*(?:close|wait)\b', suffix, re.M))
            cleanup |= bool(re.search(r'^\s*expect\s+"telnet>"', suffix, re.M) and
                            re.search(r'^\s*send\s+"quit\\r"', suffix, re.M))
            sessions.append(dict(endpoint=endpoint, path=path, line=start + offset + 1, evidence=line.rstrip('\r\n'),
                                 invocation=invocation.group(0).strip(), order=invocation.start(), cleanup=cleanup))
        index = end + 1
    for verifier_path,verifier_text in (verifiers or {}).items():
        sessions.extend(verifier_sessions(verifier_path,verifier_text,len(text)+1))
    sessions.sort(key=lambda session: session['order'])
    findings = []
    for index, first in enumerate(sessions):
        if first['path']!=path or first['endpoint'] not in servers or first['cleanup']:
            continue
        second = next((s for s in sessions[index + 1:] if s['endpoint'] == first['endpoint'] and s['order'] > first['order']), None)
        if not second:
            continue
        claim = ('The first console controller has no visible explicit client close before a later controller '
                 'connects to the same single-client QEMU serial endpoint. If that spawned client survives '
                 'controller exit on the execution backend, it retains the only connection and blocks the '
                 'later console operation; dependent setup and verification can then fail.')
        item = finding('resource_lifecycle', 'warning', 'Unreleased first console client can block the next console operation',
                       path, first['line'], first['evidence'], 'exclusive_console_client_ownership')
        item.update(causal_claim=claim,
                    failure_condition='The first spawned client remains connected after its Expect controller exits. Backend process/PTY cleanup behavior has not been reproduced.',
                    affected_behavior='The later console client cannot perform its commands while the first owns the exclusive endpoint.',
                    supporting_evidence=[{'path':path, 'evidence':servers[first['endpoint']]},
                                         {'path':path, 'evidence':first['invocation']},
                                         {'path':second['path'], 'evidence':second['evidence']},
                                         {'path':second['path'], 'evidence':second['invocation']}],
                    protocol_reference=QEMU_REFERENCE,
                    evidence_grade='conditional_source')
        findings.append(item)
        if len(findings) == 3:
            break
    return findings
