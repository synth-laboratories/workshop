# Handoff: credential locator registry

**Status:** design locked, **no implementation started**.  
**Do not reopen** `docs/CREDENTIAL_LOCATOR_REGISTRY.md` (locked product + engineering). This note is how to land it.  
**Worktree:** `workshop-v08-run-kernel`.  
**Quality bar:** `strong_option`. Parent noun: `Identity → Credential`.

If gold Craftax is involved anywhere, rust GameBench gold only. File work only under `/Users/joshuapurtell/Github`.

---

## Job (one sentence)

Agent finds another project `.env` (e.g. `runebench/.env` / `OPENROUTER_API_KEY`) without reading it; Workshop loads it into the **existing** RAM `EnvSourceStore` + proxy/sentinel; SQLite remembers the **slot** so next session is a lookup. Launch `.env` is the **fallback**, not the product.

Nothing about this is a new vault, encryption layer, or container key injection.

---

## What already exists (reuse, do not fork)

| Piece | Where | Role after this slice |
| --- | --- | --- |
| `EnvSourceStore` | `secrets/lease.rs` | RAM bytes, key `envsrc:{provider}:{variable}`. Sticky for the process. **Add `remove`.** |
| `secret_refs` | SQLite, `secrets/vault.rs` | Source **license** metadata. Add `locator_id`, `preferred`, `source_state`. Never the value. |
| `ProviderProxy` | `secrets/proxy.rs` | Unchanged. Must **not** re-read the file per call. |
| `CapabilityStore` | `secrets/capability.rs` | Run grant. **Add revoke-by-`secret_id`.** Unload/Forget/switch revokes caps from that source. |
| `CredentialLease` | `secrets/lease.rs` | Unchanged compile-to-env/manifest. |
| `secrets_manage` MCP | `bin/synth_secrets_mcp.rs` + `visuals_ipc.rs` `dispatch_secrets` | One umbrella. New operations, COMPAT `list` / refuse `request_env_import`. |
| `ApprovalKind::CredentialAccess` | `session/approval.rs` | Evolve in place. `requires_human()` stays. `approval_policy=never` cannot auto-grant. |
| `authorize_host` | `ApprovalBroker` | Agent Remember/Register/IssueLease settle here. Settings clicks do **not**. |
| Path gate today | `importer::canonicalize_import_path` | **Throw away.** Empty roots fail **open**; symlink check is a comment. |
| `import_roots()` | `secrets/mod.rs` | Instance data root **+ `$HOME`**. Drop `$HOME`. |
| `load_configured_env_sources` | `lease.rs` + `core_runtime.rs` boot | Today loads **every** canonical provider from launch `.env`. Must become: auto-upsert instance locator+license, then load **only preferred**. |

Vault paste (`secrets_create`) stays. It does not drive managed evals.

---

## Slice order (do not skip)

1. `path_gate.rs` + `locator.rs` + SQLite `MIGRATION_51` + tests. **No MCP.**
2. Boot: auto-upsert instance locator+license; preferred load; TOML **export** rewrite.
3. Settings Known locations (operator Register/Forget/picker). No `authorize_host`.
4. `CredentialConsent` / `CredentialDecision`; blocking agent MCP including `workspace_roots_list`.
5. `source_request`; occupancy switch; no suffixes on MCP.
6. COMPAT refuse `request_env_import`; rewrite skill + `WORKSHOP_AGENTS.md`; `list` → bindings without suffixes.

---

## Concrete wiring the design doc does not spell

### Migration

Latest is **50**. Add **51**. Also:

- Put `credential_locators` in `REQUIRED_TABLES` (heal `CREATE TABLE IF NOT EXISTS`).
- `heal_missing_columns` for `secret_refs.locator_id`, `preferred`, `source_state` (lineage collisions).

Suggested shape:

