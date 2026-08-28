# Trace V5 storage contract

**Status:** Normative contract; native bundle ingest, catalog storage, and
rollout-inspector projection are implemented in `synth-containers` and
`apps/synth_desktop`  
**Compatibility posture:** Preserve old bytes and identities; migrate by adding
wrappers, projections, aliases, and receipts rather than rewriting source traces.

## 1. Authorities and identities

Three identities must remain distinct:

| Identity | Syntax | Meaning |
| --- | --- | --- |
| Sealed trace digest | `sha256:<64 hex>` | Semantic digest of canonical `synth.trace.v5`, excluding its own `content_digest` |
| Bundle digest | `sha256:<64 hex>` | Semantic digest of one immutable `synth.trace-bundle.v1` generation manifest |
| Archive digest | `sha256:<64 hex>` | Byte digest of the deterministic ZIP stored by Desktop |

The sealed trace digest is the stable subject for annotations, visuals, rewards,
and deep links. A trace may appear in more than one bundle generation. A bundle
may contain more than one trace. An archive is a physical transport and must not
be mistaken for either semantic identity.

`synth-containers` owns sealing, canonical JSON, bundle validation, legacy
conversion, and standard projections. Desktop owns durable local retention,
cataloging, filtering, visual state, and annotations.

## 2. Portable producer format

Desktop accepts the `LocalTraceBundle` layout already implemented by
`synth_containers.tracing.store.bundle`:

```text
<bundle>/
  manifest.json                              # mutable generation pointer
  manifests/<generation>-<digest>.json      # immutable bundle manifest
  blobs/sha256/<prefix>/<digest>             # immutable raw bytes
  traces/<trace-id>/binding.json             # capture binding
  traces/<trace-id>/segments/*.jsonl         # immutable raw capture segments
  traces/<trace-id>/manifests/*.json         # immutable capture manifests
  traces/<trace-id>/latest.json               # mutable capture pointer, if present
  traces/<trace-id>/sealed/<digest>.json      # immutable synth.trace.v5
  evidence/<digest>.json                      # immutable evidence bundle
  projections/<kind>/<digest>.json            # derived, versioned projection
  receipts/*.json                             # validation/migration/projection receipts
  catalog.sqlite3                             # optional and rebuildable; never authority
```

Normative rules:

1. `manifest.json` may be the current `synth.trace-bundle-manifest-pointer.v1`
   or an older Push 1 inline `synth.trace-bundle.v1` manifest.
2. Every immutable file named by a modern manifest has a byte digest and byte
   size in `objects`. JSON objects may also carry a semantic digest.
3. `catalog.sqlite3`, temporary files, extracted caches, thumbnails, and Desktop
   indexes are never members of sealed identity.
4. Portable archives use the existing deterministic ZIP rules: sorted paths,
   fixed timestamps, regular files only, no symlinks, no encrypted members, and
   bounded entry/expanded sizes.
5. A bundle with `self_contained: false` may be retained as an import and shown
   as `partial`, but it is not published as a trusted local bundle until every
   required object can be resolved and verified.
6. The canonical profile `synth.canonical-json.v1` is frozen for Trace V5. A
   change to null handling, numeric encoding, key ordering, whitespace, or digest
   exclusion requires a new canonical profile and a new trace schema version.

## 3. Backward compatibility

### 3.1 Reader guarantees

New `synth-containers` releases should retain readers for:

| Input | Required behavior |
| --- | --- |
| Current bundle pointer + object inventory | Verify byte and semantic digests; native V5 read |
| Push 1 inline bundle manifest without `objects` | Use the existing legacy verification path; preserve on import |
| Standalone sealed `synth.trace.v5` JSON | Verify the trace digest; wrap unchanged in a new import bundle |
| Trace V4 | Preserve original bytes; convert through a versioned adapter; emit alias and migration receipt |
| Harbor ATIF / `agent/trajectory.json` | Preserve the native file and verifier artifacts; derive V5 with `source_format` and native aliases |
| Legacy `synth.visual.rollout_steps.v1` | Preserve as opaque source; produce a lossy viewer projection or explicitly converted V5 |
| Unknown future bundle/trace schema | Retain as opaque verified bytes when possible; report `unsupported_schema`; never guess |

Unknown optional manifest members must be retained by byte-preserving import and
ignored by readers that do not understand them. New Trace V5 semantics belong in
the existing `extensions` map. A breaking semantic change uses `synth.trace.v6`;
the meaning of `synth.trace.v5` is never revised in place.

### 3.2 Migration invariant

Migration is append-only:

```text
legacy bytes + legacy byte digest
  -> immutable source artifact in a bundle
  -> adapter@version
  -> new sealed Trace V5 with a new semantic digest
  -> migration receipt linking source digest, adapter, losses, warnings, and output digest
```

The converted trace records:

- `provenance.source_format`
- `provenance.producer` and `producer_version`
- `provenance.transformation_chain`
- native identity in `aliases`
- loss and uncertainty details in `completeness` and/or `extensions`

