# Credential locator registry

**Quality bar:** `strong_option` (trust boundary).  
**Parent noun:** `Identity → Credential` (`docs/V02_REFACTOR_NOTES_2026-08-11.md` §2).  
**Style:** Synth Style + `WORKSHOP_QUALITY_STYLE_GUIDE.md`.

## Job

The agent finds another `.env` (for example `runebench/.env` with `OPENROUTER_API_KEY`), Workshop uses it **in this session** without the agent ever reading the value, records the location so next session is a lookup not a hunt, and **workloads/containers get the existing proxy route + sentinel** — never the key copied into the container.

```text
this session                          next session
────────────                          ────────────
find filename (not cat .env)          locators_list → that slot
ask Workshop to use that file         boot may already have loaded preferred
operator Register                     if loaded → only “allow this run”
host reads, loads RAM                 same proxy into containers
remember the slot (locator)
allow this run (capability)
container: workshop-proxy + route
```

Launch `.env` is the fallback when nothing else is registered. It is not the product. The product is **the other file**.

A locator records **where a credential may exist**. It does not store, read, or authorize the value. Do not call a locator `ready`.

```text
Locator     remembered slot (so the agent can find it next time)
Source      license for Workshop to re-read that file (session + future boots)
Capability  this run (and its containers) may call through the proxy
```

---

## Locked decisions

Do not reopen these.

