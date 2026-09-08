# Capability migration ledger

This ledger records reviewed semantic mappings and proof. The generated inventory records declarations only. The frozen starting declarations are in `capability-baseline.json`; `capability-inventory.json` reflects current source. The authoritative plan is [authoritative-capabilities.md](authoritative-capabilities.md).

## Current delivery status

The requested product coverage, headless lifecycle and ACP hosting are implemented.
The current native catalogue contains **459 unique tools**. Coverage means the
current product's registered desktop and compatibility operations have an
executable public mapping, including persisted UI choices and presentation.
It does not mean every provider operation has been executed or every platform
has passed release certification.

| Boundary | Authoritative owner and mapping |
| --- | --- |
| 338 desktop registrations | `contract/specta.rs` generates executable dispatch and wire schemas; eight renderer/native mount callbacks are internal. Human decisions remain discoverable, with explicit human-action refusals. |
| 13 runtime/visual/state operations | `domains/{runtime,desktop_state,visuals}` own typed contracts; `adapters/workshop.rs` registers their execution. |
| Legacy MCP domains | `adapters/mcp/operations` owns the twelve ordered descriptor/handler modules. Compatibility executables are thin wrappers; public discovery and execution reuse these exact modules. |
| Managed browser | `browser/operations.rs` owns all 18 descriptors and one supervised backend. Existing origin policy and Context opt-in apply. Browser sessions survive client disconnects. |
| Persisted UI preferences/layout | Migration 71 and `domains/desktop_state.rs` own storage and revision checks. `preferences/runtimeStorage.ts` is the renderer port, including one-time legacy migration and conflict recovery. |
| Page and visual presentation | `app_present`, `visual_present`, and captures use the existing renderer. Requested presentation is distinct from captured evidence. Plugin visibility commits to shared preferences. |
| Integrated agent | Generated Codex homes register the same Workshop catalogue and canonical skill. Only unchanged, known legacy entries are removed when the unified bridge is available. Custom registrations remain. |
| Headless lifecycle | Same native process, database and services, with zero WebViews; explicit attach/detach/stop and CLI start/status. |
| ACP hosting | Configured local backends, durable task/run journal, streaming, human permissions, cancellation, cleanup and explicit resume; Settings → Context provides human controls. |

The compact `visual_manage` compatibility facade is expanded in the public
catalogue. `save` maps to `visuals_save`, `render` to `visuals_render`, and
`create_with_bind` to creation followed by `visual_bind_data_source`. Other
facade actions have named registrations. Typed `visual_create`/`visual_update`
retain shared ownership and revision checks instead of being shadowed by legacy
aliases. Task-correlated workflows require an explicit existing task ID;
workspace visual authoring and forking do not invent a conversation.

HTTP route families for containers/rollouts, optimizers/training, reports,
experiments/analysis, traces, diagnostics, plugins, annotations and session
control map to these shared handlers or generated desktop commands. Provider
proxy endpoints and container service protocol endpoints remain internal
execution transports; they are not a second public product API. Renderer
subscription/mount/report callbacks remain observation mechanics. Human
credential, access and annotation decisions are not executable agent actions.

The registration test requires every desktop and compatibility declaration and
every browser descriptor to have a public mapping or the explicit internal/facade
classification. Generated inventory/dispatch/schema checks catch source drift;
the inventory itself remains a declaration report, not a fabricated parity score.

Future architecture extensions in the original P1/P2/P6 plan—narrow per-client
grants, independent visual views, durable visual interaction replay, MCP Apps
embedding and the installed cross-platform/client matrix—are separate from
this delivered local product boundary. They are not claimed by its feature flag.

## Runtime and ACP implementation

The authoritative desktop registration remains `contract/specta.rs`.
`scripts/generate-workshop-dispatch.py` generates direct calls to those handlers,
including their real state injections and argument deserialization. Each
operation has a separately allocated future: native acceptance exposed a stack
overflow when all handlers were in one large async dispatcher. The generated
file contains adapter plumbing, never copied domain business logic.

