# Workshop authoritative capabilities: architecture and implementation plan

Status: finalized implementation plan; implementation kickoff authorized and underway. Finalized 2026-09-07.

Current implementation: one public MCP catalogue projects all 338 registered
desktop commands subject to human/renderer policy, 13 typed runtime/visual/state
operations, and the shared compatibility and managed-browser registrations.
Legacy executable wrappers reuse those definitions and handlers. Persisted UI
state uses the runtime's revision-checked store. One native runtime operates
with zero WebViews and attaches its desktop on demand. ACP v1 hosting uses the
existing session/run journal and approval broker, with a Settings → Context
panel. The [migration ledger](capability-migration-ledger.md) records the mappings,
native acceptance and remaining release validation. Connection scope remains
the whole explicitly selected local instance.

This document is the single planning source for this initiative. It supersedes the earlier proposal at `Documents/ChatGPT/Synth Prod/docs/workshop-authoritative-capabilities-design.md`. That earlier file is an archived draft and is not maintained. Workspace instructions prohibit editing it from this implementation workspace.

Implementation branch: `codex/authoritative-capabilities`, isolated worktree `/Users/joshuapurtell/GitHub/workshop-authoritative-capabilities`, based on committed `da92c328c8c6a60eb0ef5859bd1952b7e21b3293`. Uncommitted work in the source checkout is excluded and must be reconciled explicitly before integration. This work does not overwrite the source checkout or choose a release destination.

## Decision

Organize Workshop around domain-owned, typed capabilities executed through one runtime boundary. Generate public protocol surfaces from those definitions. Desktop, external agents, and the integrated agent consume the same capabilities under equivalent grants. The runtime owns durable workspace state; renderers own observations of what they actually displayed.

This extends existing Workshop architecture. It does not introduce another execution engine, provider authority, persistence stack, or agent-only API.

## Scope and fixed decisions

The completed system supports excellent integrated agents and complete external-agent access to Workshop, especially the live visuals panel. MCP is the public agent entry point; the CLI and desktop expose the same domain operations. ACP and Codex App Server are agent-hosting adapters.

The following decisions are fixed for this initiative:

1. Rust domain operation definitions are authoritative. The existing TypeScript contract exporter is extended; independently maintained transport schemas are removed as their domains migrate.
2. One runtime process owns each local instance's database, operation dispatch, durable work, and event journal. The desktop becomes an attachable presentation client. Closing the GUI does not implicitly stop admitted work; explicit shutdown reports active work and follows its documented policy.
3. Public operations are named and typed. There is no mandatory generic action language, new IDL, or agent-specific workflow engine.
4. All product capabilities enter the coverage inventory. Host implementation mechanics are classified separately, with an equivalent public semantic operation wherever the user can perform an action.
5. Built-in and external agents use the same authorization boundary and registered handlers. External agents do not require hosted sessions.
6. Visual document, view, surface, and observation are separate records. Full-screen presentation uses the existing renderer and style system.
7. The first end-to-end slice is visual creation through user interaction and reconnect. It establishes the pattern before broad migration.
8. Local macOS is the first acceptance target, matching the inspected product. Other supported platforms advertise actual capabilities and require their own proof; no unverified cross-platform claim is included.
9. Existing real providers remain execution authorities. No fake streams, invented receipts, or alternative test runtime count as acceptance.

Excluded from this initiative: a new cloud control service, remote unauthenticated access, arbitrary agent-authored code execution inside trusted visuals, a replacement optimizer/container runtime, or universal cross-vendor conversation transfer. These are not prerequisites for local interface parity.

## Evidence and scope

Read-only inspection covered the main Workshop checkout at `/Users/joshuapurtell/GitHub/workshop` on `codex/environment-qa-prototype`, whose recorded HEAD was `da92c328c8c6a60eb0ef5859bd1952b7e21b3293`, plus historical candidate code. Working files were inspected, so HEAD alone does not assert a clean or reproducible snapshot. These are different checkouts; this is not an assertion about a released build or a choice of implementation base.

Relevant current files, relative to `apps/synth_desktop/src-tauri/src/`:

- `core_runtime.rs`: composition, storage, event journal, services, recovery; currently also depends on Tauri.
- `platform/operations/mod.rs`: operation identity, phase, context, and persisted operation records. Extend this existing owner.
- `platform/failure/`, `platform/approval/`: existing cross-domain mechanisms to preserve.
- `contract/specta.rs`: authoritative Tauri registration and generated TypeScript exports today.
- `bin/synth_visuals_mcp.rs`: separately authored MCP definitions and forwarding logic; 2,241 lines in the inspected checkout.
- `visuals_ipc.rs`: routes and substantial application orchestration spanning several domains; 7,568 lines in the inspected checkout.
- `presentation.rs`: shared visual identity, eligibility, binding, reuse, and presentation behavior for UI and MCP. This is a useful precedent.
- `domains/`, `adapters/`, `composition/`: useful boundaries, currently partly focused on failure handling rather than a complete application operation layer.