1. **One umbrella.** Keep `secrets_manage`. No second MCP server.
2. **Host custody.** Only the Rust host reads secret-bearing files. Agents discover filenames; they do not `cat` them.
3. **Workspace roots, not parked Projects.** Filesystem authority is `allowed_workspace_roots`. Do not invent `psrc_*`. Bind file locators to an allowed root (by `workspaceRootRef`) or the instance data root.
4. **Reuse consent, don’t fork it.** Evolve `ApprovalKind::CredentialAccess` with typed `CredentialConsent`. `requires_human()` stays true; `approval_policy = "never"` cannot auto-grant; `ApprovalScope::{Session,Workspace}` cannot remember it.
5. **Agent requests settle with `authorize_host`.** Settings is inspect / Forget / operator picker. A Settings click is operator consent and does **not** call `authorize_host` (that API needs a session).
6. **Parse at the adapter.** Serde structs in, Specta types out. No new `str_field` probing.
7. **Rewrite the path gate.** Do not promote today’s `canonicalize_import_path` (empty roots fail open; symlink check is a comment). Empty roots fail closed. Stat ≠ read. Re-gate on every RegisterSource.
8. **COMPAT is one slice.** Skill + `WORKSHOP_AGENTS.md` + `request_env_import` refuse ship together. `list` aliases to bindings without suffixes until the next minor.
9. **Do not store a Binding noun.** List endpoints project `sourceId` + `loaded`. `credential_bindings_list` means “sources whose value is in RAM this process.”
10. **`displayPath` is a projection.** Never persist a second path that can drift.
11. **Request is a ceiling.** Decision may equal or weaken the request, never exceed it. Agent that needs the value calls `source_request` (upserts locator). `locator_request` is remember-without-load only.
12. **Remember requires a regular file now.** `Missing` means it disappeared later. Cannot remember a `.env` that does not exist yet.
13. **Agent names a folder by `workspaceRootRef`.** `workspace_roots_list` returns `{ workspaceRootRef, displayName }`. Absolute paths never appear in MCP args or results.
14. **SQLite is locator authority.** Row + `locator.remember` receipt in one transaction. `[[desktop.credential_locators]]` is an inspectable export rewritten after commit. Settings reads SQLite.
15. **At most one loaded value per `(provider, variable)`.** If that pair is already loaded from **this** locator, skip Register (`request_use` only). RegisterSource on a **different** locator for the same pair is a switch: the card names the live locator; approve unloads the old RAM, revokes capabilities issued from it, loads the new, sets preferred.
16. **One locator = one slot.** `(kind, workspace_root_ref, path, variable, provider)`.
17. **Forget deletes the row.** No tombstone. Same key later = new consent, new id. Cap 64 live rows.
18. **Same key, different path, supersedes** the old row.
19. **FingerprintChanged fails closed.** No auto-reload. Next RegisterSource re-asks.
20. **`cat .env` is an accepted residual.** No Codex denylist in this slice. Skill is policy, not enforcement.
21. **Operator source card may show a masked suffix after inspect.** MCP, traces, locator list, and locator rows never do.
22. **Env var names: explicit parser**, not regex.
23. **Error codes remain string constants** on `CredentialError`.
24. **Launch-configured env auto-upserts** a locator **and** a source license. Starting Workshop with that file is consent to remember and re-read that instance `.env` at boot, not to issue a lease.
25. **CredentialSource is a durable license to re-read a locator, not the bytes.** Bytes stay in `EnvSourceStore` (RAM), reconstructed at process start from licensed locators. SQLite never holds the value. This is today’s `secret_refs` + `EnvSourceStore` with `locator_id` added — not a third invention.
26. **Boot loads only the preferred license** per `(provider, variable)` through the path gate. Same fingerprint → RAM. Missing file → ValueMissing / locator Missing, no RAM. FingerprintChanged → **do not load**, no fallback, no boot prompt.
27. **Vault `Secret` is not a Locator.** Settings paste stays `secret_refs` + backend. Managed code workflows still refuse vault as the eval credential. Do not wrap pasted keys in a fake file locator.
28. **Capability is the run grant; Lease is its compiled projection.** `CapabilityStore` is authority. `CredentialLease` (`workshop.credential-lease.v1`) remains the workload/env/manifest schema. Agents see handle + `provider_routes`, not two objects.
29. **`WorkspaceRootRef` is an identifier type**, not a table. Derived from current allowed roots. Hash the same canonical path the gate uses (APFS `/tmp` vs `/private/tmp`, case). No INSERT.
30. **Preferred source per `(provider, variable)`.** Last successful RegisterSource wins that slot at boot. Instance auto-license is preferred only if nothing else has been registered for the pair. Boot loads **only** preferred. FingerprintChanged / missing / unreadable → no RAM and **no fallback** to another license (`missing ≠ default`). Forget preferred: instance license becomes preferred if it exists, else none.
31. **RAM is sticky for the process.** A live capability keeps using the bytes already loaded. File edits do not re-read on each proxy call. Unload/restart required to pick up a new fingerprint (via RegisterSource).
32. **Unload revokes.** Forgetting, superseding, switching preferred, or revoking a source unloads RAM **and** revokes capabilities issued from that source.
33. **Serialize RegisterSource per `(provider, variable)`.** One in-flight consent (agent card or Settings). A second request gets `credential_source_consent_pending`. First settle wins.
34. **Re-gate on approve.** If the file vanished, became a directory, or escaped while the card was open, fail `credential_locator_not_regular_file` and write **no** license.
35. **Forget while a card is open** fails the pending `authorize_host`. Do not Register a deleted id.
36. **All live locator rows count toward 64**, including Proposed and ApprovalPending.
37. **Root re-added does not unrevoke.** Same digest, locators stay `WorkspaceAuthorityRevoked`. Settings: “This folder is allowed again. Forget and remember to restore.”
38. **Missing file returns:** boot Missing→Observed and load **only** if fingerprint matches the license. Different bytes → FingerprintChanged, no RAM.
39. **Duplicate `displayName`:** disambiguate with parent segment; if still tied, append 4 hex of the ref. Never an absolute path.
40. **Empty `KEY=` is ValueMissing**, not a loaded credential.
41. **Dotenv duplicates:** last assignment wins (same as today’s importer).
42. **Vault and env for the same provider are both visible.** Copy: pasted connections do not drive managed evals. Locators/sources do.

---

## Constraints

- Fail closed: path/symlink escape, unapproved workspace, picker mismatch, missing file at remember/register, value supplied, unknown enum, unknown transition, decision exceeding request.
- `missing ≠ default`. Missing locator is not `$HOME` search. Omitted `provider` on list is explicit “no filter.”
- Locators are instance-local. Not git, not cloud, not telemetry, not eval receipts, not MCP absolute paths.
- Renderer never writes the registry.
- Compatibility layers dated for removal.