```sql
CREATE TABLE credential_locators (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN (
    'workspace_env_file','instance_env_file','process_environment','external_env_file')),
  workspace_root_ref TEXT,
  workspace_canonical TEXT,   -- internal only; never MCP
  relative_path TEXT,
  external_canonical TEXT,    -- picker exact-match; never MCP
  format TEXT NOT NULL DEFAULT 'dotenv',
  provider TEXT NOT NULL,
  variable TEXT NOT NULL,
  label TEXT NOT NULL,
  state TEXT NOT NULL,
  upsert_key TEXT NOT NULL UNIQUE,
  last_seen_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

ALTER TABLE secret_refs ADD COLUMN locator_id TEXT REFERENCES credential_locators(id);
ALTER TABLE secret_refs ADD COLUMN preferred INTEGER NOT NULL DEFAULT 0;
ALTER TABLE secret_refs ADD COLUMN source_state TEXT;
```

`upsert_key = kind|root-or-instance-or-ext-canonical|path|variable|provider`. SQLite UNIQUE treats NULLs as distinct, so do **not** unique the nullable columns.

Cap: **64 live rows including Proposed/ApprovalPending**. Pending consents cap **4**. Relative path ≤ **256** bytes. Forget **deletes** the row (no tombstone).

### Path gate (`secrets/path_gate.rs`)

- Empty allowed roots → **fail closed**.
- Hash `workspaceRootRef` as `wsroot_` + hex of **gate-canonical** allowed-root path (APFS `/tmp` vs `/private/tmp` matters). Identifier type, not a table.
- MCP sends `workspaceRootRef` + relative path. **No absolute paths in MCP args or results.**
- Walk every component with `symlink_metadata`; each symlink target must stay under the canonical root; `..` refuses.
- Stat ≠ read. RememberLocator stops after “regular file now.”
- Env var names: explicit parser (ASCII alnum + `_`, start letter/`_`). Not regex. Importer’s current parser allows a leading digit — fix when you call the gate.
- Empty `KEY=` = ValueMissing (do not `EnvSourceStore.put`). Dotenv last assignment wins.
- MCP **must not** accept `external_env_file`. External = native picker only; canonical picker path **equals** requested path.
- `$HOME` is not a root.

### Locator states

Same `can_transition_to` shape as `SessionStatus` (`domain/session_run.rs`). Unknown transition fails.

```
Proposed → ApprovalPending | Removed
ApprovalPending → Observed | Removed
Observed → Missing | WorkspaceAuthorityRevoked | Superseded | Removed | ApprovalPending
Missing → Observed | WorkspaceAuthorityRevoked | Removed | Superseded
WorkspaceAuthorityRevoked → Removed
Superseded → Removed
Removed → row deleted, not stored
```

Deny of a remember card **deletes** the pending row. Same key later = new id.

Root re-added does **not** unrevoke. Copy: “This folder is allowed again. Forget and remember to restore.”

Duplicate `displayName`: parent segment, then 4 hex of the ref. Never `$HOME` / absolute path.

### Boot / occupancy

- Launch-configured env auto-upserts **locator + source license** (consent = launching with that file). Not a lease.
- Preferred = last successful RegisterSource for `(provider, variable)`, else instance auto-license.
- Boot loads **only preferred**. FingerprintChanged / missing / unreadable → **no RAM, no fallback** to instance (`missing ≠ default`).
- Forget preferred → instance license becomes preferred if it exists.
- Same locator already loaded → skip Register, `request_use` only.
- Different locator, same pair → **switch card** (name the live one). Approve: unload old, **revoke its capabilities**, load new, set preferred.
- Serialize RegisterSource **one in flight per pair**; second → `credential_source_consent_pending`.
- Re-gate on approve. File gone → `credential_locator_not_regular_file`, write **no** license.
- Forget while card open: fail pending `authorize_host` (expire matching `CredentialAccess { locator_id }`). Do not Register a deleted id.

`upsert_env_source_descriptor` today ignores `env_file` / `variable` after insert (`lease.rs`). Wire `locator_id` there.

### TOML export

SQLite is authority. After commit, rewrite `[[desktop.credential_locators]]` from SQLite. **Boot must not read TOML as locators.**

