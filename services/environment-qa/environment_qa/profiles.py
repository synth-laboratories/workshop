"""The three versioned QA profiles, and the only way to launch one.

A profile is a named, versioned pairing of a DAG with an interaction policy. It
exists so a launch names *what QA to do* rather than assembling a pipeline at the
call site, and so the run records which one it was under a hash that covers both.

The two TBench profiles share one DAG deliberately. Interaction gates already
resolve differently by mode -- a human decides in `hitl`, the run proceeds under
policy in `automated` -- so expressing the pair as two DAGs would let them drift
apart while claiming to audit the same thing. K3 is a different question (does the
verifier accept what it should and reject what it should not), so it gets its own
DAG while sharing the execution and evidence primitives.

Selection is closed. An unknown profile is refused rather than falling back to a
default: the old selector treated an unrecognised value as "not a pipeline" and
silently produced a legacy rules-only run, which looks like a completed QA pass.
"""
from __future__ import annotations

from .policy import full_policy, targeted_policy, validate

PROFILE_VERSION = "1.0.2"


def k3_policy():
    """Verifier audit: does the grader accept what it should and refuse what it should not?

    The candidates are the measurement. A known-good solution that is rejected is a
    false reject; a no-op or a shortcut that is accepted is a false accept. Both are
    verifier defects, and neither is visible from reading the verifier alone, which
    is why this profile spends its budget on running candidates rather than on more
    source review.
    """
    nodes = []

    def node(id, executor, dependencies=(), **options):
        nodes.append(dict(id=id, executor=executor, depends_on=list(dependencies), required=True, **options))

    node("admission", "admission")
    node("structure", "structure", ["admission"])
    node("acceptance-contract", "review", ["admission"], role="verifier")
    node("build", "trial", ["structure"], mode="nop")

    # Known good must be accepted, and accepted the same way twice: a verifier that
    # accepts intermittently is not a verifier.
    node("known-good", "trial", ["build"], mode="oracle")
    node("known-good-repeat", "trial", ["build"], mode="oracle-repeat")
    # A no-op must be refused. If it is accepted, the task grades nothing.
    node("broken-candidate", "trial", ["build"], mode="nop")

    node("candidate-plan", "plan", ["acceptance-contract"])
    node("candidate-approval", "interaction", ["candidate-plan"], role="probe_approval")
    # A shortcut earns the reward without satisfying the intent.
    node("shortcut-candidate", "agent_trial", ["build", "candidate-approval"], mode="cheat",
         plan_gate="candidate-plan")

    node("false-accept-analysis", "review",
         ["known-good", "known-good-repeat", "broken-candidate", "shortcut-candidate"],
         role="trajectory", allow_inconclusive_dependencies=True)
    node("verifier-attribution", "review", ["false-accept-analysis", "acceptance-contract"],
         role="attribution", allow_inconclusive_dependencies=True)
    node("critic", "review", ["verifier-attribution"], role="critic", allow_inconclusive_dependencies=True)
    node("technical-review", "interaction", ["critic"], role="technical_review",
         allow_inconclusive_dependencies=True)
    node("disposition", "disposition", ["technical-review"], allow_inconclusive_dependencies=True)

    return validate({"id": "environment-qa-k3", "version": "1.0.3", "nodes": nodes,
                     "max_parallel": 4, "trial_timeout_seconds": 900, "agent_steps": 12,
                     "interaction_timeout_seconds": 86400, "backend": "docker",
                     "model": "openai/gpt-5.6-luna", "allow_automated_release": False,
                     "reasoning_effort": "high", "agent_reasoning_effort": "medium"})


PROFILES = {
    "tbench-hitl": {
        "version": PROFILE_VERSION, "mode": "hitl", "build": targeted_policy,
        "description": "Task audit with persisted human review checkpoints and clarification."},
    "tbench-non-hitl": {
        "version": PROFILE_VERSION, "mode": "automated", "build": targeted_policy,
        "description": "The same task-audit DAG with no planned human checkpoints."},
    "k3-non-hitl": {
        "version": "1.0.3", "mode": "automated", "build": k3_policy,
        "description": "Verifier audit over known-good, broken and shortcut candidates."},
}


class UnknownProfile(ValueError):
    """The requested profile is not one this build advertises."""


def advertise():
    """What `/api/config` publishes. Version is pinned, never derived at launch."""
    return [{"id": name, "version": profile["version"], "mode": profile["mode"],
             "description": profile["description"]}
            for name, profile in sorted(PROFILES.items())]


def resolve(profile_id, mode=None):
    """Return (mode, policy) for a profile, or raise.

    A supplied mode must agree with the profile's own. Silently correcting a
    mismatch would let a caller ask for a HITL audit and receive an unattended one
    whose findings nobody reviewed.
    """
    profile = PROFILES.get(profile_id)
    if profile is None:
        raise UnknownProfile(
            f"Unknown QA profile {profile_id!r}. This build advertises: {', '.join(sorted(PROFILES))}.")
    if mode is not None and mode != profile["mode"]:
        raise ValueError(
            f"Profile {profile_id!r} runs in {profile['mode']!r} mode; {mode!r} was requested.")
    policy = profile["build"]()
    # Identity travels inside the hash, so a receipt naming a profile cannot be
    # paired with a policy from a different one.
    policy = validate(dict(policy, profile=profile_id, profile_version=profile["version"]))
    return profile["mode"], policy


def describe(policy):
    """Profile identity recorded on a run, for receipts and the UI."""
    return {"profile": policy.get("profile"), "profile_version": policy.get("profile_version"),
            "policy_sha256": policy.get("sha256"), "dag": policy.get("id"),
            "gates": len(policy.get("nodes", []))}


# Legacy selector values kept readable, so old runs still open. They are not
# profiles and cannot be launched once profiles are required.
LEGACY_PIPELINES = {"full": full_policy, "targeted": targeted_policy}
