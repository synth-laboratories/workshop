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

This policy does not grant credential access, import secrets, access Keychain,
install sidecars, replace running containers, or waive visual readiness. Those
require their normal authority. Pre-provision approved proxy credentials and
required sidecars for unattended QA; this feature does not silently acquire them.
