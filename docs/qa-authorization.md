# Isolated QA authorization

QA builds (`eval-driver`) in named `.dev.` bundles can load an operator-installed
`qa-policy.json` from their own data directory. Normal release builds refuse it.
There is no agent-facing enable, edit, or approve endpoint. Ordinary human and
credential approval endpoints remain unchanged.

The profile is trusted operator configuration, not a sandbox against a process
that already has arbitrary local filesystem access. Keep it out of conversation
attachments and do not expose the installer as an agent tool. Creating a new
budget or widening scope requires explicit operator authorization.

Create an explicit profile (use canonical absolute repository paths):

```json
{
  "schema_version": 1,
  "id": "craftax-banking77-qa-20260914",
  "instance": "e2e-v011",
  "expires_at": "2026-09-15T00:00:00Z",
  "container_roots": ["/absolute/path/to/evals"],
  "recipe_roots": ["/absolute/path/to/workshop"],
  "containers": ["craftax-gamebench-rust", "banking77"],
  "recipes": ["qa.banking77.gepa.v1"],
  "inline_evaluation_digests": [],
  "providers": ["openrouter"],
  "proxy_lease_providers": ["openrouter"],
  "max_request_usd_micros": 2450000,
  "max_total_usd_micros": 9800000,
  "max_rollouts": 12
}
```

```sh
SYNTH_QA_PROFILE=/absolute/path/to/profile.json ./scripts/desktop-instance.sh qa-enable e2e-v011
./scripts/desktop-instance.sh qa-status e2e-v011
./scripts/desktop-instance.sh qa-disable e2e-v011
```

The file survives app restarts. Expiry is checked at each admission. Disabling
archives the file and does not erase spending history or cancel an active run.
An already-admitted run retains its original hard ceiling. Profile installation
refuses replacement; disable explicitly before changing the envelope.

In QA mode, project-source discovery uses only the profile roots, not ambient
machine grants. Container launches require an exact source-root and declaration
ID match and retain all normal provenance/readiness checks. Recipe compute must
name an allowed, unambiguous workspace recipe whose container and provider are
also allowed. Inline evaluations require the exact immutable digest returned by
draft admission in `inline_evaluation_digests`; no wildcard evaluation approval.

Each paid request permanently reserves its full dollar ceiling in the instance
database under the profile ID, across conversations and restarts. Retries consume
additional allowance. Cancellation, missing telemetry, and failed audit writes
do not refund it. The aggregate cap is strictly below $50. Keep the profile ID
stable across restarts and revisions; a new ID represents a new operator budget,
not a retry mechanism. Audit receipts identify `qa_policy`, never a human click.

`proxy_lease_providers` is optional and defaults to empty. Explicit opt-in permits
the recipe execution path to authorize one bounded proxy lease for the same
provider, conversation, recipe and freshly reserved compute approval. Each claim
is consumed atomically and audited as `qa_eval_proxy_lease`. Missing, replayed,
cross-session or out-of-scope requests fail without opening a modal. The opt-in
must exist when the compute reservation is made: changing a profile never
approves an already-pending credential prompt or upgrades an older reservation.

This does not authorize raw credential access, locating/registering new secret
sources, importing secrets, Keychain access, sidecar installation, container
replacement, or fabricated visual readiness. Pre-provision the approved secrets
proxy and required sidecars. Ordinary sessions and generic credential approval
endpoints retain their existing consent behavior. An absent/revoked source,
expired policy, or missing capability remains a structured failure, not a reason
to acquire new credentials or silently fall back to a different provider.