## Non-goals

- Restoring parked Projects (`project.md`).
- Agent-visible values, suffixes, lengths, fingerprints, `.env` bodies, or canonical home paths.
- Broad filesystem discovery.
- Settings-only settlement for **agent** remember/register/use.
- Replacing the proxy/lease contract.
- Codex read denylist.

---

## Nouns

Parent: `Identity → Credential`. Meet the types that already exist. The only new stored noun is **Locator**.

```text
Credential
├── CredentialLocator     NEW     durable slot (where). Never a value. Never a license.
├── CredentialSource      EXISTS  durable license to re-read a locator
│                                 secret_refs + locator_id. Value never in SQLite.
├── Secret                EXISTS  vault paste. No filesystem slot. Not a Locator.
├── CredentialCapability  EXISTS  run grant (CapabilityStore). Authority for “this run.”
└── CredentialLease       EXISTS  compiled projection of a capability (env/manifest).

WorkspaceRootRef          identifier, not a row
EnvSourceStore            RAM cache of licensed values this process. Not a noun.
CredentialConsent         RememberLocator | RegisterSource | IssueLease
```

### Lifetimes

```text
                    survives restart     holds bytes     provider calls
Locator             yes                  no              no
Source license      yes                  no              no
EnvSourceStore      no                   yes             no
Capability          no (run-scoped)      no              yes, via proxy
Lease               no                   no              projection of capability
Secret (vault)      metadata yes         backend RAM     not for managed eval
```

### Locator

A remembered slot: kind + ref + path + variable + provider. RememberLocator creates this and **stops**. No `secret_refs` row, no RAM. `Observed` means the file existed when we remembered it. `Missing` means it is not a regular file **now**.

### Source

RegisterSource writes a **durable license** that Workshop may re-read this locator at every process start and when issuing a lease. It is not “loaded until quit.”

Bytes live only in `EnvSourceStore`. SQLite never holds them. This is today’s `secret_refs` + RAM store with `locator_id` added.

Boot loads **only the preferred** license per `(provider, variable)`:

```text
preferred = last successful RegisterSource for that pair
            else instance auto-license if it exists
for preferred only:
    re-run path gate
    file gone        → locator Missing, source ValueMissing, no RAM
    fingerprint same → put bytes in EnvSourceStore
    fingerprint new  → FingerprintChanged, no RAM, no fallback, no boot prompt
    workspace gone   → locator WorkspaceAuthorityRevoked, source Revoked, no fallback
other licenses for the pair stay licensed, loaded: false
```

Launch instance `.env` uses the same license, implied by starting Workshop with that file. It is preferred until the operator RegisterSources another locator for that pair.

Today’s `configured` / `loaded` / `validated` stay **projections** of license + RAM + last inspect. They are not extra nouns.

At most one loaded source per `(provider, variable)`. RAM key is already `envsrc:{provider}:{variable}`.

### Capability vs Lease

IssueLease mints a **Capability** (handle, run, ceilings). **Lease** is the existing workload schema. Do not store two grants. Do not call a locator or source a lease.

### Binding

Not a type. `credential_bindings_list` is the projection “sources with bytes in RAM this process.” Locator list fields: `sourceId`, `loaded` — not `bindingId`. Visual Binding is a different parent.

### Secret

Settings “Add connection” pastes into the vault. That is a Credential without a Locator. Eval/code workflows still refuse it. Locators are files and process env only.

A source license does not mint a capability. Every run still needs IssueLease.

---

## How it works

### Cross-section

```text
  AGENT (MCP)              OPERATOR                 WORKLOAD
  secrets_manage           Settings / cards         eval container
  refs + displayPath       inspect / Forget         sentinel + route
           │                      │                        │
           └──────────┬───────────┘                        │
                      ▼                                    │
              ┌───────────────┐                            │
              │  Rust host    │                            │
              │  path gate    │                            │
              │  authorize_   │                            │
              │  host (agent) │                            │
              └───────┬───────┘                            │
                      │                                    │
     ┌────────────────┼─────────────────┬──────────────────┤
     ▼                ▼                 ▼                  ▼
 Locator           Source            EnvSourceStore    Capability
 SQLite            license           RAM bytes         run grant
 where             secret_refs       envsrc:p:var      ──compile──► Lease
 no bytes          no bytes          this process      env/manifest
     │                │                 │
     └──── export ────┘                 │
     [[desktop.credential_locators]]    │
     (projection, not authority)        │
                                        ▼
                                   provider proxy
                                   (never sends key to agent)
```