It must not claim that the new V5 digest was the identity of the legacy input.
Repeated migration with the same adapter version and inputs must produce the
same sealed trace digest.
Changing adapter behavior requires a new adapter version and may produce a new
trace digest without invalidating the earlier result.

### 3.3 Compatibility levels

Every import reports one of these levels:

| Level | Meaning |
| --- | --- |
| `native` | Fully understood, verified, and projectable |
| `legacy_native` | Older supported bundle/trace read without rewriting |
| `migrated` | Source preserved and a derived V5 trace is available |
| `opaque` | Bytes retained, schema unsupported, no semantic projection |
| `partial` | Some declared objects are unavailable |
| `invalid` | Digest, path, schema, or safety validation failed; not published into trusted CAS |

## 4. Desktop authoritative storage

Desktop separates untrusted/raw import retention from verified bundle storage:

```text
<content-root>/store/trace_imports/<input-prefix>/<input-digest-hex>
<content-root>/store/traces/<archive-prefix>/<archive-digest-hex>
```

`trace_imports` contains the exact supplied file, or a safe deterministic
snapshot archive of a supplied directory, before semantic trust is established.
It is quarantined data and is never resolved by a `trace_v5` visual binding.

`traces` contains only a verified deterministic portable bundle archive:

```text
<content-root>/store/traces/<archive-prefix>/<archive-digest-hex>
```

This reuses the existing Rust `ContentStore` `traces` kind and adds the
`trace_imports` kind without changing existing kinds or paths. Physical
filenames remain bare hex; the catalog stores qualified `sha256:<hex>` digests.
An extracted verified bundle is a disposable cache:

```text
<cache-root>/trace-bundles/<bundle-digest-hex>/...
```

Import transaction:

1. Stage directory/archive with path, size, entry, and symlink protections.
2. Retain the raw input/snapshot by byte digest in `trace_imports` when recovery,
   hydration, or migration may be useful.
3. Let the shared `synth-containers` validator inspect the staged bundle.
4. For `native`, `legacy_native`, or `migrated`, create and verify the
   deterministic portable archive and put it in `ContentStore(kind = "traces")`.
5. Insert the trusted bundle, membership, trace summary, and asset rows in one SQLite
   transaction; append `trace.bundle.imported` in the same transaction.
6. For `opaque`, `partial`, or `invalid`, update only the quarantine import row;
   do not create trusted bundle membership or resolve visual bindings.
7. Build filters and viewer projections asynchronously from sealed content.
8. On failure, leave neither trusted catalog authority nor a trusted extracted cache.

Import is idempotent on `bundle_digest` plus `archive_digest`. The same sealed
trace found in a later bundle adds membership/evidence; it does not fork the
trace summary row.

## 5. Desktop SQLite schema

Keep the existing `traces` table and its foreign keys for compatibility. Add
tables instead of replacing legacy rows in place.

