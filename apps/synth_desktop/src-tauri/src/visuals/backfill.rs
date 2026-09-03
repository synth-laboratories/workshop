//! Bring already-persisted visual bindings onto the canonical envelope.
//!
//! Writers were allowed to persist un-canonical bindings for several releases,
//! so existing databases hold visuals the renderer cannot read — they open,
//! declare nothing, and sit at `connecting` with no error. A schema migration
//! cannot fix this: deciding whether `{"stream": [...]}` is a descriptor map or
//! inline data is JSON logic, not SQL.
//!
//! What this does and does not touch:
//!
//! - `visuals.bindings_json` — the row the renderer reads — is upgraded, and
//!   the current revision's `bindings_json`/`bindings_digest` are re-stamped so
//!   the rendered-observation digest check compares like with like.
//! - Historical revisions are left exactly as authored. They are the audit
//!   trail, and a receipt that already named an old digest must keep resolving.
//!   The superseded digest is recorded in `metadata_json.bindingsUpgrade`.
//! - A row that cannot be canonicalised is left untouched and counted. Silent
//!   repair of an unreadable binding is the failure this whole change is about.
//!
//! See: docs/contracts/visual_bindings.md.

use super::models::{canonicalize_bindings, VISUAL_BINDINGS_SCHEMA_VERSION};
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

/// What one backfill pass changed. Counts only — the caller logs them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BindingsBackfill {
    pub scanned: usize,
    pub upgraded: usize,
    pub refused: usize,
}

impl BindingsBackfill {
    pub fn changed(&self) -> bool {
        self.upgraded > 0 || self.refused > 0
    }
}

/// Canonical rows are skipped by a contains match rather than parsed, so a
/// database that has already been backfilled costs one indexed scan at launch.
/// `serde_json` writes object keys in sorted order. After the bind write drop
/// the envelope is `{ inputs, schemaVersion }`, so `schemaVersion` is no longer
/// a prefix — match the version token anywhere in the JSON.
const CANONICAL_PREFIX: &str = r#"%"schemaVersion":"synth.visual-bindings.v1"%"#;