Nothing on the left of RAM is a key. Nothing on MCP is an absolute home path.

### Boot (no cards)

```text
  start Workshop
       │
       │  auto-upsert instance .env locator + license
       │  (consent = launching with that file)
       ▼
  for each (provider, variable):
       preferred = last RegisterSource
                   else instance license
       ▼
  path gate + stat (no content yet)
       │
       ├─ gone / not file ──► Missing, ValueMissing, RAM empty
       ├─ fingerprint new ──► FingerprintChanged, RAM empty, no fallback
       ├─ workspace gone ──► WorkspaceAuthorityRevoked, no fallback
       └─ fingerprint same ─► READ file ─► EnvSourceStore
  other licenses stay licensed, loaded: false
       ▼
  ready for IssueLease if RAM has that pair
```

### Agent needs OpenRouter for RuneBench

```text
  workspace_roots_list
       │  [{ ref: wsroot_a1b2, displayName: "runebench" }]
       ▼
  bindings_list          any RAM for openrouter + OPENROUTER_API_KEY?
       │
       ├─ YES, and it's this locator ──► request_use ──► IssueLease card
       │                                      │
       │                                      ▼
       │                                 wcap + provider_routes
       │                                 OPENAI_API_KEY=workshop-proxy
       │
       ├─ YES, but a *different* locator (instance .env)
       │     source_request(RuneBench locator)
       │          card: "OpenRouter is loaded from Workshop launch .env.
       │                 Register runebench/.env instead?"
       │          Register ─ unload instance, revoke its caps,
       │                     load RuneBench, set preferred
       │          then request_use
       │
       └─ NO RAM
            locators_list
            ├─ Observed, licensed, loaded false (FingerprintChanged / missing)
            │     source_request(locatorId) ──► RegisterSource card
            └─ no locator
                  source_request(wsroot_a1b2, ".env", OPENROUTER_API_KEY, openrouter)
                       gate: file must exist (stat, no read)
                       RegisterSource card
                         Cancel
                         Remember only  → locator Observed, no license, no RAM
                         Register       → locator + license + READ + preferred + RAM
                       then request_use if Register
```

### Cards (request is a ceiling)

```text
  RememberLocator              RegisterSource                 IssueLease
  ┌─────────────────────┐      ┌──────────────────────────┐   ┌─────────────────┐
  │ Remember this       │      │ Register this location?  │   │ Allow this run  │
  │ location?           │      │                          │   │ to use OpenRouter│
  │ runebench/.env      │      │ runebench/.env           │   │ through Workshop│
  │ OPENROUTER_API_KEY  │      │ may show ••••7F2A        │   │                 │
  │                     │      │                          │   │                 │
  │ Does not read.      │      │ Host will read the file. │   │ Not the key.    │
  │                     │      │                          │   │                 │
  │ [Cancel]            │      │ [Cancel]                 │   │ [Cancel]        │
  │ [Remember location] │      │ [Remember only]          │   │ [Allow once]    │
  │                     │      │ [Register]               │   │                 │
  └─────────────────────┘      └──────────────────────────┘   └─────────────────┘
         no Register              downgrade allowed              once only
         button                   cannot IssueLease here
```

Settings Register / Forget / picker: same host functions, no `authorize_host`. The click is consent.

### Preferred switch

```text
  (openrouter, OPENROUTER_API_KEY)

  instance .env          runebench/.env
  license ✓              license ✓
  preferred ●            preferred ○          RAM = instance
         │                      │
         │   RegisterSource on RuneBench, operator Register
         ▼                      ▼
  preferred ○            preferred ●          RAM = RuneBench
  loaded false           loaded true
  caps from instance revoked

  next boot: load RuneBench only
  RuneBench fingerprint changed: RAM empty, instance does NOT fill in
  Forget RuneBench: preferred falls back to instance
```

