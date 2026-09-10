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

