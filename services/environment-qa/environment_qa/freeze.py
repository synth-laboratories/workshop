"""Freeze the engine and profiles, and draft the acceptance manifest.

Two different jobs, deliberately in one place because they are read together.

The freeze is a fact: digests of the engine sources, the client, and each profile's
policy, so a later report can say which build produced it. Both trees are untracked,
so file digests -- not git revisions -- are the only version identity available.

The manifest is not a fact. It is a proposal with the operator's decisions left
empty. Every field the plan requires is present and every numeric threshold is
`null`, because inventing one would manufacture the approval it is supposed to
record. `ready_for_acceptance` stays false while anything required is unset, so a
manifest cannot be mistaken for a signed one.
"""
from __future__ import annotations

import hashlib
import math
import re
import json
from datetime import datetime, timezone
from pathlib import Path

from . import __version__
from .core import digest
from .profiles import PROFILES, resolve

PACKAGE = Path(__file__).parent
WEB = PACKAGE.parent / "web"

# Thresholds the plan requires an operator to declare. Named here so an unset one
# is reported as missing rather than quietly absent.
REQUIRED_THRESHOLDS = (
    "min_reference_recovery",
    "min_reviewed_finding_precision",
    "min_adjudication_coverage",
    "max_false_blocks_on_resolved_controls",
    "min_execution_completion",
    "max_seconds_per_task",
    "max_seconds_per_batch",
)

REQUIRED_COHORT_FIELDS = ("development", "held_out", "task_hashes", "profile_assignments")


def file_digests(paths):
    return {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(paths) if p.is_file()}


def engine_freeze():
    """Digests of everything that decides how a run behaves."""
    sources = file_digests(PACKAGE.glob("*.py"))
    client = file_digests(WEB.glob("*")) if WEB.is_dir() else {}
    profiles = {}
    for name in sorted(PROFILES):
        mode, policy = resolve(name)
        profiles[name] = {"version": PROFILES[name]["version"], "mode": mode,
                          "policy_sha256": policy["sha256"], "dag": policy["id"],
                          "gates": len(policy["nodes"]), "model": policy["model"]}
    return {"schema": "environment-qa.freeze.v2",
            "frozen_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "engine_version": __version__,
            "engine_sha256": digest(sources), "client_sha256": digest(client),
            "sources": sources, "client": client, "profiles": profiles,
            "note": ("Both trees are untracked, so file digests are the only version identity. "
                     "A report citing this freeze names the exact bytes that produced it.")}


def manifest_draft(cohorts=None, accounting=None):
    """An acceptance manifest with the operator's decisions left empty.

    `cohorts` may carry what is already known -- task ids and snapshot hashes -- but
    development/held-out assignment, gold defects and thresholds stay unset until a
    person sets them.
    """
    freeze = engine_freeze()
    cohorts = cohorts or {}
    stages = {}
    for stage, name in (("A", "expanded-tbench"), ("B", "cybernetics"), ("C", "reb")):
        supplied = cohorts.get(name, {})
        stages[stage] = {
            "cohort": name,
            "tasks": supplied.get("tasks", []),
            "task_hashes": supplied.get("task_hashes", {}),
            "development": supplied.get("development"),
            "held_out": supplied.get("held_out"),
            "profile_assignments": supplied.get("profile_assignments"),
            "hitl_paired_subset": supplied.get("hitl_paired_subset"),
            "k3_verifier_subset": supplied.get("k3_verifier_subset"),
            "gold_defects": supplied.get("gold_defects"),
            "controls": supplied.get("controls"),
            "ineligible": supplied.get("ineligible", []),
            "shortfall": None,
        }
    manifest = {
        "schema": "environment-qa.acceptance-manifest.v1",
        "drafted_at": freeze["frozen_at"],
        "freeze": {k: freeze[k] for k in ("engine_version", "engine_sha256", "client_sha256", "profiles")},
        "stages": stages,
        "thresholds": {name: None for name in REQUIRED_THRESHOLDS},
        "accounting": accounting or {"unit": None, "ceiling": None, "rationale": None},
        "runtime": {"model": "openai/gpt-5.6-luna", "runtime": "codex-app-server",
                    "concurrency": None, "per_gate_deadline_seconds": None,
                    "retry_allowance": None, "host_baseline": None},
        "acceptance_owner": None,
        "operator_signoff": None,
        "rules": [
            "Thresholds are declared before execution and never adjusted after results.",
            "AI proposals never become gold; agent-cua decisions are not human adjudication.",
            "Held-out cases become development data for any revision that was tuned against them.",
            "Report ineligible tasks explicitly rather than dropping them after seeing results.",
            "Preserve first-attempt results; never rerun until favourable and call that the original.",
        ],
    }
    manifest["missing"] = missing(manifest)
    manifest["ready_for_acceptance"] = not manifest["missing"]
    return manifest


