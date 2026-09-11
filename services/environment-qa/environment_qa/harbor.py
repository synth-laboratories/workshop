"""Bounded, zero-provider-cost Harbor oracle/no-op adapter for local Docker."""
import json
import os
import shutil
import subprocess
import tempfile
import hashlib
import re
import tomllib
from pathlib import Path
from .review import finding


def component_complete(events):
    if not events: return False
    last=events[-1]; action=last.get('action',{})
    if action.get('done') is not True: return False
    if action.get('command','').strip() and last.get('observation',{}).get('return_code')!=0: return False
    return any(not e.get('instrumentation') and e.get('observation',{}).get('return_code')==0 for e in events)


def probe(store, run, path, agents=None, gate_id=None, timeout_seconds=600):
    executable = shutil.which("harbor")
    if not executable:
        raise ValueError("Harbor is not installed")
    # Compose can grant host mounts/privileges; the prototype admits the simple
    # Harbor Dockerfile format only. Extend with an explicit backend policy later.
    from .admission import validate_compose
    validate_compose(path)
    work = store.root / "trials" / run["id"]
    work.mkdir(parents=True, exist_ok=True)
    evidence = []
    findings = []
    limits = []
    with tempfile.TemporaryDirectory(prefix="qa-harbor-") as temp:
        home = Path(temp)
        (home / "docker").mkdir()
        # Preserve executable plugin discovery without copying Docker's auth
        # config or credential-store settings into the isolated HOME.
        plugins = home / "docker" / "cli-plugins"
        plugins.mkdir()
        for name in ("docker-compose", "docker-buildx"):
            binary = shutil.which(name)
            if not binary:
                candidate = Path(shutil.which("docker") or "/usr/bin/docker").parent.parent / "lib/docker/cli-plugins" / name
                binary = str(candidate) if candidate.is_file() else None
            if binary:
                (plugins / name).symlink_to(Path(binary).resolve())
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(home),
               "DOCKER_CONFIG": str(home / "docker"), "NO_COLOR": "1"}
        # Never inherit provider credentials, Harbor auth, or Keychain helpers.
        if os.environ.get("DOCKER_HOST"):
            env["DOCKER_HOST"] = os.environ["DOCKER_HOST"]
        if agents and any(a in {"negative","alternative","frontier","cheat"} for a in agents):
            env["PYTHONPATH"] = str(Path(__file__).parent.parent)
            env["QA_STORE"] = str(store.root)
            env["QA_RUN_ID"] = run["id"]
            env["QA_GATE_ID"] = gate_id
            env["QA_GATE_ATTEMPT"] = next(g["attempt"] for g in run["gates"] if g["id"] == gate_id)
            for name in ("QA_PROVIDER_URL","QA_PROVIDER_MODEL","QA_PROVIDER_KEY","OPENROUTER_API_KEY",
                         "QA_CODEX_APP_SERVER","QA_INPUT_USD_PER_MILLION","QA_OUTPUT_USD_PER_MILLION",
                         "QA_MAX_APP_SERVERS"):
                if name in os.environ:
                    env[name] = os.environ[name]
        if run['policy'].get('pipeline', {}).get('backend') == 'daytona':
            if not os.environ.get('DAYTONA_API_KEY'):
                raise ValueError('Daytona QA requires an authorized runtime DAYTONA_API_KEY')
            env['DAYTONA_API_KEY'] = os.environ['DAYTONA_API_KEY']
        version = subprocess.check_output([executable, "--version"], env=env, timeout=20, text=True).strip()
        for agent in (agents or ("oracle", "nop", "oracle-repeat")):
            if store.get(run["id"])["status"] in {"cancelling", "cancelled"}:
                limits.append("Harbor probes cancelled before dispatch")
                break
            job = "qa-" + run["id"][:16] + "-" + (gate_id or agent)
            # Execute a private copy so Harbor cannot mutate the sealed bundle.
            task = home / agent / "task"
            shutil.copytree(path, task)
            transformation = None
            if agent == "oracle-repeat":
                # Metamorphic probe: verification should not invalidate an
                # unchanged correct solution by leaving its own build artifacts.
                original = task / "tests/test.sh"
                original.rename(task / "tests/qa-original-test.sh")
                wrapper = '''#!/bin/bash
rm -f /logs/verifier/reward.txt
bash /tests/qa-original-test.sh
first=$(cat /logs/verifier/reward.txt 2>/dev/null)
rm -f /logs/verifier/reward.txt
bash /tests/qa-original-test.sh
second=$(cat /logs/verifier/reward.txt 2>/dev/null)
printf '%s\\n%s\\n' "$first" "$second" > /logs/verifier/qa-repeat.txt
'''
                original.write_text(wrapper)
                transformation = {"kind": "repeat_verifier.v1", "wrapper_sha256": hashlib.sha256(wrapper.encode()).hexdigest(),
                                  "changed_paths": ["tests/test.sh", "tests/qa-original-test.sh"]}
                if run['policy'].get('pipeline',{}).get('task_aware_deadlines'):
                    config_path = task/'task.toml'
                    config_text = config_path.read_text()
                    original_timeout = tomllib.loads(config_text)['verifier']['timeout_sec']
                    section = re.search(r'(?ms)^\[verifier\]\s*\n(.*?)(?=^\[|\Z)',config_text)
                    if not section: raise ValueError('Missing verifier configuration')
                    body,count = re.subn(r'(?m)^timeout_sec\s*=.*$',f'timeout_sec = {2*original_timeout}',section.group(1))
                    if count != 1: raise ValueError('Ambiguous verifier timeout')
                    config_path.write_text(config_text[:section.start(1)]+body+config_text[section.end(1):])
                    transformation.update(verifier_timeout_before=original_timeout,verifier_timeout_after=2*original_timeout,
                                          task_config_sha256=hashlib.sha256(config_path.read_bytes()).hexdigest())
                    transformation['changed_paths'].append('task.toml')
            selected_agent = "environment_qa.harbor_agent:QaAgent" if agent in {"negative","alternative","frontier","cheat"} else "oracle" if agent == "oracle-repeat" else agent
            env["QA_TRIAL_MODE"] = agent
            command = [executable, "run", "--path", str(task), "--agent", selected_agent,
                       "--env", "docker", "--jobs-dir", str(work), "--job-name", job,
                       "--n-concurrent", "1", "--n-attempts", "1", "--max-retries", "0", "--delete"]
            selected_gate=next((g for g in run.get('gates',[]) if g['id']==gate_id),{})
            component=False
            if selected_gate.get('executor')=='targeted_trial':
                plan=next(e['result'] for e in run['evidence'] if e['gate']=='probe-plan')
                component=plan['experiments'][selected_gate['slot']].get('execution_kind')=='component'
                if component: command.append('--disable-verification')
            log_path = work / ((gate_id or agent) + ".log")
            pipeline = run['policy'].get('pipeline', {})
            native_outcome = None
            if pipeline.get('native_image'):
                if version != '0.22.0':
                    raise ValueError('Native QA requires Harbor 0.22.0')
                from .native_runtime import run_native_probe
                native_outcome, log_path = run_native_probe(
                    task=task, work=work, job=job, agent=selected_agent, environment=env,
                    pipeline=pipeline, timeout_seconds=timeout_seconds, executable=executable, component=component,
                    cancelled=lambda: store.get(run["id"])["status"] in {"cancelling", "cancelled"},
                )
                returncode = native_outcome['execution_returncode']
                execution_error = native_outcome['execution_error']
                interrupted = execution_error is not None
            else:
                from .shared_process import run_probe_process
                returncode, interrupted, execution_error = run_probe_process(
                    command, output=log_path, environment=env, cwd=home,
                    timeout_seconds=timeout_seconds,
                    max_output_bytes=pipeline.get('process_output_max_bytes', 16 * 1024 * 1024),
                    cancelled=lambda: store.get(run["id"])["status"] in {"cancelling", "cancelled"},
                )
            results = []
            for candidate in (work / job).glob("*/result.json"):
                data = json.loads(candidate.read_text())
                rewards = (data.get("verifier_result") or {}).get("rewards")
                results.append({"trial": candidate.parent.name, "rewards": rewards,
                                "exception": (data.get("exception_info") or {}).get("exception_type")})
            entry = {"agent": agent, "harbor_version": version, "exit_code": returncode,
                     "interrupted": interrupted, "execution_error": execution_error, "results": results, "log": str(log_path.relative_to(store.root)),
                     "transformation": transformation}
            if native_outcome is not None:
                entry['resource_cleanup'] = native_outcome['cleanup']
                entry['qualification_status'] = 'development_candidate_unqualified'
                if native_outcome['cleanup'].get('cleanup_status') != 'confirmed':
                    limits.append('Native provider absence remains unconfirmed')
            evidence.append(entry)
            entry['execution_kind']='component' if component else 'full_verifier'
            if component:
                entry['notice']='Component observation only; full task grader was deliberately not run.'
                trajectories=list((work/job).glob('*/agent/qa-trajectory.json'))
                try: events=json.loads(trajectories[0].read_text()) if len(trajectories)==1 else []
                except (ValueError,OSError): events=[]
                observed=any('observation' in e and not e.get('instrumentation') for e in events)
                if interrupted or returncode or len(results)!=1 or results[0]['exception'] or not observed or not component_complete(events):
                    limits.append('Component experiment incomplete or missing observations; no grade inferred')
                continue
            if interrupted or returncode or len(results) != 1 or results[0]["exception"] or not results[0]["rewards"]:
                limits.append(f"Harbor {agent} did not produce one valid trial; inspect saved logs and resources")
                continue
            rewards = results[0]["rewards"]
            reward = rewards.get("reward")
            if not isinstance(reward, (float, int)):
                limits.append(f"Harbor {agent}: unsupported reward schema")
            elif agent == "nop" and reward > 0:
                findings.append(finding("verifier_validity", "blocking", "No-op agent receives reward", "tests/test.sh", 1,
                                        json.dumps(entry), "no_op_receives_reward"))
            elif agent == "oracle" and reward <= 0:
                findings.append(finding("reproducibility", "warning", "Reference solution receives no reward", "solution/solve.sh", 1,
                                        json.dumps(entry), "oracle_fails_verifier"))
            if agent == "oracle-repeat":
                repeats = list((work / job).glob("*/verifier/qa-repeat.txt"))
                try:
                    first, second = [float(x) for x in repeats[0].read_text().splitlines()] if len(repeats) == 1 else (None, None)
                except ValueError:
                    first, second = None, None
                entry["repeat_rewards"] = [first, second]
                if first is None or second is None:
                    limits.append("Repeat-verifier probe did not produce two scalar rewards")
                elif first > 0 and second <= 0:
                    findings.append(finding("reproducibility", "blocking", "Repeating verification invalidates the same solution",
                                            "tests/test.sh", 1, json.dumps(entry), "repeat_verifier_changes_reward"))
        if agents is None:
            limits.append("Bounded oracle/no-op/repeated-verifier probes; general adversarial agent trials and independent cleanup reconciliation are not yet qualified")
    return {"findings": findings, "limitations": limits, "trials": evidence,
            "gate_status": "inconclusive" if len(limits) > (1 if agents is None else 0) else "succeeded"}
