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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_descriptors_do_not_confer_source_access(){
        assert!(!binds_run(&json!({"inputs":[{"kind":"fixture","data":{"kind":"optimizer_run","source":"run"}}]}),"run"));
    }
    #[tokio::test]
    async fn collection_cut_is_bound_immutable_and_details_are_lazy(){
        let root=tempfile::tempdir().unwrap();let db=Arc::new(Database::open(root.path().join("source.sqlite")).unwrap());
        db.with_conn(|conn|{
            conn.execute("INSERT INTO visuals(id,current_revision,title,template_id,status,renderer_kind,bindings_json,metadata_json,created_at,updated_at) VALUES('v',1,'Recorded','analysis.swarm_trajectories.v1','draft','template','{}','{}','now','now')",[])?;
            conn.execute("INSERT INTO visual_revisions(visual_id,revision,template_id,renderer_kind,bindings_json,created_at) VALUES('v',1,'analysis.swarm_trajectories.v1','template',?1,'now')",[json!({"inputs":[{"kind":"optimizer_run","source":"run"}]}).to_string()])?;
            conn.execute("INSERT INTO optimizer_runs(id,algorithm_id,status,source,created_at,payload_json,updated_at,projection_revision,aggregate_sequence) VALUES('run','gepa','completed','fixture','now','{}','now',1,2)",[])?;
            for (id,status) in [("a","completed"),("b","failed")]{
                conn.execute("INSERT INTO optimizer_run_collection_rows VALUES('run','rollouts',?1,?2,?2,1,'rollout',NULL,NULL,NULL,NULL,?3,'v1',?4,'now')",params![id,if id=="a"{0}else{1},status,json!({"observed":"original","events":[{"kind":"tool.result","sequence":1}]}).to_string()])?;
            }
            Ok(())
        }).unwrap();
        let request=json!({"revision":1,"runId":"run","collection":"rollouts"});
        let source=materialize(db.clone(),"v".into(),request.clone()).await.unwrap()["corpus"].clone();
        assert_eq!(source["count"],2);
        let query=json!({"operation":"corpus.query","revision":1,"corpus":source,"query":{"schemaVersion":"synth.visuals-core.v1","where":{"op":"all"}},"window":{"offset":0,"limit":1}});
        let page=super::super::query_engine::request(db.clone(),"v".into(),query).await.unwrap();
        assert_eq!(page["total"],2);assert_eq!(page["rows"].as_array().unwrap().len(),1);
        assert!(page["rows"][0].get("events").is_none());assert!(page["rows"][0].get("details").is_none());
        db.with_conn(|conn|{conn.execute("UPDATE optimizer_run_collection_rows SET details_json='{}',status='changed',revision=2 WHERE optimizer_run_id='run'",[])?;conn.execute("UPDATE optimizer_runs SET projection_revision=2,aggregate_sequence=3 WHERE id='run'",[])?;Ok(())}).unwrap();
        let updated=materialize(db.clone(),"v".into(),request).await.unwrap();assert_ne!(updated["corpus"]["revision"],source["revision"]);
        let detail=super::super::query_engine::request(db.clone(),"v".into(),json!({"operation":"corpus.detail","revision":1,"corpus":source,"rowId":"a"})).await.unwrap();
        assert_eq!(detail["details"]["observed"],"original");
        assert!(materialize(db.clone(),"v".into(),json!({"revision":1,"runId":"other","collection":"rollouts"})).await.is_err());
        assert!(materialize(db.clone(),"v".into(),json!({"revision":1,"runId":"run","collection":"invented"})).await.is_err());
        db.with_conn(|conn|{conn.execute("UPDATE optimizer_runs SET aggregate_sequence=-1 WHERE id='run'",[])?;Ok(())}).unwrap();
        let error=materialize(db,"v".into(),json!({"revision":1,"runId":"run","collection":"rollouts"})).await.unwrap_err();
        assert!(error.to_string().contains("behind its durable journal"));
    }
}
