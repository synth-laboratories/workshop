"""CUA fixture: real service and child protocol process, explicitly no model calls."""
import json
import os
import sys
from pathlib import Path

from environment_qa.bundles import export_bundle
from environment_qa.codex_executor import fake_launcher
from environment_qa.core import Store
from environment_qa.dispatch import request_json
from environment_qa.policy import full_policy, validate
from environment_qa.server import serve
from environment_qa import executors

root, source = map(lambda p: Path(p).resolve(), sys.argv[1:3])
kind = sys.argv[3] if len(sys.argv) > 3 else "permission"
if kind not in {"permission","clarification"}: raise ValueError("Unknown fixture kind")
store = Store(root / "store")
policy = full_policy()
policy["nodes"] = [{"id":"fixture-permission", "executor":"review", "role":"specification", "required":True, "depends_on":[]}]
policy["id"] = "qa-cua-fixture-not-live"
policy["interaction_timeout_seconds"] = 600
policy.pop("sha256", None)
policy = validate(policy)
run = store.create(export_bundle(source, store.root, [source]), mode="hitl", reviewer="ai", budget_usd=1,
    pipeline=policy, surface="test",
    overlay="CUA TEST FIXTURE — fake app-server, zero provider calls. Not task quality evidence.", request_key="cua-"+kind+"-fixture")
os.environ.update(QA_INPUT_USD_PER_MILLION="0.2", QA_OUTPUT_USD_PER_MILLION="1.2", QA_TOKEN_BUDGET="")
answer = {"findings":[], "limitations":["Fake protocol fixture only; no task quality conclusion"]}
request = ({"__approval__":{"method":"item/commandExecution/requestApproval", "params":{"command":"fixture-only-no-command-is-executed", "availableDecisions":["accept","decline"]}}}
           if kind == "permission" else {"__request__":{"method":"item/tool/requestUserInput", "params":{"threadId":"fixture", "turnId":"turn-1", "itemId":"q", "questions":[{"id":"scope","header":"Scope","question":"What is this fixture allowed to establish?"}]}}})
launcher = fake_launcher({"events":[request,
    {"method":"item/completed", "params":{"item":{"id":"result", "type":"agentMessage", "text":json.dumps(answer)}}},
    {"method":"turn/completed", "params":{"turn":{"id":"turn-1", "status":"completed"}}}
]})

def fixture_execute(store, run, gate, path):
    schema = {"type":"object", "required":["findings","limitations"], "properties":{"findings":{"type":"array","items":{"type":"object"}}, "limitations":{"type":"array","items":{"type":"string"}}}}
    return request_json(store, run["id"], gate["id"], [{"role":"user","content":"Protocol fixture only"}],
                        attempt_token=gate["attempt"], response_schema=schema, launcher=launcher)

executors.execute_gate = fixture_execute
print(f"CUA FIXTURE run={run['id']} model_calls=0", flush=True)
serve(store.root, [source], 7339)
