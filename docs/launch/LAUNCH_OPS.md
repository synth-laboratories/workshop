# Workshop v0.1 — launch operations

Friends-release (Gate F) and public-launch (Gate P) share one rollback owner, one incident channel, and one immutable artifact. Debug builds, locally copied `.app`s, and fixture-only workflows do not qualify either gate.

## Monitoring

| Signal | Where | Alert if |
|---|---|---|
| Signup / device-init / pairing success | auth + Desktop telemetry (no prompts, no keys) | error rate > 5% over 15 min or JSON device-init regresses to redirect |
| First cloud / local response | Desktop + backend 5xx / timeout | p95 first-token > published budget or 5xx > 2% |
| Checkout `mode=provider` | `/api/v1/billing/checkout-session` | fallback `hosted_web` share spikes; missing `session_id` |
| Autumn track / entitlement read | billing worker + snapshot `degraded` | snapshot `status=unknown` or `degraded` non-empty for paid orgs |
| Gateway settlement | `SmrUsageFact` + outbox lag | zero facts after a successful Responses call; duplicate `request_id`; billed cents ≠ provider receipt beyond rounding |
| Desktop usage ledger | `usage_records.cost_source` | Synth Cloud rows stuck on `tariff_estimate` after refresh; double-count after SSE replay |
| Craftax / Trace V5 | evals receipts | correlation gate red; cross-bound rollout ids |
| Crash / memory | Desktop crash reporter + Activity Monitor sample | crash on launch, runaway memory during Laguna load |

Do not record prompt text, API keys, lease tokens, or card data.

## Feature flags / env (deployed candidate)

| Flag / env | Friends / staging | Public prod |
|---|---|---|
| `SYNTH_PUBLIC_INFERENCE_GATEWAY_SETTLEMENT` | `1` (also default-on for `local`/`staging`/`dev`) | explicit `1` after review; never silent stub |
| `DEV_AUTUMN_API_BASE` / `DEV_AUTUMN_API_KEY` | fake or sandbox Autumn only | unset |
| `AUTUMN_API_BASE` / `AUTUMN_API_KEY` or `PROD_AUTUMN_API_KEY` | sandbox | live Autumn |
| `AUTUMN_PRODUCT_ID_STANDARD` / `AUTUMN_PRODUCT_ID_MAX` | `standard_monthly` / `max_monthly` unless overridden | same |
| Clerk | production-supported method (no `+clerk_test` / `424242`) | same |

## Rollback

1. **Stop distribution** — unpublish the download URL / CDN object; keep the previous artifact URL live.
2. **Pin previous artifact** — checksum + notarization ticket of N-1 must already be on the release receipt.
3. **Backend** — revert the billing/metering image to the last green SHA; disable `SYNTH_PUBLIC_INFERENCE_GATEWAY_SETTLEMENT` only if settlement is corrupting money (prefer freeze + drain over silent unmetered).
4. **Autumn** — do not delete products; pause new checkouts via catalog/feature flag if needed.
5. **Desktop** — users keep local data. Sign-out language must remain “this device,” not “delete my work.”
6. **Comms** — incident channel posts: what users see, spending pause if any, ETA, checksum of the rolled-back build.

Rollback owner is named in `V01_SCOPE_AND_OWNERS.md`. No-go authority can halt Gate F/P without a full incident.

## Hard no-go

- Artifact SHA ≠ downloaded object SHA, or notarization missing
- Starter/Pro checkout not `mode=provider`, or entitlement allowance not 2000/20000 cents
- Any Synth Cloud turn without exactly one settlement fact, or Desktop dollars that disagree with backend/provider
- CRAFTAX-LUNA-010 not proven on the installed candidate
- Secret leakage (keys in shell snapshots, logs, CUA recordings, support zips)
- Intern on the friend/public surface
- Independent reviewer or CUA tester is the implementer

## Post-publish smoke (Gate P, within 60 minutes)

1. Fresh machine: download → checksum → install → signup → pair → local Laguna first response → Synth Cloud first response.
2. Starter checkout (human completes payment) → snapshot active, allowance from entitlement, one metered turn decrements, over-limit refused.
3. Open Usage + Account: billed amount matches backend fact.
4. CRAFTAX smoke: register known-good container, one rollout, Trace V5 correlation, visual opens.
5. Sign-out / sign-in / restart: local work intact; cloud plan still correct.
6. Support zip redaction canary.

## Receipts to attach

- Artifact SHA256, notarization id, Sparkle/appcast URL
- Workshop / backend / frontend / evals / site SHAs
- Gate receipt JSON from `evals/workshop` (`gate:release`)
- CRAFTAX-LUNA-010 evidence pack
- CUA 37-item receipt (Gate P; short subset for Gate F) dated within 24h of publish