def missing(manifest):
    """Validate required structure, not just the values a caller left present.

    This checks manifest consistency, not authenticity of human labels/signoff.
    Those require the separate provenance/adjudication process.
    """
    gaps = set()
    if not isinstance(manifest, dict): return ["manifest"]
    def obj(value): return value if isinstance(value, dict) else {}
    def text(value): return isinstance(value, str) and bool(value.strip())
    def number(value, minimum=0): return type(value) in (int, float) and math.isfinite(value) and value >= minimum
    def ids(value): return isinstance(value, list) and all(text(v) for v in value) and len(set(value)) == len(value)
    def sha(value): return isinstance(value, str) and bool(re.fullmatch(r"[0-9a-f]{64}", value))
    thresholds = obj(manifest.get("thresholds"))
    for name in REQUIRED_THRESHOLDS:
        value = thresholds.get(name)
        valid = number(value) and (value > 0 if "seconds" in name else value <= 1)
        if not valid: gaps.add(f"thresholds.{name}")
    accounting = obj(manifest.get("accounting"))
    if accounting.get("unit") not in {"usd", "tokens"}: gaps.add("accounting.unit")
    if not number(accounting.get("ceiling")) or accounting.get("ceiling", 0) <= 0: gaps.add("accounting.ceiling")
    if accounting.get("unit") == "tokens" and type(accounting.get("ceiling")) is not int: gaps.add("accounting.ceiling")
    runtime = obj(manifest.get("runtime"))
    for key, expected in (("model", "openai/gpt-5.6-luna"), ("runtime", "codex-app-server")):
        if runtime.get(key) != expected: gaps.add(f"runtime.{key}")
    for key in ("concurrency", "per_gate_deadline_seconds", "retry_allowance"):
        value = runtime.get(key)
        if type(value) is not int or value < (0 if key == "retry_allowance" else 1): gaps.add(f"runtime.{key}")
    if not text(runtime.get("host_baseline")): gaps.add("runtime.host_baseline")
    stages = obj(manifest.get("stages"))
    for stage, name in (("A", "expanded-tbench"), ("B", "cybernetics"), ("C", "reb")):
        if not isinstance(stages.get(stage), dict): gaps.add(f"stages.{stage}")
        body = obj(stages.get(stage))
        prefix = f"stages.{stage}."
        if body.get("cohort") != name: gaps.add(prefix + "cohort")
        tasks = body.get("tasks")
        if not ids(tasks) or not tasks:
            gaps.add(prefix + "tasks")
            tasks = []
        taskset = set(tasks)
        if stage == "A" and len(tasks) <= 20: gaps.add(prefix + "tasks.expanded")
        parts = []
        for key in ("development", "held_out", "hitl_paired_subset", "k3_verifier_subset"):
            values = body.get(key)
            if not ids(values) or not set(values or []).issubset(taskset) or (key in {"hitl_paired_subset", "k3_verifier_subset"} and not values):
                gaps.add(prefix + key)
                values = []
            if stage == "A" and key in {"development", "held_out"} and not values: gaps.add(prefix + key)
            if key in {"development", "held_out"}: parts.append(set(values))
        if parts[0] & parts[1] or parts[0] | parts[1] != taskset: gaps.add(prefix + "partition")
        hashes = obj(body.get("task_hashes"))
        if set(hashes) != taskset or not hashes or not all(sha(v) for v in hashes.values()): gaps.add(prefix + "task_hashes")
        assignments = obj(body.get("profile_assignments"))
        if set(assignments) != taskset or not assignments: gaps.add(prefix + "profile_assignments")
        for task in tasks:
            profiles = assignments.get(task)
            if not ids(profiles) or "tbench-non-hitl" not in (profiles or []) or not set(profiles or []).issubset(PROFILES):
                gaps.add(prefix + "profile_assignments")
            profiles = profiles if ids(profiles) else []
            for key, profile in (("hitl_paired_subset", "tbench-hitl"), ("k3_verifier_subset", "k3-non-hitl")):
                if task in (body.get(key) if ids(body.get(key)) else []) and profile not in (profiles or []): gaps.add(prefix + key)
        if not isinstance(body.get("gold_defects"), list): gaps.add(prefix + "gold_defects")
        controls = body.get("controls")
        if not isinstance(controls, list) or not controls or not all(isinstance(c, dict) and text(c.get("task_id")) and c.get("task_id") in taskset and text(c.get("provenance")) and text(c.get("label")) for c in controls):
            gaps.add(prefix + "controls")
    if not text(manifest.get("acceptance_owner")): gaps.add("acceptance_owner")
    signoff = obj(manifest.get("operator_signoff"))
    if not text(signoff.get("actor")) or signoff.get("actor") != manifest.get("acceptance_owner") or not text(signoff.get("at")):
        gaps.add("operator_signoff")
    else:
        try: datetime.fromisoformat(signoff["at"].replace("Z", "+00:00"))
        except ValueError: gaps.add("operator_signoff")
    frozen = obj(manifest.get("freeze"))
    current = engine_freeze()
    for key in ("engine_version", "engine_sha256", "client_sha256", "profiles"):
        if frozen.get(key) != current[key]: gaps.add(f"freeze.{key}")
    return sorted(gaps)