`scripts/generate-workshop-schemas.mjs` uses TypeScript's checker on the existing
Specta-generated wire contract. It strips the renderer-only `typedError`
envelope and preserves the native success result under MCP's `{ result }`
envelope. Native JSON Schema validation exposed 85 existing optional numeric
annotations that erased Rust nullability. They now use
`Option<specta_typescript::Number>` at the Rust source; the renderer progress
projection was adjusted to omit an absent total. Both desktop and MCP consume
those corrected types. Generated registration/inventory checks run in the cheap
CI lane; full desktop verification also checks generated schemas.

`contract/desktop_policy.rs` explicitly excludes renderer acknowledgements and
native mount callbacks. Agent calls cannot resolve their own approvals, expand
access grants, import credential material into the Keychain-backed registry,
or manufacture human annotation evidence. These human operations return
`human_action_required`; discovery names the required human surface. This
is a boundary classification, not completed automated parity for those flows.
Existing hosted Codex homes register the same public Workshop bridge and
shared skill; unchanged owned legacy registrations are removed when it is available.

`platform/desktop_runtime.rs` owns window attachment; `agent_integration/lifecycle.rs`
launches the same native composition with `--workshop-runtime`. No second
SQLite owner or replacement service graph is introduced. The native event loop
still requires a desktop-capable operating environment. A display/capture request
attaches a window and waits for page load; headless mode does not fabricate a
screenshot. Explicit stop drains the existing service supervisor.