### Path gate (stat ≠ read)

```text
  MCP: wsroot_a1b2 + ".env"
           │
           ▼
  ref → current allowed root?  ──no──► unapproved_workspace
           │ yes
           ▼
  join, reject "..", walk each component
  every symlink stays under root ──no──► path_escape
           │
           ▼
  final target is regular file? ──no──► not_regular_file
           │
  RememberLocator: STOP (no read)
  RegisterSource / boot preferred: READ bounded bytes, parse dotenv
```

External files: operator picker only. Canonical picker path must equal requested path. Never MCP.

### Who may see what

```text
                    locator   displayPath   suffix   bytes   handle
  SQLite            yes       no            no       no      cap meta
  TOML export       yes       ~ form ext    no       no      no
  MCP               yes       yes           no       no      yes (lease)
  Settings locator  yes       yes           no       no      —
  Settings source   yes       yes           yes*     no      —
  traces/telemetry  no        no            no       no      no
  workload          no        no            no       no      sentinel+route

  * after host inspect, operator Register card only
```

---

## What is wrong today

```text
list                 → aliases + ••••suffix
request_env_import   → host reads now, parks in Settings
request_use          → authorize_host          ← copy this
import_roots()       → instance + $HOME
canonicalize_import  → empty roots fail open; symlink check empty
```

---

## Persistence

**Authority:** SQLite table `credential_locators` (instance db). Insert/update/delete of an Observed row and `locator.remember` / `locator.forget` audit happen in one transaction.

**Export:** after commit, rewrite `[[desktop.credential_locators]]` from SQLite. Export has ids, kind, refs, relative paths, labels, state — not values, suffixes, fingerprints, or canonical home paths. If export I/O fails, the SQLite row still stands; log a typed error. Boot does not treat TOML as authority.

Internal columns include the canonical workspace path so a ref can be recomputed; that path never crosses MCP.

```toml
# export only — not authority
[[desktop.credential_locators]]
id = "credloc_runebench_openrouter"
kind = "workspace_env_file"
workspace_root_ref = "wsroot_a1b2c3d4"
path = ".env"
format = "dotenv"
provider = "openrouter"
variable = "OPENROUTER_API_KEY"
label = "RuneBench OpenRouter"
state = "observed"
last_seen_at = "2026-08-27T15:00:00Z"
```

```toml
[[desktop.credential_locators]]
id = "credloc_launch_openrouter"
kind = "process_environment"
provider = "openrouter"
variable = "OPENROUTER_API_KEY"
label = "Workshop launch OpenRouter"
state = "observed"
```

```toml
[[desktop.credential_locators]]
id = "credloc_external_openrouter"
kind = "external_env_file"
path = "~/.config/provider/credentials.env"
provider = "openrouter"
variable = "OPENROUTER_API_KEY"
state = "observed"
```

External export path is privacy-preserving (`~` form). SQLite keeps the canonical path for the picker exact-match.

**Upsert key:** `(kind, workspace_root_ref | instance | canonical-external, path, variable, provider)`.

**`workspaceRootRef`:** `wsroot_` + hex digest of the canonical allowed-root path. List of refs is derived from current `allowed_workspace_roots`. A ref whose root was removed is invalid; locators → `WorkspaceAuthorityRevoked`.

| Bound | Value |
| --- | --- |
| live locators | 64 |
| pending consents | 4 |
| relative path bytes | 256 |

---

## States

Same `can_transition_to` shape as `SessionStatus`. Unknown transitions fail.

```text
Locator
  Proposed → ApprovalPending | Removed
  ApprovalPending → Observed | Removed
  Observed → Missing | WorkspaceAuthorityRevoked | Superseded | Removed | ApprovalPending
  Missing → Observed | WorkspaceAuthorityRevoked | Removed | Superseded
  WorkspaceAuthorityRevoked → Removed
  Superseded → Removed
  Removed → (terminal; row deleted, not stored)

Source
  ApprovalPending → Active | Revoked
  Active → ValueMissing | FingerprintChanged | Unreadable | Revoked
```

