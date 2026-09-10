"""Individual Harbor trials with retained, hashed runtime evidence."""
import hashlib
import json
import os
import re
import subprocess
import tempfile
from .harbor import probe
RESOURCE_POOL = None

def diagnostic_preview(data):
    """Retain exact diagnostic lines plus the tail, with explicit omission markers."""
    text=data.decode(errors='replace') if isinstance(data,bytes) else data
    if len(text)<=12000: return text
    lines=text.splitlines(keepends=True)
    selected=[]; used=0
    for index,line in enumerate(lines):
        if (re.search(r'(?<![\w-])(?:error|exception|traceback|failed|undefined|killed|checked|summary|version|dist|contract|nodes|keys)\b|not found|no rule to make|timed out',line,re.I)
                or re.search(r'^\s*(?:E\s+)?[\w.]+(?:Error|Exception)[:\s]',line)):
            excerpt=''.join(lines[max(0,index-1):index+3])
            if used+len(excerpt)>7000: break
            selected.append(f'[original lines {max(0,index-1)+1}-{min(len(lines),index+3)}]\n'+excerpt)
            used+=len(excerpt)
    return '[Diagnostic projection; omitted text remains in original hashed artifact]\n'+'\n'.join(selected)+'\n[Original final 4000 characters]\n'+text[-4000:]


def trajectory_preview(data):
    events=json.loads(data)
    if not isinstance(events,list): raise ValueError('Trajectory must be an array')
    preview=[]
    for event in events:
        item=dict(event)
        if 'observation' in item:
            item['observation']=dict(item['observation'])
            for stream in ('stdout','stderr'):
                item['observation'][stream]=diagnostic_preview(item['observation'].get(stream,''))
        preview.append(item)
    return json.dumps({'notice':'All trajectory events retained, with explicit diagnostic output projections when needed. Original artifacts retained. QA source injection is privileged instrumentation, not ordinary agent visibility.',
                      'omitted_earlier_events':0,'events':preview}).encode()


def reconcile_cleanup(trial_names):
    """Reconcile only exact Harbor-owned Compose project labels, never global state."""
    records = []
    with tempfile.TemporaryDirectory(prefix="qa-cleanup-") as config:
        env = {"PATH":os.environ.get("PATH","/usr/bin:/bin"),"DOCKER_CONFIG":config}
        if os.environ.get("DOCKER_HOST"): env["DOCKER_HOST"] = os.environ["DOCKER_HOST"]
        for trial_name in trial_names:
            if not re.fullmatch(r"[A-Za-z0-9_-]+",trial_name): raise ValueError("Invalid trial identity")
            project = (trial_name+"__env").lower()
            resources = {}
            for kind, listing, removal in (("containers",["ps","-aq"],["rm","-f"]),
                                           ("networks",["network","ls","-q"],["network","rm"]),
                                           ("volumes",["volume","ls","-q"],["volume","rm"])):
                command = ["docker",*listing,"--filter","label=com.docker.compose.project="+project]
                ids = subprocess.check_output(command,env=env,text=True,timeout=20).split()
                if ids:
                    subprocess.run(["docker",*removal,*ids],env=env,text=True,capture_output=True,timeout=30,check=True)
                remaining = subprocess.check_output(command,env=env,text=True,timeout=20).split()
                resources[kind] = {"removed":ids,"remaining":remaining}
            records.append({"project":project,"resources":resources,"clean":all(not r["remaining"] for r in resources.values())})
    return records


def trial(store, run, gate, path):
    if RESOURCE_POOL is not None:
        with RESOURCE_POOL.acquire(path,store,run['id']) as receipt:
            result = _trial(store,run,gate,path)
            result['resource_admission']=receipt
            return result
    return _trial(store,run,gate,path)


def _trial(store, run, gate, path):
    from .resources import trial_deadline
    deadline = trial_deadline(path,gate['mode']) if run['policy']['pipeline'].get('task_aware_deadlines') else run['policy']['pipeline']['trial_timeout_seconds']
    result = probe(store,run,path,agents=[gate["mode"]],gate_id=gate["id"],
                   timeout_seconds=min(deadline,run['policy']['pipeline']['trial_timeout_seconds']))
    artifacts = []
    evidence_text = {}
    work = store.root / "trials" / run["id"]
    job = work / ("qa-"+run["id"][:16]+"-"+gate["id"])
    for file in sorted(job.rglob("*")):
        if not file.is_file() or file.is_symlink(): continue
        data = file.read_bytes()
        relative = str(file.relative_to(store.root))
        artifacts.append({"path":relative,"sha256":hashlib.sha256(data).hexdigest(),"bytes":len(data)})
        if file.name in {"result.json","qa-trajectory.json","reward.txt","exception.txt","test-stdout.txt","test-stderr.txt","qa-repeat.txt","oracle.txt","image-visibility.json"}:
            # Test failures occur after dependency-install logs, not at the start.
            tail = file.name in {"test-stdout.txt","test-stderr.txt"}
            excerpt = data[-8000:] if tail else data[:12000]
            if file.name=='qa-trajectory.json':
                try:
                    excerpt=trajectory_preview(data)
                except (ValueError,TypeError): pass
            if file.name=='oracle.txt': excerpt=diagnostic_preview(data).encode()
            evidence_text[relative] = excerpt.decode(errors="replace")
    result.update(artifacts=artifacts, observations=evidence_text)
    result["cleanup"] = reconcile_cleanup([p.name for p in job.iterdir() if p.is_dir() and p.name.startswith("task__")]) if job.is_dir() else []
    if not result["cleanup"] or not all(r["clean"] for r in result["cleanup"]):
        result["gate_status"] = "inconclusive"
        result["limitations"].append("Trial cleanup was not independently established")
    if not artifacts:
        result["gate_status"] = "inconclusive"
        result["limitations"].append("Required runtime artifacts are missing")
    return result
