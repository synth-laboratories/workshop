# Container lifecycle state-machine handoff

## Current temporary behavior

`container_restart` is explicitly destructive. It first attempts a receipt-verified stop. Whether or not such a receipt exists, it then re-runs the exact `synth.container-launch.v1` command and waits for the declared health identity. The declaration is responsible for replacing its named workload. Workshop never selects a process merely because it listens on the declared port.

NanoHorizon opts into replacement through ordinary workload environment (`REPLACE=1`). This is a property of its launcher, not Workshop ownership policy. Provider credentials remain outside the process and are available only through the Workshop secrets proxy after bounded approval.

This behavior is intentionally permissive and should be replaced by the state machine below.

## Required authorities

Keep these facts independent:

1. **Declaration identity** — immutable launch schema, command, cwd, image reference, endpoint, health identity, allowed environment names, source revision, dirty digest and included source files.
2. **Observed workload identity** — health identity, protocol, image/container identity, source revision, policy revision, endpoint and observation time.
3. **Management authority** — Workshop instance policy and user approval. This never belongs in the portable container manifest.
4. **Process receipt** — launcher PID/start identity plus the actual workload identity (container runtime ID or equivalent), boot epoch, launch digest and timestamps.
5. **Execution authority** — paid-compute receipt, credential capability, exact policy/model pins, bounds and revocation state. It must not imply process-management authority.

## States

- `undeclared`
- `declared_unverified`
- `source_mismatch`
- `stopped`
- `starting`
- `healthy_external_read_only`
- `healthy_managed`
- `adoption_pending`
- `stopping`
- `replacing`
- `unhealthy`
- `identity_mismatch`
- `receipt_stale`
- `failed`

Do not collapse `healthy_external_read_only` into `healthy_managed`. Health proves liveness and identity evidence; it does not grant authority.

## Events and legal transitions

| Event | From | To | Required evidence |
|---|---|---|---|
| `declaration.loaded` | `undeclared` | `declared_unverified` | valid v1 declaration digest |
| `source.verified` | `declared_unverified` | `stopped` or healthy state | exact revision or declared dirty digest |
| `source.rejected` | `declared_unverified` | `source_mismatch` | expected/actual revision and digest |
| `launch.requested` | `stopped` | `starting` | instance authorization and declaration digest |
| `health.observed` | `starting` | `healthy_managed` | matching receipt and health identity |
| `health.observed` | any non-managed state | `healthy_external_read_only` | health identity without valid receipt |
| `adoption.requested` | `healthy_external_read_only` | `adoption_pending` | exact observed workload identity |
| `adoption.approved` | `adoption_pending` | `healthy_managed` | persistent instance approval and adopted workload receipt |
| `stop.requested` | `healthy_managed` | `stopping` | current receipt and instance authority |
| `replace.requested` | `healthy_managed` | `replacing` | current receipt, declaration digest and authority |
| `receipt.invalidated` | `healthy_managed` | `receipt_stale` | PID/runtime identity mismatch |
| `health.mismatch` | any active state | `identity_mismatch` | expected and observed identities |
| timeout/process failure | `starting`, `stopping`, `replacing` | `failed` | typed failure code and last observation |

Every other transition is illegal and must return a typed error. No fallback transition may manufacture ownership, reuse a stale receipt, infer identity from a port, or convert missing evidence into success.

## Persistent metadata

Store instance management policy separately from project declarations, keyed by canonical project source plus container ID. Persist authorization mode, approval receipt, approver, timestamps and optional expiry. Store process/workload receipts independently and invalidate them atomically when process identity, runtime container ID, source digest or launch digest changes.

The event journal should record declaration digest, previous/current state, typed event, actor, receipt references, expected/observed identities, error code and monotonic sequence. Secrets and raw credentials must never appear in any record.

## Required typed errors

- `launch_declaration_missing`
- `launch_declaration_invalid`
- `launch_source_mismatch`
- `container_external_read_only`
- `container_adoption_required`
- `container_management_not_authorized`
- `container_receipt_stale`
- `container_identity_mismatch`
- `container_start_failed`
- `container_stop_failed`
- `container_replace_failed`
- `container_readiness_timeout`

## Acceptance tests

Test managed launch/restart across app restart; external healthy discovery without stop authority; explicit adoption approval and persistence; PID reuse; stale container-runtime ID; occupied port with mismatched health; source drift; dirty-digest drift; concurrent restart requests; crash during each transitional state; readiness timeout; refusal to use provider credentials during lifecycle work; and proof that paid-compute approval neither grants nor changes management authority.