Deny of a remember card **deletes** the pending row (`Removed`), not a return to Proposed.

| Word | Means |
| --- | --- |
| Observed | file existed at remember/register; we still remember the slot |
| Missing | it was Observed; it is not a regular file **now** |
| Active | host holds a loaded value |
| WorkspaceAuthorityRevoked | that `workspaceRootRef` is no longer allowed |

`Observed` requires the audit receipt in the same transaction.

---

## Path gate

New owner in `secrets/locator.rs` (or a dedicated `path_gate` next to it). Importer calls it. `$HOME` is not a root.

```text
Discovery              Remember                         Load
─────────              ────────                         ────
workspace_roots_list   RememberLocator after gate       RegisterSource after re-gate
instance data root     file must be a regular file      read contents (host only)
exact picker file      external: picker exact-match
```

| Kind | Agent sends | Host stores (SQLite) |
| --- | --- | --- |
| `workspace_env_file` | `workspaceRootRef` + relative path | ref + relative path + canonical root (internal) |
| `instance_env_file` | relative path under data root | relative path |
| `process_environment` | variable | variable |
| `external_env_file` | proposed path (Settings/picker; not MCP) | canonical path |

MCP **must not** accept `external_env_file`. External remember is operator picker only.

Gate: ref resolves to a current allowed root; relative path has no `..` and is ≤ 256 bytes; walk/stat every component; each symlink target stays under the root; final target is a regular file; variable parser accepts; no secret-shaped fields. Content is not read on RememberLocator.

RegisterSource re-runs the gate, then reads.

---

## Consent

```rust
ApprovalKind::CredentialAccess {
    consent: CredentialConsent, // requested ceiling
    provider: String,
    locator_id: Option<String>,
    reason: String,             // copy only
}

enum CredentialDecision {
    Reject,
    RememberLocator,
    RegisterSource,
    IssueLease,
}
```

`validate_decision`: outcome ≤ request.

| Request | Card | Allowed outcomes |
| --- | --- | --- |
| RememberLocator | Cancel · Remember location | Reject, RememberLocator |
| RegisterSource | Cancel · Remember only · Register | Reject, RememberLocator, RegisterSource |
| IssueLease | Cancel · Allow once | Reject, IssueLease |

RememberLocator **never** reads. RegisterSource upserts the locator, then reads. “Remember only” on a RegisterSource card is the downgrade: persist slot, do not load.

Agent that needs the value: `source_request`, not `locator_request`.

`approval_policy = "never"` does not auto-approve any of these.

---

## MCP

Forbidden keys rejected before IPC. `additionalProperties: false`.

```text
workspace_roots_list                  { workspaceRootRef, displayName }[]
credential_bindings_list              Active sources; no paths required
credential_locators_list              displayPath only; sourceId + loaded
credential_locator_request            RememberLocator; ref + relative path
credential_locator_request_status     resume pending
credential_locator_remove             Forget; confirm if past Proposed
credential_source_request             RegisterSource; locatorId XOR (ref + path + variable + provider)
credential_source_status
credential_source_remove
request_use                           IssueLease; locatorId XOR sourceId XOR secretId
                                      locatorId allowed only if binding Active, else error
```

```text
COMPAT (same slice as skill rewrite)
  list               → bindings_list, suffixes stripped; kill next minor
  request_env_import → refuse credential_locator_compat_import
```

Happy path:

```text
AGENT                         HOST                              OPERATOR
  workspace_roots_list ───────► refs + display names
  bindings_list
       │
       ├─ loaded this process  → request_use ─ IssueLease card ────►
       │
  locators_list
       ├─ Observed, loaded false
       │     source_request(locatorId)
       │          RegisterSource card
       │            Register → load, then request_use
       │            Remember only → Observed, no load
       │
       └─ none, needs the value
             source_request(workspaceRootRef, relativePath, variable, provider)
                  gate: file must exist
                  RegisterSource card (upserts locator)
                  then request_use
```

`locator_request` is only when the agent should remember without loading (rare). Same gate: file must exist. Card has no Register button.

List result:

