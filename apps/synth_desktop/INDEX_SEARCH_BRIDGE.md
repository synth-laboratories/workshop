# Index Search bridge (v0.2 public search)

Workshop's bridge to the Synth Index public search API. One adapter module owns
the contract; one panel uses it. Nothing else in the renderer parses Index JSON.

## Where it surfaces

Settings → Account → **Synth Index search** (`IndexSearchPanel`,
`data-testid="account-index-search"`). An operator types a query, picks Fast or
Deep, and gets the Index answer: `response` text citing contributions inline as
`[<contribution_id>]`, plus `citations: [{contribution_id, revision_id}]`, each
with an **Insert citation** action (`onInsertCitation`
is a prop so the composer can wire it without touching the panel). Agents reach
the same backend through the `synth-ai` research MCP server's `index_search`
tool, registered like Workshop's other `mcp_servers` entries; that is a config
change, not a second client.

## Transport: SDK-shaped HTTP behind an injected transport, not MCP

The panel is a renderer surface, and the packaged WebKit CSP allows
`connect-src` to loopback only, so all hosted Synth traffic already crosses the
native boundary. The adapter (`runtime/indexSearch/`) therefore takes an
`IndexSearchTransport` function and speaks the raw contract itself:

- `fetchTransport(baseUrl)` — direct HTTP for loopback development backends.
- `nativeIndexTransport` — the implemented Tauri command
  `index_public_search_request`, using the configured backend URL without
  resolving an account key. Hosted requests require HTTPS; redirects are
  disabled. Only anonymous public capabilities, create, poll and cancel routes
  are allowed. Request/response bodies are bounded; only the per-search token
  may be forwarded, and only on its matching lifecycle route.

The command is registered in the canonical Specta command list. The renderer
uses `commands.indexPublicSearchRequest` from the generated protocol. The
protocol, desktop tool schema and native dispatch are regenerated together:

```
cd apps/synth_desktop/src-tauri
cargo run --locked --bin export_protocol
cd ../../..
node scripts/generate-workshop-schemas.mjs
python3 scripts/generate-workshop-dispatch.py
```

This generator compiles the host and exports types without launching Workshop,
calling providers or running tests. This source candidate is 0.10.2; desktop
packaging and installed-client acceptance are separate release checks.

MCP was rejected for the panel because it would put a Python process between a
button and an HTTP call; the MCP tool is the agent surface, not the operator's.

## Identity

Public search is anonymous (`identity: "anonymous"`). Both transports reject
account identity; this relay never reads API keys or the Keychain. The panel
reads `/api/v1/index/public/capabilities` and uses the free public route even
when the operator is signed in. Authenticated paid/private search remains a
separate future surface; this change does not silently route a free request
through it.

## Search token

`search_token` is per search, 60 minutes, memory only. `IndexSearchClient`
returns an `IndexSearchHandle` whose token is captured in a closure: it is not
a property, `toJSON()` returns only the envelope, and `viewState.ts` (the only
state the panel holds) is built from the envelope. Polling sends it only as
`X-Search-Token`. Tests assert `JSON.stringify(handle)` never contains it.

## Errors → UI states (`viewState.ts`)

| Backend | Adapter code | Panel state |
| --- | --- | --- |
| 429 `index_public_rate_limited` (+`scope`, `Retry-After`) | same, `retryAfterS`, `scope` | `rate_limited`, live countdown, submit re-enabled at 0 |
| 503 `index_public_budget_exhausted` / `index_rate_store_unavailable` / `monitor_unavailable` | same | `unavailable`: fail closed, no retry loop |
| 413 `index_request_too_large` | same | `too_large` |
| 404 `index_public_search_disabled` | same | `disabled`, submit off |
| 404 `index_search_not_found` (wrong token) | same | `failed` |
| 202 then poll | `accepted` / `polling` progress | `searching` with attempt count, Cancel button |
| abort | `index_search_cancelled` (client also sends `POST /searches/{id}/cancel` with `X-Search-Token`) | `cancelled` |
| Deep poll 200 with `state: failed` | `index_search_failed` | `failed` |
| Deep poll 200 with `state: cancelled` | `index_search_cancelled` | `cancelled` |
| anything else / bad JSON | `index_unexpected_status` / `index_malformed_response` | `failed` |

## Copy

`copy.ts` derives every price, limit, retention and privacy sentence from the
capabilities read (`/api/v1/index/public/capabilities` anonymously, see
Identity); absent capability → `null` → the row is not rendered. A test scans the adapter and the panel source for price/limit
literals.

## Retry and cancellation

A transient poll failure or unconfirmed cancellation preserves the opaque handle in memory. Explicit retry
with the same query/mode/idempotency key resumes polling instead of starting a
second search. A pending handle exposes Resume and Request cancellation actions; query/mode edits stay locked until it reaches a terminal state. Tokens are not persisted across closing/reloading the panel.
Budget and rate-limit Retry-After headers gate manual retry; there is no
automatic retry loop. Stopping locally requests backend cancellation on a
best-effort basis and does not claim the server confirmed cancellation.
Public deliveries must report a zero customer charge.

## Private (authenticated) search later

An authenticated transport and paid/private endpoint require their own reviewed
contract and acceptance. This anonymous relay cannot be used as that transport.

## Verification

```
cd apps/synth_desktop
npx tsc --noEmit -p tsconfig.json
node --test src/renderer/src/runtime/indexSearch/indexSearch.test.ts   # Node ≥ 22.6 type stripping
node scripts/lint-app-css.mjs
```

## Wire shapes (Index v0.2 public contract)

- Errors are FastAPI-style `{"detail": {"code", "scope"?}}`; 429 carries `Retry-After` as a header.
- Capabilities limits are `limits.{fast,deep}.{peer_per_minute, peer_per_day, global_per_minute, global_per_day}`.
- `poll_url` is only followed when it equals the canonical lifecycle path for the accepted search ID; the fetch transport refuses any absolute URL whose origin differs from the backend, so the search token never leaves the backend host.