Guidance: `frontend/public/synth-style.md` and `WORKSHOP_ENGINEERING_PRINCIPLES.md` in the current workspace, and `WORKSHOP_QUALITY_STYLE_GUIDE.md` in Workshop. The proposal follows API-first contracts, inward ownership of complexity, composable primitives, real-system evidence, and a single production execution path.

## The feature unit

A capability is a cohesive domain feature: contract, operations, state transitions, read projections, events, and presentation binding. A feature has one owner and one set of semantics. Multiple entry points are projections of that feature.

For each public operation, define together:

- Stable operation ID and schema version.
- Typed request and result, constraints, useful descriptions, and examples.
- Domain handler registration and the authority it uses.
- Resource scope, required permission, and consequential approval behavior.
- Immediate or job-producing execution; idempotency and revision-conflict behavior.
- Referenced result resources and relevant event schema IDs.
- Availability requirements and public exposure classification.

Use Rust request/result types and typed registrations as the implementation source. Extend existing Specta generation for TypeScript; add schema export from the same types and declarative constraints. Verify serialization, constraints, and exported schemas agree. Do not maintain a second JSON tool catalogue by hand. Dynamic eligibility remains a domain decision; the descriptor names its typed requirements and policy hooks.

The operation catalogue assembles domain registrations. It contains discovery metadata and routing, not domain workflows. Avoid a central match statement containing every feature's business logic. A bounded registration helper is sufficient; do not start by building a general-purpose framework or custom IDL.

Generate named MCP tools, typed desktop bindings, CLI bindings/help, reference documentation, and the exposure coverage manifest. A public generic `execute(anything)` tool must not replace discoverable, typed operations. Common result formatting and human-readable summaries live in shared presenters, with transport-specific content envelopes at the edge.

### Registration and generated outputs

Each domain registers its descriptors once with the composition root. Registration binds its operation ID to a typed handler, request/result schema, permission policy, supported execution mode, revision policy, and examples. The build fails on duplicate IDs, conflicting wire names, missing types, or a public descriptor without an executable binding.

Generate transport bindings and registration from this assembled catalogue. In-process calls may use a typed facade rather than JSON serialization, but must traverse the same dispatcher checks. Tauri-native callbacks and lifecycle hooks have explicit host-only definitions. Code generation handles transport plumbing; it does not generate business logic or React layouts.

Use small shared request primitives for pagination, expected revision, and retry identity. Use domain-specific result types; do not turn all results into opaque JSON or one enormous optional-field union. Where JSON is intentional, such as a bounded chart specification, name and validate its schema.

Generate an inventory with these columns: operation ID, domain owner, input/output schema version, effect category, public/internal classification and reason, GUI action, integrated-agent binding, MCP name, CLI command, handler, state authority, acceptance journey, and migration status. Freeze the initial inventory as the denominator; new operations extend it. Removed product behavior requires an explicit product decision, not an inventory deletion to improve a metric.

### Versioning

Version the Workshop contract independently of the installed MCP or agent protocol. Advertise runtime version, contract version, operation schema versions, supported optional features, and instance identity. Protocol adapters negotiate only features implemented by that client/server pair.

Additive optional fields remain compatible. Required-field changes, type changes, changed effects, or incompatible enum behavior require a new schema/major contract version. Unknown operation IDs and incompatible versions return typed errors. String-encode identifiers, hashes, and counters that may exceed JavaScript's safe integer range. Keep canonical serialization and request digests deterministic.

Preserved old wire names are generated aliases to the same handler, with a published removal version and registration upgrade path. There is no execution fallback to the removed implementation. Pin tested client/backend versions in release evidence and revalidate those adapters when upgrading them.

## Ownership and dependencies

Target organization inside the existing Rust host; paths describe responsibilities, not a requirement for a wholesale directory move:

```text
contract/                     exported schema/version catalogue and generators
platform/
  operations/                 execution context, dispatch, deduplication
  approval/                   existing approval authority
  failure/                    existing failure identity and public views
  persistence/                existing transactional mechanisms
domains/<domain>/
  contract.rs                 operation definitions, DTOs, metadata
  operations.rs               application handlers and workflows
  authority.rs                valid domain transitions
  read.rs                     bounded authoritative projections
  events.rs                   domain event definitions
adapters/
  mcp/                        public agent transport
  tauri/                      desktop transport
  cli/                        scripting transport
  agents/                     Codex App Server, ACP, other agent backends
  presentation/               desktop window/render host
composition/                  assembly and lifecycle wiring
```

Consolidate existing `domain/`, `domains/`, and feature service modules by actual ownership as slices migrate. Preserve a single implementation; do not create empty parallel domain trees with duplicated services underneath.