```sql
CREATE TABLE trace_imports (
  input_digest TEXT PRIMARY KEY,            -- qualified raw byte digest
  stored_path TEXT,                         -- quarantined ContentStore pointer
  source_kind TEXT NOT NULL,
  source_uri TEXT,
  compatibility_level TEXT NOT NULL,
  validation_status TEXT NOT NULL,
  detected_schema TEXT,
  detected_bundle_digest TEXT,
  byte_size INTEGER NOT NULL DEFAULT 0,
  imported_at TEXT NOT NULL,
  error_json TEXT NOT NULL DEFAULT '[]',
  metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE trace_bundles (
  bundle_digest TEXT PRIMARY KEY,           -- qualified semantic digest
  archive_digest TEXT NOT NULL UNIQUE,      -- qualified byte digest
  archive_path TEXT NOT NULL,               -- ContentStore path/pointer
  schema_version TEXT NOT NULL,
  compatibility_level TEXT NOT NULL,
  validation_status TEXT NOT NULL,
  self_contained INTEGER NOT NULL,
  source_kind TEXT NOT NULL,
  source_uri TEXT,
  manifest_generation INTEGER,
  object_count INTEGER NOT NULL DEFAULT 0,
  byte_size INTEGER NOT NULL DEFAULT 0,
  imported_at TEXT NOT NULL,
  metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE trace_bundle_members (
  bundle_digest TEXT NOT NULL REFERENCES trace_bundles(bundle_digest),
  trace_row_id TEXT NOT NULL REFERENCES traces(id),
  trace_digest TEXT NOT NULL,
  trace_id TEXT NOT NULL,
  capture_id TEXT,
  binding_digest TEXT,
  sealed_path TEXT,
  PRIMARY KEY (bundle_digest, trace_digest)
);

CREATE TABLE trace_assets (
  bundle_digest TEXT NOT NULL REFERENCES trace_bundles(bundle_digest),
  relative_path TEXT NOT NULL,
  kind TEXT NOT NULL,
  role TEXT,
  bytes_digest TEXT NOT NULL,
  semantic_digest TEXT,
  media_type TEXT NOT NULL,
  byte_size INTEGER NOT NULL,
  availability TEXT NOT NULL,
  PRIMARY KEY (bundle_digest, relative_path)
);

CREATE TABLE trace_index (
  trace_digest TEXT PRIMARY KEY,
  projector_version TEXT NOT NULL,
  trace_kind TEXT,
  producer TEXT,
  model TEXT,
  provider TEXT,
  harness TEXT,
  benchmark TEXT,
  task_id TEXT,
  seed INTEGER,
  terminal_reason TEXT,
  lifecycle_status TEXT,
  capture_status TEXT,
  reward REAL,
  cost_usd REAL,
  prompt_tokens INTEGER,
  completion_tokens INTEGER,
  span_count INTEGER NOT NULL DEFAULT 0,
  event_count INTEGER NOT NULL DEFAULT 0,
  tool_call_count INTEGER NOT NULL DEFAULT 0,
  error_count INTEGER NOT NULL DEFAULT 0,
  started_at TEXT,
  ended_at TEXT,
  duration_ms INTEGER,
  has_media INTEGER NOT NULL DEFAULT 0,
  has_evidence INTEGER NOT NULL DEFAULT 0,
  search_text TEXT NOT NULL DEFAULT ''
);

CREATE TABLE trace_tags (
  trace_digest TEXT NOT NULL,
  namespace TEXT NOT NULL,
  value TEXT NOT NULL,
  source_digest TEXT,
  PRIMARY KEY (trace_digest, namespace, value)
);

CREATE TABLE trace_projection_cache (
  trace_digest TEXT NOT NULL,
  projection_kind TEXT NOT NULL,
  projection_schema TEXT NOT NULL,
  projector_version TEXT NOT NULL,
  source_digest TEXT NOT NULL,
  payload_digest TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (trace_digest, projection_kind, projector_version)
);

CREATE TABLE trace_annotations (
  id TEXT PRIMARY KEY,
  trace_digest TEXT NOT NULL,
  selector_json TEXT NOT NULL,
  kind TEXT NOT NULL,
  body_json TEXT NOT NULL,
  author_json TEXT NOT NULL,
  supersedes_id TEXT,
  created_at TEXT NOT NULL
);
```

`trace_index`, `trace_tags`, FTS tables, extracted bundles, previews, and
`trace_projection_cache` are rebuildable. `trace_bundles`, archived bytes,
bundle membership, quarantine imports, and user-authored annotations are durable.

Legacy `traces.digest` values may be bare hex or non-V5 identities. Preserve
them. New V5 rows use the qualified sealed digest and set
`metadata_json.schemaVersion = "synth.trace.v5"`. A migration may associate a
legacy row with a new V5 row through metadata/aliases, but must not overwrite the
legacy digest.

## 6. Projection boundary

The first shared consumer projection should be versioned independently:

```text
synth.trace-projection.rollout-inspector.v1
```

It contains only derived viewer data: header facts, ordered entries, stable
selectors, text/media observations, tool calls/results, reward changes, usage,
errors, evidence markers, and provenance pointers. Each entry includes the
sealed trace digest plus a `synth.trace-selector.v1`; array positions alone are
not stable deep-link identifiers.

Desktop templates consume this projection. They must not parse raw V5 fields,
infer verification, or derive task membership independently.

## 7. Versioning rules

- Patch: validator fixes that do not change accepted semantics or output digest.
- Minor/additive: new optional bundle objects, projections, receipts, tags, or
  `extensions`; old readers must continue to retain the bytes.
- Major: canonicalization changes, required-field changes, renamed meanings, or
  altered identity rules; mint a new schema/profile version.
- Readers should support at least the current and previous major trace schema and
  all released `synth.trace-bundle.v1` generations.
- Conformance fixtures are permanent once released. CI validates every old
  fixture with the newest reader and asserts unchanged semantic/byte digests.

## 8. First implementation slices

### `synth-containers`

1. Freeze this contract in package docs and expose `inspect_bundle` as a stable
   library result rather than requiring Desktop to know bundle internals.
2. Add permanent fixtures for Push 1, current V5, standalone V5, V4 migration,
   Harbor ATIF migration, partial media, corruption, and unknown future schema.
3. Add a versioned migration receipt and rollout-inspector projection.
4. Keep the current backward-compatible verifier and deterministic archive path.

### Desktop core

1. Add the tables above through additive migrations.
2. Add the non-breaking `trace_imports` content kind; import verified
   deterministic archives through the existing `traces` kind.
3. Index `TraceInspection`/rollout-inspector output, not raw bundle internals.
4. Resolve `trace_v5` bindings by sealed digest and projection version.
5. Dogfood the Luna seed 8 Craftax projection and the existing large Harbor V5
   bundle as, respectively, UI and storage stress fixtures.