```json
{
  "locators": [
    {
      "locatorId": "credloc_runebench_openrouter",
      "label": "RuneBench OpenRouter",
      "kind": "workspace_env_file",
      "workspaceRootRef": "wsroot_a1b2c3d4",
      "displayPath": "runebench/.env",
      "variable": "OPENROUTER_API_KEY",
      "provider": "openrouter",
      "state": "observed",
      "sourceId": null,
      "loaded": false
    }
  ]
}
```

`workspace_roots_list` example:

```json
{
  "workspaceRoots": [
    { "workspaceRootRef": "wsroot_a1b2c3d4", "displayName": "runebench" }
  ]
}
```

No absolute paths. `sourceId` / `loaded` are joins onto the license + RAM store, recomputed.

`request_use` with a `locatorId` whose source is not Active returns `credential_source_unconfigured` (or `credential_value_unloaded`) — it does not silently RegisterSource.

---

## Settings UX

Reuse `SettingsCard` / `secrets-row`. Sentence case. Monospace for display paths and variable names. No suffixes on locator rows.

```text
┌─ Known locations ─────────────────────────────────────────────┐
│  RuneBench OpenRouter                                         │
│  runebench/.env · OPENROUTER_API_KEY                          │
│  Location remembered · credential not currently registered    │
│                               [Register source] [Forget]      │
│                                                               │
│  Workshop launch OpenRouter                                   │
│  Process environment · OPENROUTER_API_KEY                     │
│  Available only when supplied at launch                       │
│                               [Register for session] [Forget] │
└───────────────────────────────────────────────────────────────┘
```

Operator Register / Forget / picker = host functions, `origin = operator`, no `authorize_host`. Forget copy: “Workshop will forget this location. It will not delete the file.” Disabled Register: file missing or workspace revoked. Picker is the only way to remember an `external_env_file`.

---

## Errors

Map once at MCP/IPC to `AppError`. No substring classifiers.

| Code | Layer | Meaning |
| --- | --- | --- |
| `credential_locator_unapproved_workspace` | locator | ref is not a current allowed root |
| `credential_locator_path_escape` | locator | relative path or symlink left the root |
| `credential_locator_not_regular_file` | locator | absent, directory, or not a file (remember and register) |
| `credential_locator_value_supplied` | locator | secret-shaped field |
| `credential_locator_picker_mismatch` | locator | picker ≠ requested external path |
| `credential_locator_broad_discovery` | locator | search outside allowed roots |
| `credential_locator_compat_import` | locator | `request_env_import` retired |
| `credential_locator_limit` | locator | bound exceeded (pending rows count) |
| `credential_locator_decision_exceeds_request` | locator | card outcome > request ceiling |
| `credential_source_consent_pending` | source | another RegisterSource is in flight for this pair |
| `credential_source_unconfigured` | source | keep; also `request_use(locatorId)` with no loaded source |
| `credential_value_missing` | source | keep; includes empty `KEY=` |
| `credential_value_unloaded` | source | keep; includes FingerprintChanged at boot |

Pair assertions: SQLite write + audit in one txn, read back kind/path/state. Gate: under-root after walk; symlink-out and `..` refuse. Remember of a missing file errors and writes nothing. MCP round-trip contains no absolute home path.

---

## Edge cases (locked)

Identity and time, not extra nouns.

**Switching keys.** RegisterSource on locator B while A is live for the same pair is a replace, not a silent refuse. Card: “OpenRouter is currently loaded from {A display}. Register this location instead?” Approve: unload A, revoke A’s capabilities, load B, B becomes preferred. Boot will keep loading B.

**Boot does not hunt.** Preferred fails closed. Instance `.env` does not silently take over because RuneBench’s file rotated. Forget of preferred *does* fall back to instance license if one exists — that is explicit operator intent.

**Sticky RAM.** Editing `.env` during a run does not rotate the proxy key. Restart or RegisterSource again. Proxy must not re-read the file per call.

**Races.** One RegisterSource in flight per pair. Settings Forget of the pending id aborts the card. Approve always re-gates. Unload always revokes capabilities from that source.