`session/acp` is an ACP **v1** stdio adapter, selected from the
[official initialization contract](https://agentclientprotocol.com/protocol/v1/initialization).
The private, user-owned `agent-backends.json` registry chooses absolute
executables, workspaces, optional project `.env` paths and bounded limits. The
host never configures arbitrary executables through MCP and never reads `.env`
values into client configuration or the Keychain-backed registry. A configured
working directory is not an OS sandbox.

Migration 70 retains backend attachment identity, remote session ID, workspace,
parent/depth, capabilities, and process start identity against real Workshop
sessions. CoreRuntime services own status and run events. ACP updates are
journaled before a following prompt completion can overtake them. Message size,
pending requests, permission requests, turn duration and concurrency are bounded.
Permission callbacks use the existing broker and only offer one-time grants.
Cancellation expires pending cards and terminates an unresponsive backend after
five seconds. Closing a parent closes attached descendants. Connection loss
terminalizes the active run; restart never automatically replays a prompt.
Explicit resume requires an unchanged workspace and negotiated `loadSession`.
Startup reaps retained processes only when their kernel start identity matches.

The Settings → Context → Hosted agents panel uses the generated command edge
for start/send/cancel/close/resume, journal reads and explicit human decisions.
It owns no runtime state or credential store. Provider logins and ACP client
filesystem/terminal facilities are not advertised as implemented.

## Final local acceptance evidence

All live checks used the dedicated `artifacts/capabilities/instances/v09/agents`
data root. They made no provider calls, read no Keychain credentials and did not
modify the user's Codex or Claude configuration.

| Check | Result |
| --- | --- |
| Native build, Desktop + Workshop CLI | Passed with the named instance configuration and `eval-driver` feature |
| Rust shared adapters/registry | 60 passed; one opt-in exporter ignored by this filter |
| Rust CLI | 20 passed, including descriptor replacement/retry scope |
| Rust ACP transport | 4 passed |
| Rust compatibility executable boundary | 1 passed; all thirteen wrappers use the shared adapter owner |
| Canonical Specta exporter | Passed; 338 registered command bindings |
| Dispatch, JSON schemas, preference defaults and inventory drift | Passed |
| TypeScript | Passed |
| Playwright UI | 3 passed: visual routing, hosted-agent controls, runtime storage conflicts/reconnect |
| Real browser backend | 2 passed: bounded snapshots, stale refs, tabs, profiles and origin revocation |
| Native expanded coverage | 459 tools; shared visual authoring/fork ownership, headless preferences/CAS, human-decision refusals, actual browser click/snapshot/screenshot, origin denial and client reconnect passed |
| Native ACP host | Initialize/new/prompt, journaled streaming, human permission cancellation/expiry, explicit load with 150 replayed updates, uncooperative cancellation and EOF recovery passed |
| Native visual presentation | Fullscreen PNG captured at the requested revision and inspected; stale visual update refused |
| Native page presentation | `app_present` opened Settings → Context; captured route and pixels inspected |
| Runtime restart | Same live MCP process survived six failed calls during downtime, discovered the new boot and read unchanged durable state from the restarted zero-WebView runtime |

Receipts and logs are retained under ignored `artifacts/capabilities/`, including
`coverage-native-receipt.json`, `coverage-acp-receipt.json`,
`coverage-fullscreen-receipt.json`, `coverage-settings-receipt.json` and
`coverage-restart-receipt.json`. Native browser evidence includes an HTML
`output` status after a click; the snapshot omission found by that test was fixed.
The final doctor reports `fullProductCoverage`, `headless` and `acpHosting` true.
Doctor deliberately keeps `renderingVerified: false`; the separate inspected
captures supply that evidence.

The unchanged broader repository failures remain the optimizer version pin
(`0.2.19` versus `0.2.20`) and CSS literal budgets (538/520 font sizes, 340/299
radii). Authenticated provider adapters and installed-release certification were
not exercised; these receipts establish local integration behavior.

## Earlier runtime acceptance evidence (before the final coverage expansion)

- Native `scripts/test-workshop-runtime.py`: **339 discovered tools**, all input
  and output schemas validated; real sampled responses validated against their
  schemas. Same process survived detachment and performed visual/desktop reads.
- Real host + deterministic ACP peer: initialize/new/prompt, durable streaming,
  rejection of MCP self-approval, cancellation of a pending human permission,
  explicit resume with 150 replayed updates, noncooperative cancellation,
  connection-loss terminalization, and same-process desktop reattachment passed.
- The peer in `scripts/fixtures/workshop-acp-agent.py` performs **no provider calls**.
  This proves Workshop integration behavior, not an authenticated Codex/Claude
  adapter run. Receipts are under ignored `artifacts/capabilities/`.
- Focused browser tests passed for hosted task controls/human approval and
  visual presentation routing. Browser mocks are not native rendering evidence.
- Native full-screen presentation/capture passed after removing a redundant
  post-fullscreen focus call. The 3456×2168 PNG was inspected; its receipt
  reports full screen and the requested revision. `app_capture` also attached
  a previously detached desktop and returned actual pixels/app state.
- CLI cold start produced a runtime with **zero WebViews**. One live MCP client
  successfully called the runtime, survived its explicit stop/start, picked up
  the new boot/token, and continued reading the same instance. The dedicated
  runtime and its processes were stopped afterward.
- Completed Rust checks: 47 passing across contract, registration, visual, ACP and
  CLI tests. One broader contract test fails on the existing optimizer pin
  mismatch (`0.2.19` versus `0.2.20`), verified unchanged in the base commit.
  TypeScript passes. CSS lint still reports the unchanged baseline debts:
  538 font sizes versus 520 and 340 radii versus 299.
- The earlier host loader stall is resolved: fresh native/CLI builds and tests
  subsequently completed. Both main-thread attachment and restart-scoped retry
  suppression now have build and live acceptance evidence below.
- Claude Code discovers the expanded server using an isolated configuration;
  Codex parses the registration using command-line overrides. Real user client
  configuration was not changed. Neither check executes a model turn.

The following sections retain the earlier visual implementation evidence;
current checks and any remaining limits are recorded above and in the README.

## Reviewed operation: visual template discovery

- Canonical operation ID: `visuals.templates.list.v1`.
- Owner: visual domain; `domains/visuals/operations.rs::ListVisualTemplates`.
- State authority: existing instance `VisualRegistry`, including its bundled and managed template catalogues.
- Desktop entry: `visuals_templates_list` in `lib.rs`, registered by `contract/specta.rs`.
- MCP entry: `visual_list_templates`, also reachable through the existing `visual_manage/list_templates` facade.
- IPC entry: `GET /v1/visuals/templates`.
- Result: typed `{ templates: TemplateMeta[] }`; desktop keeps its compatible array-shaped envelope.
- Input: optional case-insensitive genre or ID substring filter. Unknown input fields and invalid types are rejected; a bodyless HTTP GET is explicitly normalized to an empty request by the adapter.
- Removed duplication: hand-authored MCP input schema and direct template-list calls in desktop/IPC adapters.
- Fixed behavior: MCP previously advertised `genre` but sent no query body; the filter is now serialized from the typed request.
- Preserved authorization: existing desktop / authenticated IPC boundary. This query does not grant access, create a session, or access provider credentials.
- Validation: native library/MCP compile, source inventory generation/check, whitespace verification and both focused real-catalogue/contract tests passed. Exact commands are recorded below.

This operation is **partially migrated**, not proof that the overall P1 dispatcher or external-client parity is complete. The public schema helper currently lives in the host crate; extracting protocol-only dependencies from the desktop binary belongs to the runtime separation work. Narrow actor/grant context and bounded catalogue discovery remain pending; generated desktop registration is now implemented above.

## Experimental external visual workflow

The owning definitions are in `domains/visuals/operations.rs`, projected through
`contract/capabilities.rs`. `adapters/workshop.rs` binds each declaration to an
executable handler and exposes discovery and invocation through the existing
authenticated local IPC. `agent_integration` is an attachment/configuration
client; it does not open SQLite or duplicate a tool catalogue.

| Operation ID | MCP entry | State / host authority |
| --- | --- | --- |
| `visuals.templates.list.v1` | `visual_list_templates` | Registered instance template catalogue |
| `visuals.list.v1` | `visual_list` | VisualRegistry, bounded pagination |
| `visuals.get.v1` | `visual_get` | VisualRegistry |
| `visuals.create_shared.v1` | `visual_create` | VisualRegistry transaction, durable workspace owner |
| `visuals.update.v1` | `visual_update` | VisualRegistry transaction, expected revision required |
| `visuals.present.v1` | `visual_present` | Native window and one renderer presentation intent |
| `visuals.observe.v1` | `visual_observe` | Existing renderer observations; absent evidence returns null |
| `visuals.capture.v1` | `visual_capture` | Existing serialized native WebView capture; PNG and revision-matched receipt |

Migration 69 creates one stable workspace identity per instance. Shared visual
creation stores ownership in the creation transaction; triggers reject owner
removal or simultaneous workspace/chat ownership. Forking without a destination
chat preserves the workspace owner. Existing hosted visual creation retains its
session requirement. No synthetic chat or ownerless external visual is created.

Connection scope is explicitly **the whole selected local instance**, using its
existing private IPC descriptor and bearer boundary. This is not a per-client
grant implementation. Narrow grants, revocation and independent actor identity
remain P1/P3 work. No credential is exported to MCP configuration or plugins.
The bridge rejects non-loopback URLs and private descriptor ownership failures,
disables proxies/redirects, and reloads the descriptor on every request.

Inputs and outputs derive from the Rust wire types. One protocol projection
normalizes unconstrained boolean schemas to equivalent object schemas and
removes nonstandard numeric format annotations while retaining types/ranges.
The real Claude Code discovery check exposed this compatibility requirement.
The stdio adapter currently negotiates its existing 2024-11-05 baseline; this
is not a full modern MCP conformance claim.

The packaged CLI is added to the existing adapter build/copy/sign list. Its
connect/disconnect operations preserve unrelated configuration, refuse a
different/customized Workshop entry, and stage private atomic replacements.
Plugin export uses one embedded skill and one MCP config with two thin host
manifests. It does not install into the user's clients automatically.

## Confirmed migration pressure points

1. The legacy hosted visual MCP still requires `SYNTH_SESSION_ID`. The new shared-visual operation uses durable workspace ownership and existing whole-instance authentication. Complete explicit actor/grant enforcement before declaring equal narrow authorization across all clients.
2. `visuals_ipc.rs` coordinates many domains. Migrate one owning workflow at a time, then delete its transport-owned rules.
3. Generic MCP facades coexist with named tools. Preserve their declared aliases during migration while making typed named tools the canonical interface.
4. Tauri command constants, runtime registration, renderer wrappers and MCP names are separate declarations. Similar names are not proof of equivalent behavior.
5. Source extraction does not enumerate all dynamic registration, wrapper calls, native callbacks or UI-owned mutations. Those remain an explicit P0 review queue.
6. `session/codex/home.rs::mcp_enabled_tools` restricts the integrated Codex surface independently of server registration. Current extraction identifies 39 literal allowlist entries; block/computed cases remain manual review. The visual template query is reachable through `visual_manage`, while its named tool is callable by other clients. This is an explicit visibility mapping, not proof of identical tool-selection usability.

## Validation results

| Check | Result |
| --- | --- |
| `cargo check --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib --bin synth-visuals-mcp --offline` | Passed; existing crate warnings remain |
| `python3 scripts/capability-inventory.py --check` | Passed against current source hashes |
| `git diff --check` | Passed |
| `cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib domains::visuals:: --offline` | Passed: 3 tests; real catalogue, ownership persistence/triggers, fork and concurrent revision conflict |
| `cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --bin workshop --offline` | Passed: 17 tests including config preservation, conflicts, plugin export and descriptor checks |
| Focused Rust schema-projection and executable-registration checks | Passed: 2 tests |
| Existing `visuals::registry::tests` | Passed: 14 tests, including transaction rollback, chat isolation, fork and rendering |
| `export_specta_protocol_bindings` | Passed after the reviewed existing command-count correction; generated file matches exactly |
| `npm run typecheck` | Passed |
| `npm run lint:app-css --workspace @synth/synth-desktop` | Existing baseline failure: 538 font-size literals versus budget 520; 340 radius literals versus budget 299. Base and current counts are identical; added layout rules introduce no literals covered by the gate |
| Focused Playwright: `workshop-agent-presentation.spec.ts chart-pane.spec.ts` | Passed: 2 tests; latest presentation wins, missing target errors, chart/revision/layout regression |
| Codex `mcp get workshop --json` with isolated `-c` server overrides | Passed: real client parses the generated registration; does not prove an LLM turn |
| Claude `mcp get workshop` with isolated `CLAUDE_CONFIG_DIR` | Connected, including successful real tools discovery |
| Plugin creator validator and skill quick validator | Passed on the actual exported plugin and canonical skill |
| `scripts/test-workshop-mcp.py` against the real named dev instance | Passed, including actual native PNG inspection and stale-write rejection |
| Same native smoke with `--fullscreen` | Passed; native receipt reports `windowFullscreen: true`; exits full screen afterward |

Local logs and receipts are under ignored `artifacts/capabilities/`; the full-screen
receipt is `native-mcp-fullscreen-receipt.json`. Native acceptance used the repository
instance lifecycle with a dedicated `agents` data root and Laguna autostart disabled.
The development build consumed already-staged optimizer/MLX/helper resources from
the source checkout; it is not proof of a self-contained release installation.
Playwright used the installed browser cache via `PLAYWRIGHT_BROWSERS_PATH` and a
worktree-local `TMPDIR`. Its mock bridge tests UI behavior only; native evidence
comes from the separate live MCP smoke. CUA could not resolve the raw dev binary,
so installed-app CUA acceptance remains unverified.

No provider execution, Keychain operation, or authenticated client LLM turn was
performed. The real user client configurations were not modified. At that initial milestone, full product
coverage, headless survival and ACP hosting were not yet implemented. The
current runtime/ACP work above supersedes those lifecycle limitations; durable
visual interaction replay, MCP Apps embedding and release certification remain.
The dedicated native test instance was stopped after verification; its local
receipts and database are retained. Focused Rust checks total 37 passing tests.

The Specta drift check initially failed on its existing `323` command-count
expectation, while both the base committed bindings and the generated graph
contained `329`. Reviewing `d27b80da` confirmed it introduced 23 human annotation
registrations while its test comment counted 17. The expectation now accounts
for all 23, including export, campaign lifecycle/adjudication and supersession;
that initial milestone added no desktop commands. The current ACP adapter adds seven. Generated protocol changes were produced
by the existing exporter, including the optional visual workspace owner and
previously stale template metadata types.

## Explicit coverage boundary

`fullProductCoverage` is true for the reviewed local product boundary above.
Browser interaction, rollout orchestration, visual authoring/certification and
persisted renderer preferences are now included. Headless mode uses the native
event loop; it is not a display-server-free Linux service. Native screenshots
require a desktop-capable OS session. Public connections grant the selected
local instance, not per-chat isolation. Provider authentication, authenticated
Codex/Claude ACP runs, remote MCP, independent view state, durable interaction
replay and installed release certification are not claimed by fixture receipts.
