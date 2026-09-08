//! Actual captured environment archives -> packaged CLI -> durable Workshop results.
use serde_json::{json,Value};
use synth_desktop_lib::{data::DataStore,storage::{ContentStore,Storage},trace_ingest::TraceBundleIngestRequest,trace_research};
use std::{env,fs,path::PathBuf,time::Instant};
use futures_util::{stream,StreamExt};
#[tokio::test]
#[ignore="requires scripts/trace-research-live-engines.py and staged trace runtime"]
async fn real_archives_survive_query_source_annotation_selection_and_restart() {
    let receipts:Vec<Value>=serde_json::from_slice(&fs::read(env::var("SYNTH_RESEARCH_ENGINE_RECEIPTS").expect("receipt path")).unwrap()).unwrap();
    let root=PathBuf::from(env::var("SYNTH_RESEARCH_TEST_ROOT").expect("isolated retained root"));
    fs::create_dir_all(&root).unwrap();
    let storage=Storage::open(&root).unwrap();
    let data=DataStore::new(storage.database().clone(),ContentStore::new(storage.content_root()));
    let started=Instant::now();let mut jobs=vec![];let mut count=0;let mut resumed=0;let mut statuses=std::collections::BTreeMap::new();
    for receipt in receipts {
        let environment=receipt["environment"].as_str().unwrap();
        let container=format!("e2e-{environment}");
        storage.database().with_conn(|c|{c.execute("INSERT OR IGNORE INTO containers(id,name,location,status,created_at,updated_at) VALUES(?1,?1,'local','stopped','now','now')",[&container])?;Ok(())}).unwrap();
        let mut imported:std::collections::BTreeMap<_,_>=data.list_traces().await.unwrap().into_iter().map(|trace|(trace.digest.clone(),trace)).collect();
        let pending:Vec<_>=receipt["archives"].as_array().unwrap().iter().enumerate().filter_map(|(index,archive)|{
            let digest=receipt["rollouts"][index]["result"]["trace"]["bundle_trace_digest"].as_str().unwrap();
            if imported.contains_key(digest) {resumed+=1;None} else {Some(archive.as_str().unwrap().to_owned())}
        }).collect();
        let results=stream::iter(pending).map(|archive|{
            let data=&data;let container=container.clone();
            async move {data.ingest_trace_bundle(TraceBundleIngestRequest{source_path:archive,source_kind:Some("retained_engine_or_load_fixture_e2e".into()),title:Some(environment.into()),source_uri:None,container_id:Some(container)}).await.expect("import sealed archive")}
        }).buffer_unordered(4).collect::<Vec<_>>().await;
        for (result,_) in results {assert!(result.trusted);for trace in result.traces {imported.insert(trace.digest.clone(),trace);}}
        for rollout in receipt["rollouts"].as_array().unwrap() {
            let seed=rollout["seed"].as_i64().unwrap();let job=format!("e2e-{environment}");if !jobs.contains(&job) {jobs.push(job.clone());}
            let trial=rollout["rolloutId"].as_str().unwrap();
            let result=&rollout["result"];
            statuses.insert(trial.to_owned(),result["status"].clone());
            let digest=result["trace"]["bundle_trace_digest"].as_str().expect("sealed digest");
            let trace=imported.get(digest).expect("imported exact authority");
            let context=rollout.get("researchContext").cloned().unwrap_or_else(||json!({"environment":environment,"taskId":result["task_instance_id"],"seed":seed,"model":"code-policy","effort":null,"harnessRevision":"isolated_policy_process","promptRevision":"none","protocolRevision":"none"}));
            let details=json!({"traceRef":{"kind":"trace_v5","id":trace.id,"digest":digest},"researchContext":context});
            storage.database().with_conn(|c|{
                c.execute("INSERT OR IGNORE INTO optimizer_runs(id,algorithm_id,status,source,created_at,payload_json,updated_at) VALUES(?1,'eval','completed','live-e2e','now','{}','now')",[&job])?;
                c.execute("INSERT OR REPLACE INTO optimizer_run_collection_rows(optimizer_run_id,collection,item_id,ordinal,sequence,revision,kind,details_version,details_json,score,status,updated_at) VALUES(?1,'evaluations',?4,?5,1,1,'eval_trial','v2',?2,?3,?6,'now')",rusqlite::params![job,details.to_string(),result["reward"].as_f64().or_else(||result["reward"]["reward"].as_f64()),trial,seed,result["status"].as_str().expect("recorded rollout status")])?;Ok(())
            }).unwrap();count+=1;
        }
    }
    let import_ms=started.elapsed().as_millis();let query_started=Instant::now();
    let query=json!({"schemaVersion":"synth.trace-query.v2","evalJobIds":jobs,"grain":"episodes","limit":1});
    let snapshot=data.research_query(query.clone()).await.expect("query packaged CLI");
    let query_ms=query_started.elapsed().as_millis();
    assert_eq!(snapshot.result_count,count);
    for row in snapshot.facets["rows"].as_array().unwrap(){assert_eq!(row["traceAvailability"],"available");assert_eq!(row["analysisState"],"not_requested");assert_eq!(row["status"],statuses[row["trialId"].as_str().unwrap()]);}
    for ids in snapshot.result_ids.chunks(200) {
        let selection=trace_research::annotation_selection(&snapshot,ids).unwrap();
        assert_eq!(selection["startsCompute"],false);assert_eq!(selection["targets"].as_array().unwrap().len(),ids.len());
    }
    let entities=data.research_query(json!({"schemaVersion":"synth.trace-query.v2","evalJobIds":jobs,"grain":"entities","limit":1})).await.unwrap();
    assert!(entities.result_count>count);
    let source=data.research_source(entities.snapshot_id.clone(),entities.result_ids[0].clone(),None,0,512).await.unwrap();
    assert_eq!(source["resolved"],true);assert!(source["resolved_text"].as_str().is_some_and(|s|!s.is_empty()));
    let visual=data.resolve_trace_projection(snapshot.facets["rows"][0]["traceDigest"].as_str().unwrap().into(),"rollout-inspector".into()).await.unwrap();
    assert!(visual.payload["visual"]["items"].as_array().is_some_and(|a|!a.is_empty()));
    fs::write(root.join("visual.json"),serde_json::to_vec(&visual.payload).unwrap()).unwrap();
    fs::write(root.join("snapshot.json"),serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let digest=snapshot.result_digest.clone();let id=snapshot.snapshot_id.clone();drop(data);drop(storage);
    let reopened=Storage::open(&root).unwrap();
    let data=DataStore::new(reopened.database().clone(),ContentStore::new(reopened.content_root()));
    let restored=data.query_snapshot(id).await.unwrap();assert_eq!(restored.result_digest,digest);assert_eq!(restored.result_ids,snapshot.result_ids);
    assert_eq!(data.research_query(query).await.unwrap().snapshot_id,snapshot.snapshot_id);
    let page=trace_research::page(&restored,0,1).unwrap();assert_eq!(page["rows"].as_array().unwrap().len(),1);
    fs::write(root.join("acceptance.json"),serde_json::to_vec_pretty(&json!({"status":"passed","rollouts":count,"entityResults":entities.result_count,"snapshotId":snapshot.snapshot_id,"resultDigest":digest,"sourceResolved":true,"restart":true,"resumedImportedTraces":resumed,"importMs":import_ms,"episodeQueryMs":query_ms,"totalMs":started.elapsed().as_millis()})).unwrap()).unwrap();
}

#[tokio::test]
#[ignore="requires retained scale fixture and native store"]
async fn retained_long_trace_visual_projection() {
    let receipts:Vec<Value>=serde_json::from_slice(&fs::read(env::var("SYNTH_RESEARCH_ENGINE_RECEIPTS").unwrap()).unwrap()).unwrap();
    let digest=receipts[0]["rollouts"][0]["result"]["trace"]["bundle_trace_digest"].as_str().unwrap();
    let root=PathBuf::from(env::var("SYNTH_RESEARCH_TEST_ROOT").unwrap());
    let storage=Storage::open(&root).unwrap();
    let data=DataStore::new(storage.database().clone(),ContentStore::new(storage.content_root()));
    let started=Instant::now();
    let projection=data.resolve_trace_projection(digest.into(),"rollout-inspector".into()).await.unwrap();
    fs::write(root.join("long-visual.json"),serde_json::to_vec(&projection.payload).unwrap()).unwrap();
    fs::write(root.join("long-visual-measurement.json"),serde_json::to_vec_pretty(&json!({"projectionMs":started.elapsed().as_millis(),"bytes":serde_json::to_vec(&projection.payload).unwrap().len()})).unwrap()).unwrap();
}

#[tokio::test]
#[ignore="requires retained scale fixture and native store"]
async fn retained_long_trace_visual_windows() {
    let receipts:Vec<Value>=serde_json::from_slice(&fs::read(env::var("SYNTH_RESEARCH_ENGINE_RECEIPTS").unwrap()).unwrap()).unwrap();
    let digest=receipts[0]["rollouts"][0]["result"]["trace"]["bundle_trace_digest"].as_str().unwrap().to_owned();
    let root=PathBuf::from(env::var("SYNTH_RESEARCH_TEST_ROOT").unwrap());
    let storage=Storage::open(&root).unwrap();
    let data=DataStore::new(storage.database().clone(),ContentStore::new(storage.content_root()));
    let started=Instant::now();
    data.prepare_trace_windows(digest.clone()).await.unwrap();
    let index_ms=started.elapsed().as_millis();let started=Instant::now();
    let first=data.trace_view_window(digest.clone(),None,0,200).await.unwrap();
    let first_ms=started.elapsed().as_millis();let bytes=serde_json::to_vec(&first).unwrap();
    assert_eq!(first["visual"]["items"].as_array().unwrap().len(),200);
    assert!(bytes.len()<1024*1024);assert!(first.get("content_digest").is_none());
    let snapshot=first["view_window"]["snapshotDigest"].as_str().unwrap().to_owned();
    assert_eq!(first["view_window"]["total"],10002);
    fs::write(root.join("window-first.json"),&bytes).unwrap();
    drop(data);drop(storage);
    let storage=Storage::open(&root).unwrap();
    let data=DataStore::new(storage.database().clone(),ContentStore::new(storage.content_root()));
    let next_started=Instant::now();
    let last=data.trace_view_window(digest.clone(),Some(snapshot.clone()),10000,200).await.unwrap();
    let next_ms=next_started.elapsed().as_millis();
    assert_eq!(last["visual"]["items"].as_array().unwrap().len(),2);
    assert_eq!(last["view_window"]["snapshotDigest"],snapshot);
    assert_eq!(last["view_window"]["nextOffset"],Value::Null);
    let across=data.trace_view_window(digest.clone(),Some(snapshot.clone()),199,2).await.unwrap();
    assert_eq!(across["visual"]["items"].as_array().unwrap().len(),2);
    assert_eq!(data.trace_view_window(digest.clone(),None,0,200).await.unwrap()["view_window"]["snapshotDigest"],snapshot);
    assert!(first_ms<2000,"Indexed first window took {first_ms} ms");
    assert!(next_ms<2000,"Indexed next window took {next_ms} ms");
    assert!(data.trace_view_window(format!("sha256:{}","0".repeat(64)),Some(snapshot),0,200).await.is_err());
    fs::write(root.join("window-last.json"),serde_json::to_vec(&last).unwrap()).unwrap();
    fs::write(root.join("window-measurement.json"),serde_json::to_vec_pretty(&json!({"status":"passed","totalEvents":10002,"firstWindowItems":200,"firstWindowBytes":bytes.len(),"indexMs":index_ms,"firstWindowMs":first_ms,"lastWindowMs":next_ms,"restartPinned":true})).unwrap()).unwrap();
}

#[tokio::test]
#[ignore="requires retained scale archive; imports one long trace into a fresh retained store"]
async fn cold_long_trace_import_prepares_first_interaction() {
    let receipts:Vec<Value>=serde_json::from_slice(&fs::read(env::var("SYNTH_RESEARCH_ENGINE_RECEIPTS").unwrap()).unwrap()).unwrap();
    let root=PathBuf::from(env::var("SYNTH_RESEARCH_TEST_ROOT").unwrap()).join(format!("cold-import-{}",uuid::Uuid::new_v4().simple()));
    let storage=Storage::open(&root).unwrap();
    let data=DataStore::new(storage.database().clone(),ContentStore::new(storage.content_root()));
    let started=Instant::now();
    let (imported,_)=data.ingest_trace_bundle(TraceBundleIngestRequest {
        source_path:receipts[0]["archives"][0].as_str().unwrap().into(),source_kind:Some("synthetic_scale_acceptance".into()),
        title:Some("Cold 10002-event replay".into()),source_uri:None,container_id:None,
    }).await.unwrap();
    assert!(imported.trusted);
    let digest=receipts[0]["rollouts"][0]["result"]["trace"]["bundle_trace_digest"].as_str().unwrap().to_owned();
    let import_ms=started.elapsed().as_millis();
    let cached:i64=storage.database().with_conn(|c| Ok(c.query_row("SELECT count(*) FROM trace_projection_cache WHERE trace_digest=?1 AND projector_version='workshop.windows.v2'",[&digest],|r|r.get(0))?)).unwrap();
    assert_eq!(cached,1,"import must precompute long replay pages");
    drop(data);drop(storage);
    let storage=Storage::open(&root).unwrap();let data=DataStore::new(storage.database().clone(),ContentStore::new(storage.content_root()));
    let started=Instant::now();let page=data.trace_view_window(digest,None,0,200).await.unwrap();let first_ms=started.elapsed().as_millis();
    assert_eq!(page["view_window"]["total"],10002);assert_eq!(page["visual"]["items"].as_array().unwrap().len(),200);
    assert!(first_ms<2000,"cold-import first interaction {first_ms} ms");
    let receipt=json!({"status":"passed","store":root,"importMs":import_ms,"firstInteractionMs":first_ms,"firstWindowBytes":serde_json::to_vec(&page).unwrap().len(),"restart":true,"totalEvents":10002});
    fs::write(root.parent().unwrap().join("cold-import-acceptance.json"),serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
}

#[tokio::test]
#[ignore="requires isolated copy of retained 450-result store and engine receipts"]
async fn pinned_pages_survive_new_import_and_restart() {
    let root=PathBuf::from(env::var("SYNTH_RESEARCH_TEST_ROOT").unwrap());
    let baseline:Value=serde_json::from_slice(&fs::read(root.join("baseline.json")).unwrap()).unwrap();
    let receipts:Vec<Value>=serde_json::from_slice(&fs::read(env::var("SYNTH_RESEARCH_ENGINE_RECEIPTS").unwrap()).unwrap()).unwrap();
    let storage=Storage::open(&root).unwrap();
    let data=DataStore::new(storage.database().clone(),ContentStore::new(storage.content_root()));
    let pinned=data.query_snapshot(baseline["snapshotId"].as_str().unwrap().into()).await.unwrap();
    assert_eq!(pinned.result_count,450);
    let first=trace_research::page(&pinned,0,200).unwrap();
    let (imported,_)=data.ingest_trace_bundle(TraceBundleIngestRequest{source_path:receipts[0]["archives"][0].as_str().unwrap().into(),source_kind:Some("pagination_mutation_acceptance".into()),title:Some("New retained archive between pages".into()),source_uri:None,container_id:None}).await.unwrap();
    assert!(imported.trusted);
    let trace=&imported.traces[0];
    let details=json!({"traceRef":{"kind":"trace_v5","id":trace.id,"digest":trace.digest},"researchContext":{"environment":"craftax","taskId":"pagination-mutation","seed":0}});
    // Controlled membership mutation tests snapshot isolation, not native eval launch.
    storage.database().with_conn(|c|{c.execute("INSERT INTO optimizer_run_collection_rows(optimizer_run_id,collection,item_id,ordinal,sequence,revision,kind,details_version,details_json,score,status,updated_at) VALUES('e2e-synthetic-load','evaluations','mutation-import-craftax',100000,1,1,'eval_trial','v2',?1,0.0,'completed','now')",[details.to_string()])?;Ok(())}).unwrap();
    let mut ids=first["resultIds"].as_array().unwrap().clone();
    for offset in [200,400] {ids.extend(trace_research::page(&pinned,offset,200).unwrap()["resultIds"].as_array().unwrap().clone());}
    assert_eq!(ids,serde_json::to_value(&pinned.result_ids).unwrap().as_array().unwrap().clone());
    let refreshed=data.research_query(baseline["queryAst"].clone()).await.unwrap();
    assert_eq!(refreshed.result_count,451);assert_ne!(refreshed.result_digest,pinned.result_digest);
    let id=pinned.snapshot_id.clone();let digest=pinned.result_digest.clone();drop(data);drop(storage);
    let reopened=Storage::open(&root).unwrap();
    let data=DataStore::new(reopened.database().clone(),ContentStore::new(reopened.content_root()));
    let restored=data.query_snapshot(id).await.unwrap();assert_eq!(restored.result_ids,pinned.result_ids);assert_eq!(restored.result_digest,digest);
    fs::write(root.join("acceptance.json"),serde_json::to_vec_pretty(&json!({"status":"passed","pinnedResults":450,"refreshedResults":451,"pageSizes":[200,200,50],"newArchiveImportedBetweenPages":true,"restart":true,"resultDigest":digest,"membership":"controlled fixture mutation"})).unwrap()).unwrap();
}

#[test]
fn explicit_absent_seed_survives_archive_metadata_fallback() {
    let root=tempfile::tempdir().unwrap();
    let storage=Storage::open(root.path()).unwrap();
    storage.database().with_conn(|c|{
        c.execute("INSERT INTO optimizer_runs(id,algorithm_id,status,source,created_at,payload_json,updated_at) VALUES('fixed','eval','completed','test','now','{}','now')",[])?;
        c.execute("INSERT INTO traces(id,digest,title,source,metrics_json,metadata_json,created_at) VALUES('trace','sha256:fixed','fixed','import','[]','{\"seed\":780039}','now')",[])?;
        let details=json!({"seed":780039,"traceRef":{"kind":"trace_v5","id":"trace","digest":"sha256:fixed"},"researchContext":{"seed":null,"repeat":780040,"taskId":"fixed-world"}});
        c.execute("INSERT INTO optimizer_run_collection_rows(optimizer_run_id,collection,item_id,ordinal,sequence,revision,kind,details_version,details_json,score,status,updated_at) VALUES('fixed','evaluations','trial',0,1,1,'eval_trial','v2',?1,1,'completed','now')",[details.to_string()])?;
        let input=trace_research::resolve_inputs(c,&json!({"schemaVersion":"synth.trace-query.v2","evalJobIds":["fixed"]}))?;
        assert!(input["episodes"][0]["seed"].is_null());
        assert_eq!(input["episodes"][0]["repeat"],780040);
        assert_eq!(input["episodes"][0]["traceDigest"],"sha256:fixed");
        Ok(())
    }).unwrap();
}