def main(argv=None):
    import argparse
    parser = argparse.ArgumentParser(description="Freeze the engine and draft an acceptance manifest.")
    parser.add_argument("--out", type=Path, help="Directory to write freeze.json and manifest.json into.")
    parser.add_argument("--cohorts", type=Path, help="JSON file of known cohort content.")
    parser.add_argument("--validate", type=Path, help="Validate an existing signed manifest against current bytes; writes nothing.")
    args = parser.parse_args(argv)
    if args.validate:
        try:
            gaps = missing(json.loads(args.validate.read_text()))
        except (OSError, ValueError) as error:
            gaps = [f"manifest: {error}"]
        print(json.dumps({"ready_for_acceptance": not gaps, "missing": gaps}, indent=2))
        return 2 if gaps else 0
    cohorts = json.loads(args.cohorts.read_text()) if args.cohorts else None
    freeze, manifest = engine_freeze(), manifest_draft(cohorts)
    if args.out:
        args.out.mkdir(parents=True, exist_ok=True)
        (args.out / "freeze.json").write_text(json.dumps(freeze, indent=2) + "\n")
        (args.out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        print(f"engine {freeze['engine_sha256'][:16]} | profiles {len(freeze['profiles'])} | "
              f"unset manifest fields {len(manifest['missing'])}")
    else:
        print(json.dumps({"freeze": freeze, "manifest": manifest}, indent=2))
    # A drafted manifest is not a signed one, and the exit code says so.
    return 0 if manifest["ready_for_acceptance"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
