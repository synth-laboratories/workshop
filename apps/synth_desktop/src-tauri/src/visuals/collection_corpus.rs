//! Workshop adapter: a transactionally pinned derived index of a bound run's
//! existing collection read model. Raw details never cross IPC until selected.
use crate::storage::Database;
use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::sync::Arc;

fn binds_run(value:&Value,run:&str)->bool {
    // Only canonical bind descriptors confer access. A similarly shaped object
    // embedded in fixture data or metadata is not a source binding.
    value.get("inputs").and_then(Value::as_array).is_some_and(|inputs|inputs.iter().any(|binding|
        binding.get("kind").and_then(Value::as_str)==Some("optimizer_run")
        && binding.get("source").and_then(Value::as_str)==Some(run)))
}

pub async fn materialize(db:Arc<Database>,visual_id:String,request:Value)->Result<Value>{
    let revision=request["revision"].as_i64().filter(|v|*v>0).context("positive visual revision required")?;
    let run=request["runId"].as_str().filter(|v|!v.is_empty()&&v.len()<=200).context("runId required")?.to_owned();
    let collection=request["collection"].as_str().context("collection required")?.to_owned();
    if !["candidates","rollouts","evaluations","metric_points","proposer_calls"].contains(&collection.as_str()) {bail!("collection has no durable analytical adapter");}
    db.run_transaction(move|conn|{
        let bindings:String=conn.query_row("SELECT bindings_json FROM visual_revisions WHERE visual_id=?1 AND revision=?2",params![visual_id,revision],|r|r.get(0))?;
        if !binds_run(&serde_json::from_str(&bindings)?,&run){bail!("analytical source is not bound to this visual revision");}
        let (projection,sequence):(Option<i64>,Option<i64>)=conn.query_row("SELECT projection_revision,aggregate_sequence FROM optimizer_runs WHERE id=?1",[&run],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let projection=projection.context("run has no durable projection")?;
        let sequence=sequence.context("run has no durable evidence cursor")?;
        let journal_tail:i64=conn.query_row("SELECT COALESCE(MAX(sequence_number),0) FROM optimizer_events WHERE optimizer_run_id=?1",[&run],|r|r.get(0))?;
        if sequence<journal_tail {bail!("run projection is behind its durable journal; repair the run read model before pinning analytics");}
        let id=format!("optimizer:{run}:{collection}");
        let cut=format!("collection-v1:{projection}:{sequence}");
        let schema="synth.workshop.run-collection.v1";
        let previous:Option<i64>=conn.query_row("SELECT expected_count FROM visual_corpora WHERE visual_id=?1 AND visual_revision=?2 AND corpus_id=?3 AND corpus_revision=?4 AND sealed=1",params![visual_id,revision,id,cut],|r|r.get(0)).optional()?;
        if let Some(count)=previous{return Ok(json!({"corpus":{"id":id,"revision":cut,"schema":schema,"count":count}}));}
        let count:i64=conn.query_row("SELECT COUNT(*) FROM optimizer_run_collection_rows WHERE optimizer_run_id=?1 AND collection=?2",params![run,collection],|r|r.get(0))?;
        if count>1_000_000 {bail!("collection exceeds analytical index bound");}
        conn.execute("INSERT INTO visual_corpora VALUES(?1,?2,?3,?4,?5,?6,1)",params![visual_id,revision,id,cut,schema,count])?;
        conn.execute("INSERT INTO visual_corpus_rows SELECT ?1,?2,?3,?4,item_id,ordinal,json_object('id',item_id,'ordinal',ordinal,'sequence',sequence,'revision',revision,'kind',kind,'label',label,'parentId',parent_id,'score',score,'costUsd',cost_usd,'status',status,'detailsVersion',details_version) FROM optimizer_run_collection_rows WHERE optimizer_run_id=?5 AND collection=?6",params![visual_id,revision,id,cut,run,collection])?;
        conn.execute("INSERT INTO visual_corpus_details SELECT ?1,?2,?3,?4,item_id,details_json FROM optimizer_run_collection_rows WHERE optimizer_run_id=?5 AND collection=?6",params![visual_id,revision,id,cut,run,collection])?;
        Ok(json!({"corpus":{"id":id,"revision":cut,"schema":schema,"count":count}}))
    }).await
}

