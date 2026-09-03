use anyhow::Result;
use rusqlite::{params, Connection};

use crate::platform::failure::{FailureKind, OperationalFailure, VisualFailure};
use crate::platform::operations::{OperationContext, OperationKind, OperationPhase};

pub fn raise_render_failed(
    conn: &Connection,
    visual_id: &str,
    detail: &str,
) -> Result<OperationalFailure> {
    let mut context = OperationContext::bootstrap(crate::instance::boot_epoch());
    context.visual_id = Some(visual_id.to_owned());
    let raised = crate::platform::failure::FailureRuntime::raise_in_tx(
        conn,
        FailureKind::Visual(VisualFailure::RenderFailed {
            visual_id: visual_id.to_owned(),
            detail: detail.to_owned(),
        }),
        context,
        OperationKind::VisualRender,
        OperationPhase::Execute,
        None,
        "visual_authority",
    )?;
    conn.execute(
        "UPDATE visuals SET current_failure_id = ?1 WHERE id = ?2",
        params![raised.failure_id.as_str(), visual_id],
    )?;
    Ok(raised)
}
