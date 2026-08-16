# Container live-eval capability contract

Branch: `v0.4-container-capability-gating`.

Workshop refuses `POST /rollouts/prepare` unless the registered record
explicitly advertises the normalized live-eval protocol. This file is the
service-side half of that contract: what a pool must publish, and the exact
GameBench change the Craftax ReAct Luna pool needs.

## Protocol

`GET /info` (or the `/metadata` fallback) returns a `capabilities` block:

```json
{
  "capabilities": {
    "protocol": "synth.container.live-eval.v1",
    "operations": {
      "rollouts.prepare": true,
      "rollouts.start_prepared": true,
      "rollouts.get": true,
      "rollouts.poll": true,
      "reward.get": true,
      "trace_v5.capture": false
    },
    "policy_refs": [
      {
        "harness": "react",
        "config": "luna_low",
        "model": "gpt-5.6-luna",
        "auth": "chatgpt-codex"
      }
    ]
  }
}
```

Workshop projects this into `container.metadata.capabilities`, adding
`observed_at`, `source`, and `complete`, and serves it from `container_list`,
`container_get`, and `container_probe`.

### Rules

- Support is **tri-state**: `supported`, `unsupported`, `unknown`. An operation
  the block omits is `unknown`, and `unknown` fails preflight closed.
- An operation is never inferred. Task family, endpoint name, a successful
  `/health`, `rollout_stream_sse`, and any other transport advertisement map to
  nothing. `trace_v5.capture` in particular means the start path accepts a real
  capture context and produces a sealable artifact; SSE does not imply it.
- `policy_refs` describe which policies exist. They never imply an operation,
  and an operation never implies a policy.
- Workshop never issues a mutating request to discover support.

### Sources, in precedence order

| `source` | Meaning |
|---|---|
| `info` | The service's own normalized block from `/info` or `/metadata`. |
| `metadata` | An operator declaration in `config.toml` (see below), for a known-good pool that predates service-side advertisement. |
| `compatibility` | Mapped from a well-known **explicit** advertisement: a `capabilities.operations` / `operations` object, or a `features` / `routes` / `endpoints` array naming an operation outright (`"rollouts.prepare"`, `"POST /rollouts/prepare"`). A bare `/rollouts` route maps to nothing. |
| `none` | Nothing was advertised. Every operation is `unknown`. |

### No caller may assert its own capabilities

`container_register.metadata` arrives through an agent-callable MCP tool. A
capability claim there would let an agent register an incompatible raw engine
while declaring every operation supported — defeating this gate entirely. So
registration metadata is never read as a capability source: `capabilities`,
`declaredCapabilities`, and `declared_capabilities` are **stripped** from the
metadata map before the record is stored, and the host-computed projection is
written in their place. Unrelated caller metadata is preserved.

### Health

Readiness is HTTP status **and** payload. A service that answers
`200 {"ok": false}` — or `{"healthy": false}`, `{"ready": false}`, or
`{"status": "unhealthy"}` — is recorded `unhealthy`, because a record that read
`ready` would pass the health half of preflight. Only an explicit negative
demotes a record; an unfamiliar payload stays `ready`, so this cannot invent
failures for services that report nothing. Registration, probe, and the Tauri
hydration path share one interpretation.

### Freshness

A record must have been probed within `limits::CONTAINER_CAPABILITY_MAX_AGE`
(15 minutes) or prepare returns `container_capabilities_stale`. The window is
deliberately wider than `CONTAINER_METADATA_REFRESH` (5 minutes) so a record
merely due for its next `/info` refresh is not reported as stale. Reusing a
cached `/info` body preserves the earlier `observed_at`: a health-only refresh
cannot launder a stale capability observation into a fresh one.

## Required GameBench change

The intended pool source is:

```text
gamebench: tasks/craftax-singleplayer/containers/react/craftax_singleplayer_container.py
```

Required source revision: `be0ad8a9e1977ca73b0e5c56b9195947b6d40c36`
(`feat(craftax): add normalized live Luna rollout contract`), image
`synth-containers 0.4.0.20260730`.

At that revision the service implements `POST /rollouts/prepare` and enforces
`react:luna_low`, but `/info` and `/metadata` advertise only
`liveEval.policyRefs`. Workshop therefore projects `source: "none"` and every
operation as `unknown`, and prepare fails closed with
`container_capability_mismatch`.

**This change belongs in the GameBench repository, not in this branch.** Add the
block below to both `GET /info` and `GET /metadata` in
`craftax_singleplayer_container.py`, keeping the existing `liveEval.policyRefs`
for older readers:

```python
"capabilities": {
    "protocol": "synth.container.live-eval.v1",
    "operations": {
        "rollouts.prepare": True,
        "rollouts.start_prepared": True,
        "rollouts.get": True,
        "rollouts.poll": True,
        "reward.get": True,
        # Flip to True only once the start path accepts a real
        # trace_context and seals a Trace V5 artifact.
        "trace_v5.capture": False,
    },
    "policy_refs": [
        {
            "harness": "react",
            "config": "luna_low",
            "model": "gpt-5.6-luna",
            "auth": "chatgpt-codex",
        }
    ],
},
```

Note that `/metadata` already carries an unrelated `capabilities` object
(`async_rollout`, `checkpoint_resume`, …). Workshop only treats a block as
normalized when it names the protocol or carries an `operations` object, so the
existing keys must be merged into the same object rather than replaced.

## Until the pool ships it

An operator — the person at the keyboard, not an agent — may declare the pool in
`config.toml`. That file is written only by Tauri commands and is unreachable
from the loopback IPC the MCP adapters speak, so this authority cannot be
reached from a session:

```toml
[[containers.capability_declaration]]
base_url = "http://127.0.0.1:8104"
protocol = "synth.container.live-eval.v1"
operations = { "rollouts.prepare" = true, "rollouts.start_prepared" = true, "rollouts.get" = true, "rollouts.poll" = true, "reward.get" = true, "trace_v5.capture" = false }
policy_refs = [{ harness = "react", config = "luna_low" }]
```

Entries are matched on `base_url` (trailing slash ignored) and applied at
register and probe. The record then reports `source: "metadata"`, which is
honest about who made the claim. A service-side block always wins over an
operator declaration, so shipping the GameBench patch supersedes this entry
without needing it removed.

## Failure codes

All three cross MCP as tool failures (`isError: true`), never as a successful
result carrying an `error` field.

| Code | Retryable | Remediation |
|---|---|---|
| `container_unhealthy` | yes | Start or repair that registered pool, then `container_probe`. |
| `container_capabilities_stale` | yes | Call `container_probe` before preparing. |
| `container_capability_mismatch` | no | Read `missing` and `available_policy_refs`; select a compatible registered target or stop and report `compatible_runtime_unavailable`. |

None of them is a reason to probe another port, register a new record, switch to
a raw engine, change the policy, or substitute prior evidence.
