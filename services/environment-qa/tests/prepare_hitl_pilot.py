"""Stage hash-verified TBench tasks for real, pending, rules-only human review.

No provider calls, Docker trials, gold import, or automated human decisions.
"""
import argparse
import hashlib
import json
import shutil
from pathlib import Path
from environment_qa.bundles import export_bundle
from environment_qa.core import Store

parser = argparse.ArgumentParser()
parser.add_argument("--corpus", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--cases", nargs="+", default=["case-01", "case-05"])
args = parser.parse_args()
manifest = json.loads((args.corpus / "manifest.json").read_text())
cases = {c["id"]: c for c in manifest["cases"]}
store = Store(args.output / "store")
receipts = []
for case_id in args.cases:
    case = cases[case_id]
    source = (args.corpus / case["task_path"]).resolve()
    if not source.is_relative_to(args.corpus.resolve()):
        raise ValueError("Source escapes corpus")
    task = args.output / "snapshots" / case_id
    for name, expected in case["files"].items():
        original = source / name
        target = task / name
        if (original.is_symlink() or not original.resolve().is_relative_to(source)
                or not target.resolve().is_relative_to(task.resolve())):
            raise ValueError("Invalid snapshot path")
        if hashlib.sha256(original.read_bytes()).hexdigest() != expected:
            raise ValueError(f"Source drift: {case_id}/{name}")
        if target.exists():
            if hashlib.sha256(target.read_bytes()).hexdigest() != expected:
                raise ValueError(f"Existing pilot snapshot drift: {case_id}/{name}")
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(original, target)
    bundle = export_bundle(task, store.root, [args.output / "snapshots"])
    if bundle["files"] != case["files"]:
        raise ValueError("Staged snapshot differs from source manifest")
    run = store.create(bundle, mode="hitl", reviewer="rules", probes=False,
                       request_key=f"hitl-workshop-pilot:{case_id}",
                       overlay=f"{case_id} / {case['family']}: real TBench source-only HITL pilot. No Codex or runtime execution; not full QA validation. Leave approval to the user.")
    receipts.append({"case_id":case_id, "family":case["family"], "run_id":run["id"],
                     "snapshot_sha256":bundle["sha256"], "source_commit":case["commit"]})
receipt = {"scope":"real TBench source-only HITL pilot", "provider_calls":0,
           "automated_human_decisions":0, "cases":receipts}
(args.output / "pilot.json").write_text(json.dumps(receipt, indent=2)+"\n")
print(json.dumps(receipt, indent=2))
