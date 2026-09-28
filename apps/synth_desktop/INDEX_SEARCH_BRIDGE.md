# Index Search bridge (v0.2 public search)

Workshop's bridge to the Synth Index public search API. One adapter module owns
the contract; one panel uses it. Nothing else in the renderer parses Index JSON.

## Where it surfaces

Settings → Account → **Synth Index search** (`IndexSearchPanel`,
`data-testid="account-index-search"`). An operator types a query, picks Fast or
Deep, and gets results with an **Insert citation** action (`onInsertCitation`
is a prop so the composer can wire it without touching the panel). Agents reach
the same backend through the `synth-ai` research MCP server's `index_search`
tool, registered like Workshop's other `mcp_servers` entries; that is a config
change, not a second client.

## Transport: SDK-shaped HTTP behind an injected transport, not MCP

The panel is a renderer surface, and the packaged WebKit CSP allows
`connect-src` to loopback only, so all hosted Synth traffic already crosses the
native boundary. The adapter (`runtime/indexSearch/`) therefore takes an
`IndexSearchTransport` function and speaks the raw contract itself:

- `fetchTransport(baseUrl)` — direct HTTP; valid for `127.0.0.1`/`localhost`
  backends (dev profile) and tests. This is what ships in this export.
- native relay — a Tauri command (`index_public_search_request`) that forwards
  the same `{method, path, headers, body, identity}` envelope through
  `http.rs` and attaches the operator's key from the secrets proxy when
  `identity === "account"`. It needs a regenerated `protocol.ts`, which belongs
  to the release-engineering checkout, so it is not in this change.

MCP was rejected for the panel because it would put a Python process between a
button and an HTTP call; the MCP tool is the agent surface, not the operator's.

## Identity

Public search is anonymous by default (`identity: "anonymous"`). The renderer
never holds an API key: `fetchTransport` fails closed with
`index_identity_unavailable` for `account`, and only the native relay may attach
credentials (project-local `.env` via the secrets proxy; never Keychain).

Identity also selects the capabilities route. The bare
`GET /api/v1/index/capabilities` answers 401 without a credential, so the
anonymous client reads `GET /api/v1/index/public/capabilities`
(`INDEX_PUBLIC_CAPABILITIES_PATH`); only `identity: "account"` reads the
authenticated route (`INDEX_ACCOUNT_CAPABILITIES_PATH`). Both carry the same
`public_search` block (`enabled`, `modes`, `limits`, `price_cents`,
`retention`, `privacy_copy`), so `parseCapabilities` and `copy.ts` are shared.
`capabilitiesPathFor(identity)` is the single place that picks.

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
| abort | `index_search_cancelled` | `cancelled` |
| anything else / bad JSON | `index_unexpected_status` / `index_malformed_response` | `failed` |

## Copy

`copy.ts` derives every price, limit, retention and privacy sentence from the
capabilities read (`/api/v1/index/public/capabilities` anonymously, see
Identity); absent capability → `null` → the row is not rendered. A test scans the adapter and the panel source for price/limit
literals.

## Private (authenticated) search later

Same adapter: construct `IndexSearchClient` with `identity: "account"` and the
native relay transport; add the private endpoint path constants next to the
public ones and a `visibility` field on `IndexSearchRequest`. No panel change.

## Verification

```
cd apps/synth_desktop
npx tsc --noEmit -p tsconfig.json
node --test src/renderer/src/runtime/indexSearch/indexSearch.test.ts   # Node ≥ 22.6 type stripping
node scripts/lint-app-css.mjs
```
