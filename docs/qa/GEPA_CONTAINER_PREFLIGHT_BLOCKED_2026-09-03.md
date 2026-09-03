# GEPA refuses every container over a field it does not read

**Date:** 2026-09-03
**Impact:** container-backed GEPA on every task. No GEPA cell can start.
**Fix location:** `optimizers` repo (`~/GitHub/optimizers`, branch `v0.7`), not Workshop.

## Symptom

`gepa.banking77.qa.v1` starts, leases a credential, and dies before the first
rollout:

```
pydantic_core._pydantic_core.ValidationError: 1 validation error for ContainerMetadataPayload
capabilities.metadata
  Field required [type=missing, ...]
```

Run `gepa_gepa_banking77_qa_v1_9e94b7db`: created 14:28:39, failed 14:28:40,
zero rollouts, zero cost. The credential lease was issued and revoked within a
second, so the failure is genuinely at preflight.

## Cause

`synth_optimizers/gepa.py` declares the container metadata block as required:

```python
class ContainerCapabilitiesPayload(BaseModel):
    metadata: ContainerCapabilityMetadataPayload   # required
```

and then reads it on exactly one branch, forty lines later:

```python
if self.policy is None and not metadata.capabilities.metadata.policy_ready:
```

`gepa.banking77.qa.v1` configures its own policy (`harness = "classify"`,
`policy_config = "classify"`), so `policy_ready` has no bearing on the run. The
unconditional validation refuses it anyway.

No container serves the block. Both the QA python server on `:8099` and the
Docker image on `:18110` advertise only `operations`, `policy_refs`, and
`protocol`, on `/info` and `/metadata` alike. So this is not a gap in one
image — it refuses every container that exists today.

## Fix

Applied to `~/GitHub/optimizers/src/synth_optimizers/gepa.py`, **uncommitted**,
on branch `v0.7` alongside ten other dirty files:

```python
class ContainerCapabilityMetadataPayload(BaseModel):
    policy_ready: bool = False

class ContainerCapabilitiesPayload(BaseModel):
    metadata: ContainerCapabilityMetadataPayload = Field(
        default_factory=ContainerCapabilityMetadataPayload
    )
```

Verified against the payload the containers actually serve:

| Input | `policy_ready` |
|---|---|
| capabilities without a `metadata` block | `False` |
| `{"metadata": {"policy_ready": true}}` | `True` |

Defaulting to *not ready* keeps the branch that reads the field failing closed,
with its own accurate message. The alternative — making every container emit a
field that is almost never read — treats the symptom.

## Why this is not fixed on the instance

The instance runtime is a versioned, digest- and signature-verified install
(`data/optimizers/versions/0.2.19/`, `load_verified_manifest`, `sign_manifest`).
Hand-patching `0.2.19` in place would either fail verification or require
signing the forgery with the instance's own key, and any run against it would
record `algorithmVersion: synth-optimizers-0.2.19` for bytes that are not
0.2.19. That is the same provenance lie the container revision gates exist to
prevent.

So this needs a version bump and a package build that Workshop installs and
verifies. It is a release step, not something to patch around.

## Related

The pipeline itself is unaffected and was exercised by this failure: the run
produced 22 captures with 0 failures and a report that states the run failed,
claims no uplift, and reports `0 allocated, 0 scored` rather than presenting
silence as health.