pub fn canonicalize_persisted_bindings(conn: &Connection) -> Result<BindingsBackfill> {
    let mut report = BindingsBackfill::default();
    let candidates: Vec<(String, i64, String, String)> = {
        let mut statement = conn
            .prepare(
                "SELECT id, current_revision, bindings_json, metadata_json
                 FROM visuals
                 WHERE bindings_json NOT LIKE ?1",
            )
            .context("prepare visual bindings backfill scan")?;
        let rows = statement
            .query_map(params![CANONICAL_PREFIX], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .context("scan visual bindings")?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read visual bindings")?
    };

    for (id, revision, bindings_json, metadata_json) in candidates {
        report.scanned += 1;
        let authored: Value = match serde_json::from_str(&bindings_json) {
            Ok(value) => value,
            Err(error) => {
                report.refused += 1;
                crate::platform::logging::report(
                    "visuals",
                    "eprintln",
                    format!("synth-desktop: visual {id} has unparseable bindings: {error}"),
                );
                continue;
            }
        };
        let canonical = match canonicalize_bindings(&authored) {
            Ok(canonical) => canonical,
            Err(error) => {
                report.refused += 1;
                crate::platform::logging::report(
                    "visuals",
                    "eprintln",
                    format!(
                        "synth-desktop: visual {id} bindings cannot be upgraded to \
                     {VISUAL_BINDINGS_SCHEMA_VERSION}: {error:#}"
                    ),
                );
                continue;
            }
        };
        if !canonical.form.is_upgrade() {
            continue;
        }

        let upgraded_json =
            serde_json::to_string(&canonical.value).context("serialize canonical bindings")?;
        let previous_digest: Option<String> = conn
            .query_row(
                "SELECT bindings_digest FROM visual_revisions
                 WHERE visual_id = ?1 AND revision = ?2",
                params![&id, revision],
                |row| row.get(0),
            )
            .unwrap_or(None);
        let mut metadata: Value =
            serde_json::from_str(&metadata_json).unwrap_or_else(|_| json!({}));
        if !metadata.is_object() {
            metadata = json!({});
        }
        metadata["bindingsUpgrade"] = json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "form": canonical.form.as_str(),
            "inputs": canonical.upgraded_slots,
            "revision": revision,
            "previousBindingsDigest": previous_digest,
            "upgradedAt": Utc::now().to_rfc3339(),
        });

        conn.execute(
            "UPDATE visuals SET bindings_json = ?2, metadata_json = ?3 WHERE id = ?1",
            params![
                &id,
                &upgraded_json,
                &serde_json::to_string(&metadata).context("serialize visual metadata")?
            ],
        )
        .context("write canonical visual bindings")?;
        conn.execute(
            "UPDATE visual_revisions SET bindings_json = ?3, bindings_digest = ?4
             WHERE visual_id = ?1 AND revision = ?2",
            params![
                &id,
                revision,
                &upgraded_json,
                super::registry::digest_json(&canonical.value)
            ],
        )
        .context("re-stamp canonical revision digest")?;
        report.upgraded += 1;
        crate::platform::logging::report(
            "visuals",
            "eprintln",
            format!(
                "synth-desktop: upgraded visual {id} rev {revision} bindings ({})",
                canonical.form.as_str()
            ),
        );
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE visuals (
                id TEXT PRIMARY KEY,
                current_revision INTEGER NOT NULL,
                bindings_json TEXT NOT NULL,
                metadata_json TEXT NOT NULL DEFAULT '{}'
             );
             CREATE TABLE visual_revisions (
                visual_id TEXT NOT NULL,
                revision INTEGER NOT NULL,
                bindings_json TEXT,
                bindings_digest TEXT,
                PRIMARY KEY (visual_id, revision)
             );",
        )
        .unwrap();
        conn
    }

    fn insert(conn: &Connection, id: &str, revision: i64, bindings: &Value) {
        conn.execute(
            "INSERT INTO visuals (id, current_revision, bindings_json, metadata_json)
             VALUES (?1, ?2, ?3, '{}')",
            params![id, revision, bindings.to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO visual_revisions (visual_id, revision, bindings_json, bindings_digest)
             VALUES (?1, ?2, ?3, 'digest-before')",
            params![id, revision, bindings.to_string()],
        )
        .unwrap();
    }

    fn bindings_of(conn: &Connection, id: &str) -> Value {
        let raw: String = conn
            .query_row(
                "SELECT bindings_json FROM visuals WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    /// The v0.4 acceptance shape: ten live streams the renderer could not see.
    #[test]
    fn upgrades_the_slot_keyed_map_and_records_the_superseded_digest() {
        let conn = database();
        insert(
            &conn,
            "vis_incident",
            3,
            &json!({"stream": [{
                "slot": "stream",
                "kind": "live_sse",
                "source": "http://127.0.0.1:8114/rollouts/r1/stream",
                "poll_url": "http://127.0.0.1:8114/rollouts/r1/events"
            }]}),
        );

        let report = canonicalize_persisted_bindings(&conn).unwrap();

        assert_eq!(report.upgraded, 1);
        assert_eq!(report.refused, 0);
        let upgraded = bindings_of(&conn, "vis_incident");
        assert_eq!(
            upgraded["schemaVersion"],
            json!(VISUAL_BINDINGS_SCHEMA_VERSION)
        );
        assert_eq!(upgraded["inputs"].as_array().unwrap().len(), 1);
        assert!(upgraded.get("slots").is_none());
        let metadata: String = conn
            .query_row(
                "SELECT metadata_json FROM visuals WHERE id = 'vis_incident'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let metadata: Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(
            metadata["bindingsUpgrade"]["previousBindingsDigest"],
            json!("digest-before")
        );
        let digest: String = conn
            .query_row(
                "SELECT bindings_digest FROM visual_revisions WHERE visual_id = 'vis_incident'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_ne!(digest, "digest-before");
    }

    #[test]
    fn is_idempotent_and_skips_canonical_rows() {
        let conn = database();
        insert(
            &conn,
            "vis_incident",
            1,
            &json!({"stream": [{
                "slot": "stream",
                "kind": "live_sse",
                "source": "http://127.0.0.1:8114/rollouts/r1/stream"
            }]}),
        );

        let first = canonicalize_persisted_bindings(&conn).unwrap();
        let second = canonicalize_persisted_bindings(&conn).unwrap();

        assert_eq!(first.upgraded, 1);
        assert_eq!(second, BindingsBackfill::default());
    }

    #[test]
    fn leaves_unreadable_bindings_alone_and_counts_them() {
        let conn = database();
        insert(
            &conn,
            "vis_mixed",
            1,
            &json!({
                "stream": {"kind": "live_sse", "source": "http://127.0.0.1:8114/rollouts/r1/stream"},
                "notes": [1, 2, 3]
            }),
        );

        let report = canonicalize_persisted_bindings(&conn).unwrap();

        assert_eq!(report.refused, 1);
        assert_eq!(report.upgraded, 0);
        assert!(bindings_of(&conn, "vis_mixed")
            .get("schemaVersion")
            .is_none());
    }
}