Dependency direction: adapters invoke operations; operations use domain authorities and narrow infrastructure/provider ports; composition supplies implementations. Domains cannot import MCP, Tauri AppHandle, Codex wire DTOs, or renderer code. Only domain-owned repositories/authorities write their state. The composition root assembles rather than becoming a universal service locator used to access arbitrary internals.

An application workflow may coordinate domains through their operations or narrow services. It may not write their tables directly. Containers and Optimizers remain authoritative for their executions and evidence. Workshop mirrors declared provider state and preserves provider identities and digests.

## Execution contract

```text
transport request
  -> authenticated actor and explicit instance/workspace scope
  -> typed operation validation and resource authorization
  -> domain preflight / approval where required
  -> domain handler
  -> durable state, operation result, and event publication record
  -> post-commit delivery
  -> typed result or durable job reference
```

Extend the existing OperationContext with authenticated client/principal and scope. Hosted session/turn IDs are optional correlation references. External clients must not need a fabricated internal chat session to operate a visual. Claimed session IDs in input do not confer permissions.

Separate invocation identity, long-running job identity, provider run identity, and visual identity. A retry refers to the original operation. It must not silently start another admitted run.

For local transactional mutations, commit state and event publication records atomically. For external side effects, persist intent and the provider reference, use provider idempotency where available, and reconcile ambiguous responses. Never claim exactly-once execution across a database and an external provider. An unknown dispatch outcome needs reconciliation, not an automatic duplicate execution.

Scope idempotency keys by actor/grant, workspace, and operation; bind them to canonical arguments. Reject reuse with different arguments. Check current authorization before returning retained results. Mutations of shared records accept expected revisions; conflicts return the current revision and a safe explanation.

Reuse the existing failure model. Permission-required, unavailable, conflict, retryable failure, and unknown outcome have distinct meanings. Existing approval receipts bind the exact consequential action/specification and allowed budget. No transport can self-approve by supplying an approval ID. Use only already-authorized non-Keychain credentials and the secrets proxy.

Do not force every query through a durable job or replay the entire journal to answer it. Use existing bounded read models. Jobs refer to the owning domain lifecycle; a generic jobs view must not invent a competing lifecycle authority.

## Visuals and app snapshots

Separate four identities:

| Object | Ownership and lifetime |
| --- | --- |
| Visual document | Workspace-owned definition, bindings, annotations, and revision |
| View | Filters, selection, zoom, replay/follow-live position; independently addressable and optionally persisted |
| Surface | Window/pane/full-screen attachment to a view, renderer identity, and connection epoch |
| Observation | What a renderer displayed at a specific document/view revision and data cursor, optionally with an image |

This allows the same visual to appear in two windows with different filters. Shared views are explicit. An agent updates a document or an identified view; it cannot accidentally overwrite every user's window state through a global active-visual variable.

Presentation requests identify a surface, or use a documented selection policy that fails with an actionable ambiguity when needed. Persist requested presentation separately from renderer acknowledgement. Return queued until acknowledged; return rendered only with an observation. A closed/disconnected renderer makes the observation stale. Never infer that a visual was displayed merely because a show event was emitted.

The runtime exposes presentation and capture ports; Tauri implements them. This makes runtime independence achievable without claiming the current Tauri-coupled CoreRuntime already runs as a standalone daemon. Separate ownership in-process first; then extract the separately supervised per-instance daemon as a required delivery phase. Active work survives GUI exit once that phase is accepted.

Semantic snapshots include workspace and surface IDs, selected resource references, document/view revisions, data position, freshness, and pending interactions. Screenshots are optional evidence of layout. Runtime facts come from domain projections; renderer geometry and actual display state come from renderer observations. A snapshot spanning both names its separate revisions/cursors and does not pretend to be one globally atomic image of a live system.

Keep the existing visual family registry, binding validation, trusted template rules, and rendering path. Both full-screen and pane modes render the same component. Reuse `app.css` and `visuals/chrome/tokens.css`; introduce no agent-specific visual skin or second renderer. New visual families declare slot schemas, supported interactions, and observation requirements alongside their existing templates.

User interactions are typed events naming the view, visual revision, selected resources, and intended recipient when a message is submitted. Hovering/selecting is context, not automatic permission to launch work. Only the selected controller receives a submitted follow-up by default; other observers do not independently execute it.

## Observation, recovery, and local clients

Provide bounded snapshots plus cursor-based event reads. Each client keeps an independent cursor; reads are not a destructive shared queue. Specify cursor scope, ordering, retention, and gap handling. On a retention gap, return an explicit reset requirement and a new snapshot position. Slow consumers get bounded data and a resumable cursor. Do not promise total ordering across unrelated provider streams; preserve each provider's sequence and Workshop's ingestion position.

Notifications accelerate observation; cursor reads remain the baseline. UI subscriptions, integrated-agent events, and external MCP reads use the same event source. The same external agent can reconnect and recover the workspace without reconstructing it from conversation history.

