# Legacy local-runtime migration integration

`storage::legacy_migration` imports the retired Python runtime database into the
Rust CoreRuntime database. It is intentionally not run implicitly yet.

At startup, after `Storage::open` has applied the current Rust schema and before
Codex or Intern providers start, the composition root should:

1. Call `default_legacy_candidates()` and `detect_legacy_database()`.
2. Skip migration when no recognized source exists or a receipt for that source
   is already stored in `runtime_settings`.
3. Surface the detected source and planned backup path for user confirmation.
4. Open a writable CoreRuntime connection and call `migrate_legacy_database`.
5. Show receipt counts and warnings; only start providers after success.

The importer never deletes or writes the source database. It creates a
consistent SQLite backup plus a JSON receipt, preserves legacy IDs, uses stable
derived event IDs, copies trace/TSX content into the Rust CAS, verifies SQLite
integrity and foreign keys before commit, and records an explicit dependency-safe
rollback deletion order. The app should retain both source and backup until the
Python-runtime removal gate has been dogfooded and signed off.

When legacy processes still hold the database, WAL, or SHM open, the scanner
warns the user to stop them when possible. Apply uses SQLite `VACUUM INTO` for a
transactionally consistent snapshot and imports every database row from that
snapshot—not from the changing live database—so the backup, receipt, and
destination cannot disagree. No checkpoint or write is issued against the
legacy source.

## Exact Tauri registration

The command module is compiled and tested but `lib.rs` registration is left to
the composition-root owner. In `setup`, after `core` is created, register:

```rust
let migration = crate::storage::legacy_migration::MigrationService::new(
    core.storage().database().clone(),
    core.storage().content_root().to_path_buf(),
    crate::storage::app_data_root().join("migration-backups"),
);
app.manage(migration);
```

Add these exact functions to `tauri::generate_handler!`:

```rust
crate::storage::legacy_migration::migration_scan,
crate::storage::legacy_migration::migration_prepare,
crate::storage::legacy_migration::migration_apply,
crate::storage::legacy_migration::migration_cancel,
```

No migration runs at startup. `migration_prepare` creates a ten-minute,
in-memory confirmation plan and exact phrase. `migration_apply` requires both,
rejects a source changed since inspection, consumes the plan once, and returns
the durable receipt. The Runtime Settings UI surfaces this explicit flow.
