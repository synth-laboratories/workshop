//! Bounded, schema-aware collection of the visual caches.
//!
//! Two durable caches accumulate behind visuals: rendered posters/diagrams in
//! `visual_renditions`, and proof-of-render rows in `visual_render_receipts`.
//! Neither is product truth — the kernel projection is — so both are safe to
//! discard, and both must be, because nothing else ever removes them.
//!
//! Collection is schema-aware rather than purely age-based. A row produced by
//! a renderer version that no longer exists cannot be served and is not worth
//! ranking against a fresh one; the same goes for a row pointing at a visual
//! revision that has been deleted. Those go first, unconditionally. Only then
//! does a size bound apply, oldest-first.
//!
//! What is deliberately *not* collected: a receipt for a visual revision that
//! still exists. That row is what lets a reopened visual tell "the projection
//! moved on" from "the projection went backwards under something I already
//! showed", and dropping it to save a few hundred bytes would trade a
//! correctness signal for nothing.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

/// Renditions retained beyond the orphan and version sweeps.
pub const MAX_RETAINED_RENDITIONS: i64 = 512;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CollectionReport {
    /// Renditions produced by a renderer version that is no longer current.
    pub stale_renditions: usize,
    /// Renditions dropped to stay inside the size bound, oldest first.
    pub evicted_renditions: usize,
    /// Receipts whose visual revision no longer exists.
    pub orphaned_receipts: usize,
}

impl CollectionReport {
    pub fn total(&self) -> usize {
        self.stale_renditions + self.evicted_renditions + self.orphaned_receipts
    }
}