`synth_config::write_toml` is **private**. Add something like `rewrite_credential_locator_export`. Export I/O failure: SQLite row still stands; log a typed error. Export: ids, kind, refs, relative paths, labels, state — **no** values, suffixes, fingerprints, canonical home paths. External export path in `~` form only.

### Errors

Keep `CredentialError` string constants in `lease.rs`. Map at MCP/IPC via `StructuredFailure` (`error.rs`) so the **code** survives `AppError::from`. Do not substring-classify.

New codes (design doc table):  
`credential_locator_unapproved_workspace`, `path_escape`, `not_regular_file`, `value_supplied`, `picker_mismatch`, `broad_discovery`, `compat_import`, `limit`, `decision_exceeds_request`, `credential_source_consent_pending`.

Existing: `credential_source_unconfigured`, `credential_value_missing`, `credential_value_unloaded`.

`request_use(locatorId)` with no loaded source → unconfigured/unloaded, **do not** silently Register.

---

## Consent (the part that will eat the most calendar)

Today:

```rust
ApprovalKind::CredentialAccess { provider, purpose }
ApprovalDecision { Reject | Approve { scope } | ApproveWithCap }
authorize_host(...) -> Result<String>  // drops the decision
```

Need:

```rust
ApprovalKind::CredentialAccess {
    consent: CredentialConsent, // requested ceiling
    provider: String,
    purpose: String,            // copy only; keep name so existing UI keeps working
    locator_id: Option<String>,
    display_path: Option<String>,
    variable: Option<String>,
    switch_from_display: Option<String>,
}
enum CredentialConsent { RememberLocator, RegisterSource, IssueLease }
enum CredentialDecision { Reject, RememberLocator, RegisterSource, IssueLease }
```

**Do not put a masked suffix on `ApprovalKind` / `safe_payload`.** That JSON is journaled. Operator Settings may show a suffix after inspect; MCP, traces, locator rows, and approval events never do.

`validate_decision` is **not** a total order:

| Request | Allowed outcomes |
| --- | --- |
| RememberLocator | Reject, RememberLocator |
| RegisterSource | Reject, RememberLocator, RegisterSource |
| IssueLease | Reject, IssueLease |

Wire through existing `CodexApprovalDecisionRequest.decision: String` (do not add a Specta field unless you must):

| UI | wire | meaning |
| --- | --- | --- |
| Cancel | `reject` | Reject |
| Remember location / Remember only | `remember-locator` | RememberLocator |
| Register | `register-source` | RegisterSource |
| Allow once | `once` | requested ceiling (IssueLease, or Register if that was the request) |

**Must change** `useAppController.ts` — it currently collapses every non-`always` approve to `"once"`:

```ts
const decision = kind === "reject" ? "reject" : payload.decision === "always" ? "always" : "once";
```

Pass `remember-locator` / `register-source` through. Extend `SecretsBridge` / `ChatTranscript` `onApprove` the same way.

**Must change** `authorize_host`: add `authorize_host_outcome -> Result<(String, ApprovalDecision)>` so MCP `source_request` can tell Remember-only vs Register. Existing `authorize_host` can wrap it and keep treating any non-Reject as Ok (IssueLease callers).

Construction sites to update (today `{ provider, purpose }`):

- `lib.rs` optimizer start
- `visuals_ipc.rs` `/v1/secrets/use`
- `session/approval.rs` tests
- `session/approval_policy.rs` tests

`requires_human()` already matches `CredentialAccess`; keep it. `remembered_key` already returns `None` for human kinds.

Expire-on-Forget: add `expire_credential_locator(locator_id)` next to `expire_origin`.

---

## MCP / IPC traps

`dispatch_secrets` **path denylist** matches substrings `delete`, `get`, `value`, `export`, `grant`, …  
Do **not** name routes `/v1/secrets/locator_delete` or anything with `get`/`value`. Suggested:

```
POST /v1/secrets/workspace_roots
POST /v1/secrets/bindings
POST /v1/secrets/locators
POST /v1/secrets/locator_request
POST /v1/secrets/locator_status
POST /v1/secrets/locator_remove
POST /v1/secrets/source_request
POST /v1/secrets/source_status
POST /v1/secrets/source_remove
POST /v1/secrets/use          (existing; locatorId XOR sourceId XOR secretId)
POST /v1/secrets/list         COMPAT → bindings, no suffixes
POST /v1/secrets/import       COMPAT → refuse credential_locator_compat_import
```

Plan: **parse at the adapter with serde structs + `deny_unknown_fields`**. Strip `operation` / `sessionRef` then deserialize. Do not add more `str_field` probing.

`request_use` today **requires `secretId`**. Extend XOR. `locatorId` only if that source is loaded.

Agent sequence: `workspace_roots_list` → `bindings_list` → `locators_list` → `source_request` → `request_use`.

`cat .env` is an **accepted residual**. No Codex denylist this slice.

---

## Specta / renderer

Commands go in `secrets/mod.rs`, `contract/commands.rs`, `contract/specta.rs` `collect_commands!`.

`export_specta_protocol_bindings` asserts **277** invoke commands and diffs `src/renderer/src/generated/protocol.ts`. After adding commands:

```
cargo test -p synth-desktop --lib regenerate_protocol_bindings -- --ignored
```

Bump the 277 count with a comment. Do **not** hand-mirror `env.d.ts`.

Wire Settings through `desktopBridge.ts` + `bridge/types.ts` `SecretsBridge`.

---

## Settings / cards

`SecretsSettings.tsx`: add **Known locations** (sentence case, monospace paths/vars, **no suffixes** on locator rows). Register / Forget. Forget copy: does not delete the file. Disabled Register if Missing or WorkspaceAuthorityRevoked. Picker is the only external remember.

Today `pickEnv` calls `requestEnvImport` (vault import inbox). Change picker to RememberLocator(external) then operator Register.

`ChatTranscript.tsx` `CredentialAccessApprovalModal`: branch on `consent`.

- RememberLocator: Cancel · Remember location (no Register button)
- RegisterSource: Cancel · Remember only · Register. If switch: “OpenRouter is loaded from {A}. Register this location instead?”
- IssueLease: Cancel · Allow once (current card)

`sessionView.ts` + `landing.ts` `approvalPayload`: pass through `consent`, `displayPath`, `variable`, `switchFromDisplay`. Consent-keyed labels.

---

## COMPAT + copy (same slice as skill)

- MCP `request_env_import` → `credential_locator_compat_import`
- MCP `list` → bindings projection, **strip suffixes**, kill next minor
- Rewrite `apps/synth_desktop/skills/use-synth-secrets/SKILL.md`
- Rewrite `apps/synth_desktop/context/WORKSHOP_AGENTS.md` (still says `request_env_import` + absolute `sourcePath`)
- Drop `$HOME` from `import_roots()`

Paid-eval proxy contract unchanged: `docs/HANDOFF_SECRETS_PROXY_OPTIMIZER_ROUTE_2026-08-18.md`.

---

## Tests the design already listed

See `CREDENTIAL_LOCATOR_REGISTRY.md` § Tests. Minimum to not lie:

- Gate: `..`, symlink-out, empty roots, missing file writes nothing
- Remember does not `put` RAM; Register does; Remember-only on Register card does not
- Preferred boot; FingerprintChanged no fallback; Forget preferred → instance
- Empty `KEY=` not loaded; occupancy switch revokes old caps
- MCP: no absolute home path in results; `request_env_import` COMPAT refuse; `request_use(locatorId)` unloaded errors
- `approval_policy=never` does not auto Remember/Register/IssueLease

---

## Suggested file-level split for the next engineer

Do **not** try to land MCP, approvals, Settings, and boot in one PR if you can avoid it. Natural PR cuts:

1. Path gate + locator SQLite + states + export rewrite + boot preferred load (host only). Settings can wait one PR if operator Register is the first consumer.
2. Operator Settings Known locations.
3. Consent + MCP + skill/AGENTS COMPAT.

If you must ship one PR, still land (1) with tests before wiring `authorize_host`.
