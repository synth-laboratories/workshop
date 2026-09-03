# GEPA refuses every container over a field it does not read

**Date:** 2026-09-03
**Impact:** container-backed GEPA on every task. No GEPA cell can start.
**Fix location:** `optimizers` repo (`~/GitHub/optimizers`), not Workshop.
**Status:** fixed and verified in 0.2.20 — see [Outcome](#outcome). GEPA now
clears this check and stops at the next one, which is still open.

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

## Outcome

The fix shipped as `synth-optimizers` 0.2.20 and is verified in production.

Run `gepa_gepa_banking77_qa_v1_3f4f6b07` executed from
`data/optimizers/versions/0.2.20/runtime` and cleared the
`capabilities.metadata` validation that had killed every previous attempt,
failing instead at the *next* statement of the same preflight function:

```
File ".../versions/0.2.20/runtime/.../synth_optimizers/gepa.py", line 1631
ValueError: container GET /program failed with HTTP 404
```

That progression is the proof: reaching `/program` is only possible once the
metadata check passes.

Getting there took a version cut, because Workshop installs the sidecar by
pinned version and seals it with a digest and an instance-local signature.
Three defects sat between the fix and a run, each hiding the next:

1. **The floor was not a floor.** `ensure_ready` installed the pinned sidecar
   only when *nothing* was installed, never comparing the installed version
   against `min_supported`. Raising the pin shipped a wheel no existing
   instance would ever install: a run on a binary built from the 0.2.20 commit
   reported `algorithmVersion: synth-optimizers-0.2.19` and failed at the
   preflight 0.2.20 fixes.
2. **The pin was restated where it could drift.** `manager.rs` holds the
   embedded distribution's source revision and lock sha; the staging script
   restated both. Bumping only the script staged 0.2.20 while the app still
   demanded `686f41c4`, and the install failed with `embedded Optimizers
   distribution does not match the release pin`.
3. **Idempotency returned a dead run as a start.** A rerun mapped to the
   already-terminal failed run and never started the sidecar at all, so the
   run reported zero rollouts without executing anything. `optimizer_evaluation_start`
   documents the remedy: a new key for an intentional rerun.

All three are fixed in `6bbae6bb`.

## The next gate: `GET /program`

`_preflight_container_capabilities` validates `ProgramPayload` from the
container's `/program` route immediately after the metadata check. No evals
image serves it:

```
curl -o /dev/null -w '%{http_code}' http://127.0.0.1:8099/program   # 404
```

This is a second contract requirement the images do not implement, and unlike
the first it is not an over-strict validator — GEPA genuinely needs the
program definition it is asked to rewrite. Closing it means implementing the
route in the container images, which is work in the `evals` repo rather than a
loosened check here.

## Loose end

The run mirror records `algorithmVersion: synth-optimizers-0.2.19` while the
stderr log proves `versions/0.2.20/runtime` executed. A record naming a
version that did not produce it is the same class of provenance defect the
container revision gates exist to catch, and it is worth fixing before any
evidence is read back with trust.
