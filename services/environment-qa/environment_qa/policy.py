"""Versioned executable QA DAGs. Policy is trusted configuration, never task data."""
from copy import deepcopy
import re
from .core import digest


def full_policy():
    nodes = []
    def node(id, executor, dependencies=(), **options):
        nodes.append(dict(id=id, executor=executor, depends_on=list(dependencies), required=True, **options))
    node("admission", "admission")
    node("structure", "structure", ["admission"])
    node("specification", "review", ["admission"], role="specification")
    node("verifier", "review", ["admission"], role="verifier")
    node("build", "trial", ["structure"], mode="nop")
    for i in range(3):
        node(f"oracle-{i+1}", "trial", ["build"], mode="oracle")
    node("noop", "trial", ["build"], mode="nop")
    node("repeat-verifier", "trial", ["build"], mode="oracle-repeat")
    node("probe-plan", "plan", ["specification", "verifier"], rounds=2)
    node("probe-approval", "interaction", ["probe-plan"], role="probe_approval")
    for kind in ("negative", "alternative", "frontier", "cheat"):
        node(kind, "agent_trial", ["build", "probe-approval"], mode=kind)
    trials = [n["id"] for n in nodes if n["executor"] in {"trial", "agent_trial"}]
    node("trajectory", "review", trials, role="trajectory", allow_inconclusive_dependencies=True)
    node("targeted-plan", "plan", ["trajectory"], rounds=1, allow_inconclusive_dependencies=True)
    node("targeted-approval", "interaction", ["targeted-plan"], role="probe_approval")
    node("targeted-cheat", "agent_trial", ["build", "targeted-approval"], mode="cheat", plan_gate="targeted-plan")
    node("targeted-analysis", "review", ["targeted-cheat", "trajectory"], role="trajectory", allow_inconclusive_dependencies=True)
    node("attribution", "review", ["targeted-analysis", "specification", "verifier"], role="attribution", allow_inconclusive_dependencies=True)
    node("critic", "review", ["attribution"], role="critic", allow_inconclusive_dependencies=True)
    node("technical-review", "interaction", ["critic"], role="technical_review", allow_inconclusive_dependencies=True)
    node("domain-review", "interaction", ["attribution"], role="domain_review", allow_inconclusive_dependencies=True)
    node("disposition", "disposition", ["technical-review", "domain-review"], allow_inconclusive_dependencies=True)
    return validate({"id": "environment-qa-full", "version": "2.0.0", "nodes": nodes,
                     "max_parallel": 4, "trial_timeout_seconds": 1800, "agent_steps": 24,
                     "interaction_timeout_seconds": 86400, "backend": "docker", "model": "openai/gpt-5.6-luna",
                     "allow_automated_release": False})


