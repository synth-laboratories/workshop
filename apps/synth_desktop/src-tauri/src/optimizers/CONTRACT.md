# Local Optimizer Runtime Contract v1

What Desktop requires of a local algorithm runtime, and — more usefully — what
it must not require. `gepa` conforms today; `eval` is expected to follow.

This exists because the v0.4 Craftax GEPA acceptance was blocked for a full
investigation cycle by a gap nothing in the system could name: the host gated
readiness on `GET /v1/optimizer/capabilities`, and `synth-optimizers` had never
implemented that route on any branch. Every symptom pointed somewhere else.

## 1. Who owns which fact

The rule the rest of this document follows: **a runtime answers only for
itself, and host vocabulary is never round-tripped through a runtime.**

| Fact | Owner | Where it lives |
| --- | --- | --- |
| `algorithms` | runtime | capability handshake |
| `replay`, `cancellation` | runtime | handshake, derived from its own routes |
| environment readiness | runtime | handshake (e.g. eval's container runtime) |
| `recipes` | **host** | `contract/runtimes.rs` |
| `compatibleTemplateIds` | **host** | `contract/runtimes.rs` |
| version pins, floor, compat | **host** | `contract/runtimes.rs` |

Recipe ids and visual template ids are Desktop's. They appear nowhere in the
plugin, which only ever knew them because Desktop's install payload supplied
them. Asking a runtime to echo them back proved only that it had been told what
to say, and would have forced a plugin release for every new host template.

The same rule read in the other direction: **the host must not answer on a
runtime's behalf.** A handshake the host serves from its own constants is a
digest of a host constant, and the anti-swap pin then attests nothing.

## 2. Required routes

| Route | Purpose |
| --- | --- |
| `GET /health` | liveness only — never carries capability facts |
| `GET /v1/optimizer/capabilities` | runtime-owned facts + `contractVersion` |
| `GET /runs/{run_id}/optimizer-events?after_sequence=&limit=` | cursor-resumable event pages |
| `POST /runs/{run_id}/cancel` | required iff `cancellation: true` |

Transport is loopback HTTP behind the host-owned auth proxy, with a host-issued
bearer token. The proxy is a three-route allowlist, not a general proxy.

## 3. Derivation

Capability values are derived from the runtime's own route table or an
environment probe. Never hand-written literals.

A literal drifts from what the service actually serves, and a `replay: true`
that is true only by accident of branch lineage is worth nothing. Derive
`cancellation` from the cancel route existing and `replay` from the events
route existing, and the answer stays honest by construction.

## 4. Errors

Structured, never collapsed. Preserve the upstream status.

- Route-missing, unreachable, and malformed-response are three distinct faults.
  Reporting them identically is what cost the original investigation its first
  pass — "upgrade the plugin" and "restart the service" looked the same.
- A non-JSON body must not rewrite the status. A plain-text 404 reported as 502
  turns a retryable condition fatal on the first poll.
- `route_not_found` and `run_not_found` mean opposite things for a live run.
  A runtime that conflates them (0.2.5 does) forces the host to assume the
  retryable reading and fall back to the bound in §6.
- Redact credentials from every error and log.

## 5. Versions

One table, `contract/runtimes.rs`. Everything else imports from it.

- The floor is enforced at install/select. It is install-time UX — it explains
  a refusal before a download rather than after a failed handshake. **The
  handshake is the gate.**
- Floors are per channel. The host pins a different version per channel, so one
  floor says nothing about the other.
- Version comparison is numeric per segment: `0.2.10` outranks `0.2.9`.
- A runtime Desktop does not install cannot be held to a version. Say
  "unmanaged" rather than print a number nobody can substantiate.

## 6. Spend guarantees

Two distinct promises. Collapsing them is how the second one went missing.

- **Zero spend when the runtime is unavailable.** Pre-spawn, enforced by the
  ready check and the handshake.
- **Bounded spend when the handshake passes but events cannot be polled.**
  Post-spawn, enforced by `OPTIMIZER_RUN_INDEX_WAIT`. Without it, a run the
  polled service can never see is waited out until the child exits on its own:
  every rollout paid for, every event unreachable, failure delivered at the end.

## 7. Lifecycle

- Single-flight start; the host owns the child's process group.
- Config files are written **after** ready and removed on stop/abort. A config
  file is never readiness evidence.
- The stored handshake is lifecycle state, not durable configuration. It is
  cleared on uninstall and version-select, so a previous install's attestation
  cannot vouch for whatever is installed next.
- No fixed ports; no bind-then-drop-then-rebind handoff.

## 8. Evidence

Every runtime ships its spec at `/openapi.yaml`, and a parity test asserts
router ⊆ spec.

**Test doubles are generated from the contract table and that spec.** This is
the load-bearing rule of the whole document. Every optimizer-manager test runs
against an in-process stand-in that hand-served `/v1/optimizer/capabilities` —
precisely the endpoint the real plugin did not implement. The suite was green
and the product was broken, and the stand-in was the reason.

Two corollaries learned the hard way:

- A stand-in may **withhold** what the real artifact would not provide; it must
  never **invent** what the artifact cannot do. Making it less generous exposes
  failures; making it more generous hides them.
- Anything a stand-in cannot express is not thereby proven. It belongs to the
  real-artifact contract test, which runs against the installed wheel with no
  opt-in env gate.

## 9. What this document is not

It is Desktop's requirements, not evidence about any runtime. Everything in
`contract/runtimes.rs` is a host-side claim. The only statements with
evidentiary weight are the ones a runtime makes about itself over the
handshake — which is why §1 is the first section and not the last.