Ship one public Workshop MCP entry point, selecting an explicit registered local instance/workspace and exposing named operations from the catalogue. Prefer a stdio bridge to the local runtime; multiple bridge processes share that runtime. Registration tooling configures clients and reports connection/readiness. It does not start an independent workspace database per agent. Scope any local IPC capability to the instance and grant; do not print credentials or expose them in snapshots.

Support capability-based discovery and bounded domain groupings as the tool surface grows. Classify host-internal mechanics explicitly (for example, reporting a mounted native terminal's geometry). They are not user operations to expose as raw MCP tools; the equivalent user operation, such as resizing/focusing a surface, must be public. Missing product operations cannot be hidden by calling them internal.

### Local runtime and client lifecycle

Use the existing instance registry and authenticated local IPC as the migration starting point. A startup lock ensures exactly one owning runtime per instance. A bridge may request startup of the installed runtime for its configured instance, but cannot guess among multiple workspaces or clone an instance silently. Stale discovery records trigger verified reconciliation, not an attachment to another running instance.

The instance connection descriptor identifies the process generation, contract version, endpoint, and a protected authentication mechanism. Keep credential material out of model-visible command arguments and logs. Prefer protected local IPC; if the current loopback HTTP transport is retained, require authentication, validate local origins/hosts, and keep the listener local. File permissions and tokens prevent accidental cross-client access; they do not constitute a sandbox against arbitrary code running as the same OS user.

The desktop, MCP bridge, and CLI reconnect using instance identity and cursors. A daemon restart reconciles accepted work against provider truth. Orphaned processes and unknown outcomes become explicit states. Cancellation requests have their own acknowledgement and do not imply the provider already stopped. A transport timeout or MCP request cancellation does not silently cancel durable work; clients invoke the job's explicit cancellation operation.

The GUI may be launched through a public presentation operation using the installed host. If launch succeeds but rendering is pending, return a durable presentation request ID. If no compatible host is installed, return `presentation_unavailable`; creation, queries, and job control still work headlessly. No fake screenshot is substituted.

Provide a proposed `workshop connect` installation flow, `workshop mcp` bridge, and `workshop doctor` diagnostics. These commands are deliverables, not claims about existing commands. Connection setup is idempotent, identifies the chosen instance, preserves unrelated client configuration, and supports removal. An authorized user can register supported clients without a Workshop-specific chat or provider key import.

## Integrated agents and external agents

Both invoke the same public Workshop operations. The integrated experience may have faster delivery, composer context, and native affordances, but no private domain privileges.

Agent hosting uses an adapter interface for session creation, input, observation, cancellation, and explicitly advertised optional functions. Native Codex and ACP adapters preserve backend identities, approvals, and unsupported capabilities. This is separate from the MCP interface that lets those agents use Workshop. Do not force all native agent features into an invented universal session lifecycle or claim unsupported resume/steer behavior.

Delegation is a domain workflow, not an implicit consequence of ACP. The agent-hosting phase includes explicit start, send, observe, and cancel operations plus bounded delegation where the backend supports it. It records parent/child relations, workspace scope, granted resources, budgets, cancellation rules, and target backend. MCP access alone does not start agents or provider-backed experiments.

Submitted user messages name a target agent/session. A delivery record is separate from the message's observation event; retries reuse the delivery identity. Mark delivery accepted only when the backend acknowledges it. When a backend cannot deduplicate an uncertain send, expose the uncertainty instead of sending it again automatically. MCP clients that cannot receive unsolicited turns retrieve interactions on their next event read; Workshop does not claim that an event automatically wakes every external agent.

## Domain ownership and coverage

Ownership is assigned by responsibility, not a named developer. Each implementation PR names its accountable domain owner and reviewer.

| Domain / owner | Required product coverage | Authority retained |
| --- | --- | --- |
| Workspace and access | Instances, workspace selection, projects, files, grants, preferences, import/migration | Existing workspace/access services and local instance registry |
| Visuals and presentation | Discover families; create, bind, update, organize, present, capture, review, export; views and interactions | Visual registry, shared presentation service, host observations |
| Data and evidence | Containers catalogue, datasets, trace import/query, artifacts, ranges, lineage, provenance | Existing data/evidence repositories and provider identities |
| Research | Experiments, research logs, reports, annotations, seals, audience/sharing where supported | Existing research/report/annotation services |
| Execution | Prepare, validate, approve, start, inspect, cancel, pause/resume where supported; training and inference | Containers/Optimizers and their existing Workshop projections |
| Agent sessions | Backend discovery, start/send/observe/cancel, native optional features, bounded delegation | Session service plus identified backend |
| Local tools | Terminal, browser, computer-use integrations and their settings | Existing platform services and their permission boundaries |
| Administration | Accounts, provider configuration, models, downloads, plugins, updates, telemetry preferences, diagnostics | Existing account/configuration/installation services |
| Credential access | Discover authorized bindings, request/revoke scoped use, inspect sanitized status | Existing credential broker and secrets proxy |

Public parity means an agent can initiate and observe the same supported workflow under the same grants. Human-only OS authentication or approval steps remain explicit interaction requirements, not undocumented GUI rescue. No plan step authorizes Keychain access, raw secret exposure, purchases, publishing, or new external messages. Credential configuration in this initiative uses the already-authorized non-Keychain routes; any Keychain-backed product workflow remains outside the execution authorization of this work.

## Minimum common contracts

The following logical operations are mandatory. Final wire names are generated from domain definitions and preserve existing names where compatible.

| Contract | Required semantics |
| --- | --- |
| Capabilities list / describe | Bounded discovery, versions, supported features, availability reason, useful examples |
| Workspace snapshot | Authorized bounded domain summaries plus explicit revision/cursor references |
| Events read | Scoped `after` cursor, limit and byte bound, independent readers, explicit gaps |
| Operation / job get | Recoverable outcome, owning-domain state, provider references, pending decision |
| Job cancel | Idempotent cancellation request; eventual acknowledged state from owner |
| Resource read | Stable resource reference, media type, revision/digest, bounded range/paging |
| Visual create / update / bind | Validated definitions and bindings; expected revision for shared changes |
| View create / update | Explicit sharing, filters/selection/position, revision conflict handling |
| Surface present / close | Pane/window/full-screen target, request identity, renderer acknowledgement |
| Visual snapshot / capture | Semantic state and optional real image tied to exact revisions/cursors |
| Interaction read / submit | Independent observation; explicit recipient and deduplicated delivery |
| Agent start / send / observe / cancel | Backend identity, negotiated capabilities, supported resume/steer behavior |

For normal queries return bounded useful data directly. For a long operation return its durable reference promptly. For permission requirements return the pending decision and supported completion mechanism. Apply authorization to subsequent resource reads and event replay, not only to the initial tool invocation. Retained captures and diagnostic bundles have explicit retention, deletion, and credential redaction rules.

## Persistence and performance contracts

Reuse existing tables/journal wherever they already own the required state. Add migrations only for missing durable concepts: invocation deduplication/outcome, authenticated client grants if absent, visual views, presentation requests, surface generations/observations, and interaction delivery. Store large captures/artifacts through the existing content store; keep references in transactional records.

Do not create parallel job/event tables that compete with existing domain histories. Extend or project the existing owners. Every new durable record has a scope, retention policy, migration path, and deletion/cascade policy. Deletion retains only the minimum audit/deduplication evidence required by the declared retention contract. Cross-workspace moves are explicit operations with authorization at both ends.

The first performance baseline records hardware, cold/warm state, retained record counts, event volume, visual family, and actual client versions. Initial acceptance targets below are budgets to validate, not measurements already achieved:

- Warm local bounded read and control acknowledgement: p95 at or below 250 ms, excluding external provider work and process startup.
- Warm simple visual presentation: p95 at or below 1 second from accepted request to matching renderer acknowledgement.
- Event visibility: p95 at or below 500 ms on an attached local notification path; polling latency is reported separately.
- Standard summary responses: default page of 50 items and hard item/byte limits declared per operation; target at most 64 KiB serialized for ordinary summaries. Images and explicit artifact reads use separate declared bounds.
- Cursor reads and paged queries stay bounded as retained history increases; no eager full-journal replay on pane open or reconnect.
- The first workload suite includes at least two simultaneous real external clients, the integrated client, two independent views, and a real live event source. Document any resource limitation with measured evidence.

If a target fails, retain the failure and fix or explicitly revise the budget with evidence before release. Do not pass a regression by silently changing the workload or substituting fixtures.

## Incremental delivery

Phases below are ordered delivery gates. A phase is complete only when its stated proof is retained. The initial visual milestone is useful independently, but it does not count as full product parity.

| Phase | Responsible owner | Deliverable | Exit gate |
| --- | --- | --- | --- |
| P0 — inventory and baseline | Architecture / integration | Reproducible source inventory, capability ownership map, chosen base, real acceptance baseline | Every existing entry point is accounted for or explicitly marked unmapped; no inferred parity claim |
| P1 — authoritative operation foundation | Runtime / contracts | Typed domain registration, shared invocation context, generated discovery/bindings, one migrated visual operation | The same real handler is reached from desktop and MCP; schemas agree; obsolete handler/schema removed |
| P2 — complete visual journey | Visuals / presentation | Visual/view/surface/observation contracts, bidirectional interaction, public capture and full-screen control | Real external-agent journey including user selection, independent views, conflict and reconnect passes |
| P3 — independent local runtime | Runtime / platform | Single per-instance owner, attachable desktop, bridge and CLI, install/connect/doctor | Work survives GUI exit; restart reconciles; multiple clients share state; packaging proof |
| P4 — full domain migration | Individual domain owners | Every public operation migrated; complete MCP and CLI coverage; existing UX uses shared operations | Inventory closes with zero unimplemented public operations and zero private built-in-agent bypasses |
| P5 — agent hosting and delegation | Sessions / agent adapters | Native Codex and ACP adapters, bounded delegation, same public Workshop tools | Real supported backends pass session/control/permission journeys; unsupported features remain explicit |
| P6 — conformance and release | Integration / release QA | Full real-client matrix, performance evidence, upgrade proof, documentation and removal of obsolete paths | All completion criteria below pass on the chosen integration revision |

### P0: inventory and baseline

1. Record the exact commit, dirty-file delta if deliberately included, toolchain, instance paths, and source/client versions. Never merge the source checkout's unrelated dirty changes wholesale.
2. Extract existing Tauri registration, bridge command constants/calls, MCP tool definitions/dispatch, CLI commands, direct integrated-agent callbacks, and UI-owned mutations. Store source locations and extraction limitations. A static candidate mapping is evidence of declarations only; verify actual handler reachability before marking parity.
3. Map each operation to a domain and authority. Record duplicated schemas, generic tool families, aliases, transport-owned logic, hidden GUI rescue, and missing headless support.
4. Freeze the inventory and establish current behavior using a bounded real visual workflow. Record existing defects separately; do not fix unrelated feature work inside the architecture refactor.

### P1: operation foundation

1. Extend `platform/operations` rather than adding a second dispatcher and operation record. Establish authenticated actor/grant context separate from hosted chat/session correlation.
2. Define the initial visual operation beside its domain handler. Export input/output schemas and named MCP registration from that definition; retain existing compatible wire names.
3. Route Tauri and MCP through the typed operation boundary. Move eligibility, defaults, identity, and record mutation into its owner. Transport adapters translate envelopes only.
4. Introduce schema drift and dependency checks immediately for migrated operations. Legacy inventory items remain visibly pending until migrated; do not enforce an all-green baseline by exempting unknown operations.
5. Add revision/idempotency behavior at the operation's actual transactional boundary. Do not add unused framework primitives anticipating every possible domain.

### P2: visual reference slice

1. Migrate template discovery, create/bind/update/get and the existing shared presentation path.
2. Add explicit view records and surface attachment; migrate existing active-visual/session layout references with deterministic defaults and preserve current saved visuals.
3. Add requested-versus-observed presentation, renderer generation, observation freshness, and real capture evidence. Preserve existing quality gates and trusted-template rules.
4. Route filters, selection, replay position and full-screen changes through the common contract. Keep high-frequency local pointer movement local; commit meaningful state changes at documented interaction boundaries.
5. Add bounded app snapshots and independent event/interaction cursors. Route an explicit user submission to exactly one selected agent controller with delivery acknowledgement.
6. Demonstrate the external-agent reference journey and the same journey through the integrated experience. Preserve keyboard focus, escape/back behavior, compact layout and existing style tokens.

### P3: local runtime extraction and packaging

1. Remove Tauri/AppHandle dependencies from domain operations by introducing narrow presentation/process/platform ports implemented by the host.
2. Separate core assembly from desktop startup. The packaged runtime owns storage, providers, sessions, sidecars, grants and event journal. The desktop handles windows and native interaction.
3. Move instance ownership under the existing instance registry with startup locking, version checks, stale-process reconciliation and explicit shutdown semantics. Keep development and installed instances isolated.
4. Add the single Workshop MCP bridge and generated CLI. Connection registration preserves unrelated Codex/Claude client configuration; removal undoes only Workshop's owned entry. Do not store provider secrets in generated configuration.
5. Prove that closing a panel, closing the desktop, disconnecting a bridge and explicitly stopping the runtime have distinct documented effects.
6. Package and install an isolated development instance. Verify real executable discovery and startup without a source checkout, including native capture. Reuse signing/notarization workflows when a release is requested; do not publish as part of implementation authorization.
7. Ship client onboarding with one portable Workshop skill and thin Codex/Claude Code plugin packaging around the same MCP bridge. MCP-only clients must remain usable through generated tool descriptions and concise server instructions; the skill teaches workflows rather than defining operation semantics. Avoid duplicate registrations when switching between manual setup and a plugin.
8. Maintain the repository root README as the user-facing setup entry point. Replace its explicitly planned flow with verified installation and registration commands, workspace grants, client reload instructions, a rendered-visual smoke check, troubleshooting, upgrade and removal. Record tested client versions. A12 acceptance includes following those instructions in fresh Codex and Claude Code sessions, both with and without the skill; no source checkout, hosted session ID, provider credential import, or pasted system prompt may be required for the connection check. Keep implementation details authoritative in this plan rather than duplicating the architecture in the README.

### P4: migrate all product domains

Perform each domain as a reviewable slice with generated contracts, owning handlers, switched consumers, deleted duplicate paths, and real evidence:

1. Research records, data, evidence, annotations and artifact reads/exports.
2. Execution admission, approval, run controls, training and inference. Preserve immutable execution digests and authoritative provider lifecycles.
3. Workspace/access, preferences, local tools, diagnostics and migration operations.
4. Account/configuration, models/downloads, plugins, updates, telemetry and authorized credential-capability management.
5. Session lifecycle and remaining integrated-agent-only callbacks, preparing P5.

The inventory governs completeness across all five groups. A supported GUI action that can only open an external approval page is represented as an explicit workflow with its pending state; it must not claim the remote action is complete. Do not invent remote APIs or bypass product-enforced approvals to satisfy coverage.

### P5: hosted agents

1. Move existing Codex integration behind the backend adapter boundary without losing provider-specific features or session history.
2. Implement ACP using the documented protocol version selected at implementation time. Negotiate backend functions and preserve native event/approval details through typed extensions.
3. Expose public agent start/send/read/cancel plus supported resume/steer controls. Persist parent-child relationships, inherited scope, resource limits and cancellation policy for delegation.
4. Supply hosted agents the same Workshop MCP capability catalogue. Integrated context, approval cards, progress and visuals are projections of public records.
5. Verify reconnect, backend crash, uncertain send, cancel during work, waiting-for-user state, and nested delegation limits against real supported agents. No provider run starts without the applicable authorization.

### P6: integration and release gate

1. Reconcile with concurrent source-branch work by focused semantic merges. Regenerate all bindings after integration; do not hand-merge generated outputs.
2. Run conformance on the integrated revision and actual installed bundle. Remove obsolete ad-hoc schema catalogues, route-owned workflows and dormant alternate runtimes from the migrated production path.
3. Test schema migration from a retained real previous-version instance copy. Preserve the original instance. Forward migrations are transactional; downgrade requires a declared compatible version or restoring the untouched backup, not running an older binary against an incompatible database.
4. Publish the generated developer reference, client connection guide, capability/version matrix and troubleshooting guidance as repository artifacts. External publication is a separate authorized action.
5. Record exact results and remaining limitations. Any failed required gate keeps the initiative incomplete.

Dependency order: P0 -> P1 -> P2 -> P3 -> P4 -> P5 -> P6. Domain implementations can be independently prepared after P1, but they must integrate through the same registry and pass P3/P4 gates. No automatic delegation or parallel agents are required by this plan.

Wire-name preservation can use a declared, time-bounded alias invoking the exact same typed handler. This is protocol compatibility only, never an alternate execution path or a fallback after failure. Remove obsolete adapters and standalone hand-authored catalogues as clients migrate.

## Acceptance journeys and evidence

| ID | Real journey | Required evidence |
| --- | --- | --- |
| A1 | External Codex creates/binds/presents a visual and inspects actual capture | Invocation/result IDs, visual revision, renderer observation, real image |
| A2 | External Claude Code performs the same visual workflow | Same classes of evidence through Claude's actual MCP client |
| A3 | User selects an object, submits follow-up, target agent acts | Selection/view revision, submitted interaction, recipient, acknowledged delivery, resulting operation |
| A4 | Two external clients and integrated UI share a workspace with independent views | Distinct principals/cursors/views, shared document identity, correct per-view state |
| A5 | Concurrent updates and duplicated requests | One valid winning revision, typed conflict, deduplicated mutation/outcome |
| A6 | GUI exits, bridge disconnects, runtime restarts | Durable references, recovered state, honest provider reconciliation, no duplicate admitted work |
| A7 | Renderer delays/crashes; stale capture is requested | Pending/failed presentation, rejected stale observation, recovery at the correct revision |
| A8 | Cursor is outside retention or events outpace a client | Explicit gap/reset, bounded replay, complete snapshot plus continuation |
| A9 | Grant is revoked or approval/spec changes | Denied future access, no leaked replay/artifact, invalidated incompatible approval |
| A10 | Real bounded execution with live visual, cancellation and terminal evidence | Same declared spec/digest, provider receipts, cancellation acknowledgement, reconciled terminal state |
| A11 | Hosted Codex and ACP agent lifecycle/delegation | Backend/version identity, supported controls, bounded scope, accurate parent/child results |
| A12 | Installed instance setup, client registration, upgrade and removal | Configuration diff, version checks, database migration evidence, unrelated settings preserved |

No synthetic provider streams or fabricated receipts are used for these journeys. Schema/property checks and deterministic validation against actual request types are useful narrow checks, but do not substitute for the real workflow evidence above.

Use existing application-owned compile/contract checks for code changes. Keep new cross-product real-client acceptance and release automation in `workshop-release`, consistent with the existing consolidation guidance. Historical renderer tests using stubs are labeled as such and cannot establish real-system acceptance. Do not expand those stub paths to cover this initiative.

Existing commands observed in the implementation base include `npm run desktop:check`, `npm run test:visuals`, the Rust Specta export test, and `npm run desktop:verify`. Inspect what they invoke on the integration revision before relying on their result; old script names can outlive their implementations. Run the nearest meaningful checks during edits, cross-boundary checks at handoff, and full real installed acceptance at P6. Do not execute paid/provider-backed scripts implicitly through a broad umbrella command.

Each retained journey receipt identifies commit/diff, instance, client/backend versions, operation and resource IDs, authorization reference without secret values, timestamps, expected invariants, observed results, and artifact references. Record partial validation and failures explicitly. Real provider experiments follow the user's aggregate cost authorization policy; this plan itself incurs no provider charge.

## Risks and required mitigations

| Risk | Required mitigation / owner |
| --- | --- |
| Catalogue becomes a second application framework | Domain-owned handlers, minimal typed registration, no business logic in generator; architecture owner |
| Schema exporter cannot express Rust serialization precisely | Fail schema conformance early on the visual slice; support explicit validated domain schema constraints; contracts owner |
| Runtime extraction breaks desktop-only services | Narrow host ports and real packaging/capture tests before GUI lifetime claim; platform owner |
| Large MCP catalogue harms agent tool selection | Useful named tools, coherent domain descriptions, paginated discovery and observed task-level usability; agent interface owner |
| Existing authorization assumes internal chat sessions | Separate authenticated client grants from optional session correlation; access owner |
| Side effect succeeds before connection fails | Durable intent and provider reconciliation; no duplicate retry under unknown outcome; execution owner |
| Multi-window state or interaction routing races | Explicit view/surface IDs, expected revisions, single recipient and delivery identity; visuals/sessions owners |
| Refactor collides with ongoing source changes | Isolated base, per-domain patches and explicit integration diff review; integration owner |
| Core migration loses historical evidence | Transactional migration, retained original instance and real upgrade proof; persistence owner |

## Completion and change control

Completion requires all of the following:

- Every frozen-inventory public operation has an owning domain and generated MCP/desktop/CLI binding or an explicitly equivalent semantic UI mapping. Zero unexplained internal classifications or unimplemented public gaps remain.
- The integrated agent has zero private domain execution paths. Authenticated callers under equivalent grants get equivalent behavior.
- Visuals provide real full-screen presentation, capture, independent views, user interaction and reconnect from both Codex and Claude Code.
- One packaged runtime owns each instance, and admitted work survives GUI closure with proven crash recovery.
- Native Codex and at least one actual ACP-compatible backend pass their advertised supported lifecycle; unsupported backend features are disclosed.
- Existing Synth tokens, visual components, approval/evidence authorities and honest lifecycle semantics remain authoritative.
- Generated contracts and architectural checks pass, old duplicate production implementations are removed, real acceptance journeys pass, and the declared performance budgets are measured.
- The integration revision, migration path, client compatibility, receipts and remaining external limitations are documented. No required test is substituted by a fixture result.

Changes to scope, contract ownership or phase exit gates update this document and the generated inventory in the same review. Progress reports cite completed gates and real evidence; they do not equate scaffolding, tool count, compilation, or a screenshot with complete agent support.

There is no calendar estimate until P0 establishes the actual migration inventory. Work is delivered in the phase gates above, with the P2 visual milestone demonstrated early. Implementation authorization does not imply a merge, public release, or paid evaluation beyond the user's existing scope and budget rules.

## Enforcement and proof

- Generate and diff-check schemas, named tool registration, desktop bindings, and docs from the domain definitions.
- Enforce module dependency rules: transports cannot write domain tables or own eligibility and lifecycle rules. Domain modules cannot import host/protocol APIs.
- Fail coverage checks for any public operation lacking MCP exposure; fail for any integrated-agent tool that bypasses the public operation boundary. Semantic parity still requires end-to-end proof, not just matching method counts.
- Exercise the same real production path in isolated real instances through desktop and external clients. Compare authoritative outcomes and resource relationships, allowing naturally different IDs and timestamps across equivalent runs.
- Verify concurrent edits, idempotent retry, reconnect, cursor gaps, delayed render acknowledgement, stale capture rejection, and independent views using real state and process control. Do not substitute fabricated provider streams or receipts.
- Run real Codex and Claude Code journeys when authorized client credentials and budgets are available. Until then report structural/transport validation as partial, not proof of stellar agent usability. Do not incur provider charges merely to validate this design document.
- Track task completion without GUI rescue, calls per successful workflow, latency to useful result, and recoverability. A parity count alone does not measure tool quality.

## Definition of a new feature

A feature is complete when its domain contract, owning handler, bounded read view, honest lifecycle/events, authorization behavior, generated public surfaces, and real-system acceptance evidence exist. Its UI consumes that contract and existing Synth visual primitives. Adding a visual family or agent backend uses the same pattern.

The core invariant: one domain owner, one operation definition, one authoritative state transition, multiple generated interfaces.

## Protocol reference

MCP supports named tools, JSON-schema inputs/outputs, structured content, images, and resource links. These are useful projections of Workshop's contract; they do not define Workshop's business semantics. See [MCP tools specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools).
