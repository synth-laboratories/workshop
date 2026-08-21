//! Hosted Go-Ex state-batch ingest. Workshop does not ship a task GELO recipe.

use super::events::OptimizerEventDraft;
use super::OptimizerService;
use anyhow::Result;
use serde_json::{json, Map, Value};

pub(super) async fn append_state_batch(
    service: &OptimizerService,
    run_id: &str,
    remote_status: &str,
    batch: Value,
) -> Result<()> {
    let mut snapshot = Map::new();
    snapshot.insert(
        "slices".into(),
        batch.get("slices").cloned().unwrap_or_else(|| json!({})),
    );
    snapshot.insert("status".into(), json!(remote_status));
    service
        .append_event_payloads(
            run_id.to_string(),
            vec![
                OptimizerEventDraft::new("goex.state.batch.updated", "go-ex")
                    .level("info")
                    .snapshot(snapshot)
                    .raw(json!({"source": "optimizers-beta-state-batch"})),
            ],
        )
        .await?;
    Ok(())
}