**Refs.** `displayName` collisions get `parent/name` or 4 hex of the ref. Re-adding a workspace root does not resurrect revoked locators. Canonical path for hashing is the gate’s canonical path.

**Counts.** Proposed and ApprovalPending consume the 64 cap so agents cannot spam remember cards.

**Empty values.** `KEY=` is missing, not loaded. Last dotenv assignment wins.

**Vault vs file.** Both can appear in Settings. Managed evals still only use a loaded env source.

**Accepted residuals.** `cat .env`; `displayPath` in MCP transcripts; stale `locatorId` after Forget (re-list).

---

## Privacy

- SQLite instance-local; export TOML has no canonical home paths.
- MCP: refs + `displayName` / `displayPath` only.
- Traces, telemetry, eval receipts: no locator lists.
- Audit: id, kind, variable, provider; no path in exported reports.

---

## Landing zone

```text
storage/migrations.rs                credential_locators table
secrets/locator.rs                   registry, states, refs, export rewrite
secrets/path_gate.rs                 walk/stat/symlink; fail closed
secrets/importer.rs                  read on RegisterSource only; calls path_gate
secrets/lease.rs                     source + locator_id; error constants
secrets/mod.rs                       specta commands

session/approval.rs                  CredentialAccess { consent }; CredentialDecision
visuals_ipc.rs                       typed dispatch; authorize_host for agent path
bin/synth_secrets_mcp.rs             operations; forbidden keys; COMPAT

SecretsSettings.tsx                  Known locations; operator origin
sessionView.ts                       consent-keyed copy
skills/use-synth-secrets/SKILL.md    bindings → source_request → request_use
context/WORKSHOP_AGENTS.md           no cat .env; no absolute sourcePath
```

Specta emits renderer types. Do not hand-mirror.

---

## Tests (one purpose each)

- SQLite round-trip has no value/suffix/fingerprint; export TOML neither
- export I/O failure leaves the SQLite row + receipt intact
- `workspace_roots_list` has displayName, no absolute path
- MCP locator_request with an absolute path is rejected
- `..` and symlink escape refuse
- empty allowed roots refuse
- `$HOME/.env` without picker → `credential_locator_unapproved_workspace`
- remember of a missing file errors and inserts no row
- RememberLocator does not load `env_sources`
- RegisterSource upserts locator; Remember-only on that card does not load
- RememberLocator card cannot Register (decision exceeds request)
- loaded from this locator → request_use does not open RegisterSource
- RegisterSource on a second locator for the same pair switches preferred and revokes old capabilities
- boot loads only preferred; FingerprintChanged does not fall back to instance env
- Forget preferred falls back to instance license when it exists
- empty `KEY=` does not enter EnvSourceStore
- duplicate displayName is disambiguated without an absolute path
- workspace root re-add leaves locators WorkspaceAuthorityRevoked
- Missing file restored with same fingerprint loads at boot; different fingerprint does not
- file deleted while RegisterSource card is open → approve writes no license
- second concurrent RegisterSource for the same pair → credential_source_consent_pending
- unload/Forget revokes capabilities issued from that source
- pending locator rows count toward the 64 cap
- `request_use(locatorId)` without Active source errors, does not load
- removing a workspace root revokes locators and unloads sources
- FingerprintChanged does not auto-reload
- `request_env_import` refuses with COMPAT code
- `approval_policy = "never"` does not grant RememberLocator
- unknown transition fails

Playwright: Known locations empty, Register, Forget, missing-file disabled Register. CUA RememberLocator and RegisterSource cards on the real Desktop path.

---

## Implementation order

1. SQLite table + states + `workspaceRootRef` + rewritten path gate (no MCP).
2. Auto-upsert locator when launch env / existing source is already loaded; TOML export.
3. Settings Known locations (SQLite projection; operator Register/Forget).
4. `CredentialConsent` + `CredentialDecision`; blocking agent MCP including `workspace_roots_list`.
5. `source_request` (locatorId or ref+path); no suffixes in tool results.
6. COMPAT refuse `request_env_import`; rewrite skill; drop `$HOME` root; `list` alias without suffixes.

Paid-eval proxy/lease contract unchanged.