def targeted_policy():
    """Source-led QA: at most two justified probes, no blanket reference solves."""
    policy = full_policy()
    keep = {'admission', 'structure', 'specification', 'verifier', 'probe-plan',
            'probe-approval', 'trajectory', 'attribution', 'critic',
            'technical-review', 'domain-review', 'disposition'}
    policy['nodes'] = [n for n in policy['nodes'] if n['id'] in keep]
    by_id = {n['id']: n for n in policy['nodes']}
    specialists=['environment-review','boundary-review']
    for id,role in zip(specialists,['environment','boundaries']):
        policy['nodes'].append(dict(id=id,executor='review',depends_on=['admission'],role=role,required=True))
    for original in list(policy['nodes']):
        if original['executor']=='review' and original.get('role') in {'specification','verifier','environment','boundaries'}:
            replica=dict(original,id=original['id']+'-independent')
            policy['nodes'].append(replica)
            specialists.append(replica['id'])
    scopes = {
        'specification': 'Instruction-to-test requirements and hidden constraints only.',
        'specification-independent': 'Reference-solution-to-instruction contradictions only; trace identities, paths and prerequisites.',
        'verifier': 'False acceptance: concrete invalid outputs, bypasses, leaked artifacts and missing decisive assertions only.',
        'verifier-independent': 'False rejection: concrete valid alternatives and test isolation or stale output only.',
        'environment-review': 'Dependencies only: trace direct AND transitive packages, build tools, imported APIs and version constraints in the reference solution. For supplied Makefiles, trace producer-consumer prerequisites under parallel make, including generated directories, headers and libraries. Identify minimal metadata/API probes for unverified compatibility. Do not substitute generic network availability complaints.',
        'environment-review-independent': 'Resource and process lifecycle only: trace every opened session/socket/child process through ownership, explicit cleanup and the NEXT connection or repeated invocation. Also derive allocation and time lower bounds. Do not discuss hidden verifier requirements.',
        'boundary-review': 'Input semantics only: derive concrete lexical/numeric/encoding counterexamples and asymmetric API defaults in the reference implementation.',
        'boundary-review-independent': 'Portability only: filesystem mounts, privilege limits, agent-versus-verifier staging, library-dependent performance baselines. Trace exact deployment conditions, not generic missing test coverage.',
    }
    for node in policy['nodes']:
        if node['id'] in scopes:
            node['review_scope'] = scopes[node['id']]
            node['finding_limit']=4
    # Fixed two-pass coverage, not selecting a favorable run after scoring.
    for node in list(policy['nodes']):
        if node.get('review_scope'):
            replica=dict(node,id=node['id']+'-second-pass')
            if node['id']=='specification':replica['assertion_audit']=True
            policy['nodes'].append(replica)
            specialists.append(replica['id'])
    policy['nodes'].append(dict(id='dependency-inventory',executor='source_inventory',depends_on=['admission'],required=True))
    policy['nodes'].insert(2,dict(id='dependency-contracts',executor='contract_plan',depends_on=['dependency-inventory'],required=True))
    for node in policy['nodes']:
        if node.get('review_scope'): node['depends_on']=['dependency-inventory']
    by_id['probe-plan'].update(executor='targeted_plan', depends_on=['admission','structure','specification','verifier','dependency-contracts',*specialists],
        optional_evidence_dependencies=['specification','verifier','dependency-contracts',*specialists])
    probes = []
    for index in range(2):
        id = f'experiment-{index+1}'
        probes.append(id)
        policy['nodes'].append(dict(id=id, executor='targeted_trial', depends_on=['probe-approval'], required=True, slot=index))
    policy['nodes'].append(dict(id='contract-analysis',executor='contract_analysis',depends_on=[*probes,'dependency-contracts'],required=True,allow_inconclusive_dependencies=True))
    by_id['trajectory']['depends_on'] = [*probes,'contract-analysis']
    by_id['attribution']['depends_on'] = ['trajectory','specification','verifier',*specialists]
    policy.update(id='environment-qa-targeted', version='4.15.0', trial_timeout_seconds=120, agent_steps=8,source_review_passes=2,collect_runtime_limits=True,prepare_source_toolchain=True,measure_build_backends=True,
                  reasoning_effort='high',agent_reasoning_effort='medium')
    return validate(policy)


def validate(policy):
    policy = deepcopy(policy)
    nodes = policy["nodes"]
    ids = [n["id"] for n in nodes]
    if not nodes or len(nodes)>100 or len(ids) != len(set(ids)) or any(not isinstance(i,str) or not re.fullmatch(r"[a-z][a-z0-9-]{0,63}",i) for i in ids):
        raise ValueError("Nonempty DAG with unique gate IDs required")
    allowed = {"admission", "structure", "review", "trial", "plan", "interaction", "agent_trial", "disposition", "targeted_plan", "targeted_trial", "source_inventory", "contract_plan", "contract_analysis"}
    for node in nodes:
        optional=node.get('optional_evidence_dependencies',[])
        if not isinstance(optional,list) or not set(optional)<=set(node['depends_on']):
            raise ValueError('Optional evidence must name direct dependencies')
        if node["executor"] not in allowed or not set(node["depends_on"]) <= set(ids):
            raise ValueError("Unknown executor or dependency")
    seen = set()
    while len(seen) < len(nodes):
        ready = {n["id"] for n in nodes if set(n["depends_on"]) <= seen} - seen
        if not ready: raise ValueError("QA policy contains a cycle")
        seen |= ready
    if not isinstance(policy["max_parallel"], int) or not 1 <= policy["max_parallel"] <= 20:
        raise ValueError("Concurrency must be between 1 and 20")
    if policy["model"] != "openai/gpt-5.6-luna":
        raise ValueError("This policy requires the user-selected Luna model")
    for key in ('reasoning_effort','agent_reasoning_effort'):
        if key in policy and policy[key] not in {'none','low','medium','high','xhigh','max'}:
            raise ValueError('Unsupported Luna reasoning effort')
    for key, maximum in (("trial_timeout_seconds", 43200), ("agent_steps", 200), ("interaction_timeout_seconds", 604800)):
        if not isinstance(policy[key], int) or not 1 <= policy[key] <= maximum:
            raise ValueError("Invalid bounded policy field: " + key)
    if type(policy.get("allow_automated_release")) is not bool: raise ValueError("Explicit automated release policy required")
    policy.pop("sha256", None)
    policy["sha256"] = digest(policy)
    return policy


def gates(policy):
    return [dict(n, status="pending", attempt=None, attempts=[]) for n in policy["nodes"]]