/// Collect both visual caches. Idempotent, and safe to run at any time.
pub fn collect(conn: &Connection, renderer_version: &str) -> Result<CollectionReport> {
    let mut report = CollectionReport::default();

    // Renditions need no orphan sweep: `visual_renditions` carries a foreign
    // key onto `visual_revisions`, so with `foreign_keys=ON` an orphan cannot
    // be created in the first place. Receipts below are a different matter —
    // they are deliberately unconstrained, because a receipt must survive to
    // describe a render even if the revision it describes is later rewritten,
    // and that is exactly what lets them accumulate.
    //
    // A rendition drawn by a renderer that is no longer installed can be
    // orphaned in the sense that matters: the bytes are real, but they no
    // longer depict what this build would draw.
    report.stale_renditions = conn
        .execute(
            "DELETE FROM visual_renditions WHERE renderer_version <> ?1",
            params![renderer_version],
        )
        .context("collect stale-renderer visual renditions")?;

    // Only now is a size bound meaningful, because everything left is servable.
    report.evicted_renditions = conn
        .execute(
            "DELETE FROM visual_renditions
             WHERE rowid IN (
                 SELECT rowid FROM visual_renditions
                 ORDER BY created_at DESC, rowid DESC
                 LIMIT -1 OFFSET ?1
             )",
            params![MAX_RETAINED_RENDITIONS],
        )
        .context("evict visual renditions beyond the retention bound")?;

    // A receipt for a deleted revision proves nothing about anything that can
    // still be opened. A receipt for a *live* revision is kept regardless of
    // age: it is the only thing that can detect evidence going backwards.
    report.orphaned_receipts = conn
        .execute(
            "DELETE FROM visual_render_receipts
             WHERE NOT EXISTS (
                 SELECT 1 FROM visual_revisions r
                 WHERE r.visual_id = visual_render_receipts.visual_id
                   AND r.revision = visual_render_receipts.visual_revision
             )",
            [],
        )
        .context("collect orphaned visual render receipts")?;

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    fn seed(conn: &Connection, visual_id: &str, revision: i64) {
        conn.execute(
            "INSERT OR IGNORE INTO visuals(
                id, current_revision, title, template_id, status, renderer_kind,
                created_at, updated_at
             ) VALUES(?1,?2,'t','chart.v1','draft','chart',
                      '2026-08-31T00:00:00Z','2026-08-31T00:00:00Z')",
            params![visual_id, revision],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO visual_revisions(
                visual_id, revision, template_id, renderer_kind, content_digest, created_at
             ) VALUES(?1,?2,'chart.v1','chart','sha256:x','2026-08-31T00:00:00Z')",
            params![visual_id, revision],
        )
        .unwrap();
    }

    fn rendition(conn: &Connection, visual_id: &str, revision: i64, renderer: &str, at: &str) {
        conn.execute(
            "INSERT OR REPLACE INTO visual_renditions(
                visual_id, revision, format, theme, size_class, content_digest,
                media_type, renderer_version, width_px, height_px, created_at
             ) VALUES (?1,?2,'svg','light','md','sha256:d','image/svg+xml',?3,10,10,?4)",
            params![visual_id, revision, renderer, at],
        )
        .unwrap();
    }

    #[test]
    fn collection_drops_what_cannot_be_served_and_keeps_what_proves_a_render() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path()).unwrap();
        storage
            .database()
            .with_conn(|conn| {
                seed(conn, "vis-live", 1);
                rendition(conn, "vis-live", 1, "current", "2026-08-31T00:00:00Z");
                // Live revision, but drawn by a renderer that no longer exists.
                seed(conn, "vis-old-renderer", 1);
                rendition(conn, "vis-old-renderer", 1, "ancient", "2026-08-31T00:00:00Z");

                // Receipts carry no foreign key on purpose, so this row is
                // creatable — and is precisely the kind that accumulates.
                for (visual, revision) in [("vis-live", 1_i64), ("vis-gone", 9)] {
                    conn.execute(
                        "INSERT OR REPLACE INTO visual_render_receipts(
                            visual_id, visual_revision, optimizer_run_id, template_id,
                            template_version, projection_revision, data_digest, tail_cursor, rendered_at
                         ) VALUES (?1,?2,'run-a','optimizer.run.v1','tpl',7,'fnv1a64:aaaa',12,'2026-08-31T00:00:00Z')",
                        params![visual, revision],
                    )
                    .unwrap();
                }

                let report = collect(conn, "current").unwrap();
                assert_eq!(report.stale_renditions, 1, "a rendition from a retired renderer no longer depicts this build");
                assert_eq!(report.orphaned_receipts, 1, "a receipt for a revision that does not exist proves nothing openable");

                let renditions: i64 = conn
                    .query_row("SELECT count(*) FROM visual_renditions", [], |row| row.get(0))
                    .unwrap();
                assert_eq!(renditions, 1, "the servable rendition survives");

                // The live receipt is kept regardless of age: it is the only
                // thing that can detect evidence going backwards under a
                // visual that already rendered.
                let kept: i64 = conn
                    .query_row(
                        "SELECT count(*) FROM visual_render_receipts WHERE visual_id='vis-live'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(kept, 1);

                // Idempotent: a second pass finds nothing left to do.
                assert_eq!(collect(conn, "current").unwrap().total(), 0);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn the_size_bound_keeps_the_newest_and_is_applied_after_the_schema_sweeps() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path()).unwrap();
        storage
            .database()
            .with_conn(|conn| {
                let over = MAX_RETAINED_RENDITIONS + 20;
                for index in 0..over {
                    let visual = format!("vis-{index}");
                    seed(conn, &visual, 1);
                    rendition(
                        conn,
                        &visual,
                        1,
                        "current",
                        &format!("2026-08-31T00:00:{:02}Z", index % 60),
                    );
                }
                let report = collect(conn, "current").unwrap();
                assert_eq!(report.evicted_renditions, 20);
                let remaining: i64 = conn
                    .query_row("SELECT count(*) FROM visual_renditions", [], |row| row.get(0))
                    .unwrap();
                assert_eq!(remaining, MAX_RETAINED_RENDITIONS);
                Ok(())
            })
            .unwrap();
    }
}
