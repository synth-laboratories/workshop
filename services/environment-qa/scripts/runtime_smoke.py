"""Explicitly bounded live Codex + scoped-tool smoke; not a QA quality score."""
import argparse
import json
import os
import uuid
from pathlib import Path

from dotenv import dotenv_values
from environment_qa.bundles import export_bundle
from environment_qa.core import Store
from environment_qa.dag import claim, complete
from environment_qa.dispatch import SESSIONS, publish, request_json
from environment_qa.harbor_bridge import HarborBridge
from environment_qa.policy import full_policy, validate


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--store", type=Path, required=True)
    parser.add_argument("--env-file", type=Path, required=True)
    parser.add_argument("--codex", required=True)
    parser.add_argument("--request-key", default="live-runtime-smoke-v1")
    parser.add_argument("--budget-usd", type=float, default=.1)
    parser.add_argument("--allowance-id")
    parser.add_argument("--require-tool-submission", action="store_true")
    args = parser.parse_args()
    values = dotenv_values(args.env_file)
    key = values.get("OPENROUTER_API_KEY") or values.get("QA_PROVIDER_KEY")
    if not key: raise SystemExit("Authorized env file contains no OpenRouter credential")
    os.environ.update(OPENROUTER_API_KEY=key, QA_CODEX_APP_SERVER=args.codex,
        QA_INPUT_USD_PER_MILLION="0.2", QA_OUTPUT_USD_PER_MILLION="1.2", QA_TOKEN_BUDGET="")
    store = Store(args.store)
    task = store.root / "runtime-fixture"
    task.mkdir(exist_ok=True)
    (task / "instruction.md").write_text("Benign runtime smoke only, not a benchmark quality task.")
    (task / "task.toml").write_text('version="1.0"\n')
    policy = full_policy()
    policy.update(id="runtime-smoke-not-quality", reasoning_effort="medium", nodes=[
        {"id":"runtime-smoke", "executor":"review", "role":"specification", "required":True,"depends_on":[]}])
    policy.pop("sha256", None)
    run = store.create(export_bundle(task, store.root, [task]), reviewer="ai", budget_usd=args.budget_usd,
                       pipeline=validate(policy), request_key=args.request_key, allowance_id=args.allowance_id)
    claimed = claim(store, run["id"])
    if not claimed: raise SystemExit("Smoke already attempted; inspect its evidence, do not silently retry")
    gate, attempt = claimed
    marker = uuid.uuid4().hex
    bridge = HarborBridge(evidence={"probe":{"marker":marker}})
    schema = {"type":"object", "additionalProperties":False, "required":["marker"],
              "properties":{"marker":{"type":"string"}}}
    try:
        prompt = "Call read_evidence with evidence_id probe, then return its marker in the required JSON schema. Do not guess the marker. No other action is needed."
        if args.require_tool_submission:
            prompt = "Call read_evidence with evidence_id probe, then call submit_result with its exact marker. Both dynamic tool calls are required; a final message alone does not pass this protocol check. Do not guess the marker."
        result = request_json(store, run["id"], gate["id"], [{"role":"user", "content": prompt}],
            attempt_token=attempt, response_schema=schema, bridge=bridge, max_tokens=1024)
        if result.get("marker") != marker: raise ValueError("Scoped evidence marker did not round-trip")
        if args.require_tool_submission:
            executor = SESSIONS._sessions[SESSIONS.key(run["id"], gate["id"], attempt)]
            if not any(r.get("tool") == "submit_result" and r.get("arguments") == result for r in executor.tool_results):
                raise ValueError("No matching dynamic submission was observed")
        complete(store, run["id"], gate, attempt, {"findings":[],"limitations":["Runtime smoke, not task quality"],"runtime_result":result})
        print(json.dumps({"run_id":run["id"],"status":"runtime_smoke_passed"}), flush=True)
    except Exception as error:
        complete(store, run["id"], gate, attempt, {"gate_status":"inconclusive", "findings":[],
            "limitations":["Runtime smoke failed: " + type(error).__name__ + ": " + str(error)[:500]]})
        print(json.dumps({"run_id":run["id"],"status":"runtime_smoke_failed", "error":str(error)[:500]}), flush=True)
        raise SystemExit(1)
    finally:
        for executor in SESSIONS.release_run(run["id"]):
            publish(store, run["id"], executor)


if __name__ == "__main__": main()
