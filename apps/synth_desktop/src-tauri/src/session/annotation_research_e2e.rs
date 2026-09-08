//! Retained live Jesterky receipts projected through the production query authority.
use super::annotation_projection::*;
use crate::{data::DataStore,storage::{Storage,ContentStore}};
use serde_json::{json,Value};
use std::{fs,path::PathBuf};
#[tokio::test]
#[ignore="requires real engine and live Jesterky receipts plus native-store fixture"]
async fn live_annotations_query_with_rewards_and_preserve_old_snapshot() {
 let repo=PathBuf::from(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).unwrap().to_path_buf();
 let root=repo.join("artifacts/trace-research-e2e");
 let store_root=std::env::var_os("SYNTH_RESEARCH_TEST_ROOT").map(PathBuf::from).unwrap_or_else(||root.join("native-store"));
 let storage=Storage::open(&store_root).unwrap();
 let data=DataStore::new(storage.database().clone(),ContentStore::new(storage.content_root()));
 let ordinary=std::env::var_os("SYNTH_RESEARCH_ORDINARY_ANNOTATIONS").is_some();
 let annotation_root=if ordinary {"ordinary-annotations"} else {"live-jesterky"};
 let label=if ordinary {"trace.recorded"} else {"trace.observed"};
 let expected_findings=if ordinary {2} else {6};
 let old:Value=serde_json::from_slice(&fs::read(store_root.join("snapshot.json")).unwrap()).unwrap();
 let before=data.query_snapshot(old["snapshotId"].as_str().unwrap().to_owned()).await.unwrap();
 for env in ["craftax","dungeongrid"] {
  let receipt:Value=serde_json::from_slice(&fs::read(root.join(format!("{annotation_root}/{env}/receipt.json"))).unwrap()).unwrap();
  assert_eq!(receipt["job"]["state"],"sealed");
  let evidence=&receipt["evidence"];let digest=evidence["trace_ref"]["content_digest"].as_str().unwrap();
  let trace_id=evidence["trace_ref"]["trace_id"].as_str().unwrap();let campaign=format!("e2e-{annotation_root}-{env}");let container=format!("e2e-{env}");
  storage.database().with_conn(|c| {
   ensure_import_campaign(c,&campaign,&container,trace_id,None)?;
   apply_job_snapshot(c,Some(&campaign),&container,&receipt["job"])?;
   let annotations=json!({"annotations":evidence["annotations"],"bundle_digest":evidence["content_digest"]});
   let head=json!({"bundles":[{"is_head":true,"bundle_digest":evidence["content_digest"],"bundle_id":evidence["bundle_id"],"annotation_count":evidence["annotations"].as_array().unwrap().len()}]});
   assert!(project_trace_head(c,&campaign,trace_id,digest,&annotations,&head)?.is_some());Ok(())
  }).unwrap();
 }
 let jobs=json!(["e2e-craftax","e2e-dungeongrid"]);
 let findings=data.research_query(json!({"schemaVersion":"synth.trace-query.v2","evalJobIds":jobs,"grain":"annotations","where":[{"field":"label","op":"contains","value":label}]})).await.unwrap();
 assert_eq!(findings.result_count,expected_findings);
 let episodes=data.research_query(json!({"schemaVersion":"synth.trace-query.v2","evalJobIds":jobs,"grain":"episodes","annotationWhere":[{"field":"label","op":"contains","value":label}]})).await.unwrap();
 assert_eq!(episodes.result_count,2);
 let source=data.research_source(findings.snapshot_id.clone(),findings.result_ids[0].clone(),None,0,512).await.unwrap();assert_eq!(source["resolved"],true);
 let after=data.query_snapshot(before.snapshot_id.clone()).await.unwrap();assert_eq!(before.result_digest,after.result_digest);assert_eq!(before.result_ids,after.result_ids);
 fs::write(root.join(if ordinary {"ordinary-annotation-query-acceptance.json"} else {"annotation-query-acceptance.json"}),serde_json::to_vec_pretty(&json!({"status":"passed","findings":expected_findings,"matchedEpisodes":2,"jesterkyEnabled":!ordinary,"oldSnapshotUnchanged":true,"sourceResolved":true})).unwrap()).unwrap();
}

#[tokio::test]
#[ignore="requires the retained native app evidence store; no provider calls"]
async fn retained_reviews_refresh_queries_without_rewriting_saved_answers() {
 let repo=PathBuf::from(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).unwrap().to_path_buf();
 let root=repo.join("artifacts/trace-research-e2e/native-app-store");
 let storage=Storage::open(&root).unwrap();
 let data=DataStore::new(storage.database().clone(),ContentStore::new(storage.content_root()));
 let (finding,head):(String,String)=storage.database().with_conn(|c| {
  Ok(c.query_row("SELECT finding_id,evidence_head_digest FROM annotation_findings ORDER BY finding_id LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?)))?)
 }).unwrap();
 let query=json!({"schemaVersion":"synth.trace-query.v2","evalJobIds":["e2e-craftax","e2e-dungeongrid"],"grain":"annotations","where":[{"field":"annotationId","op":"eq","value":finding}]});
 let original=data.research_query(query.clone()).await.unwrap();assert_eq!(original.result_count,1);
 let old_source=data.research_source(original.snapshot_id.clone(),original.result_ids[0].clone(),None,0,512).await.unwrap();
 let original_traces=data.list_traces().await.unwrap().into_iter().map(|t|(t.id,t.digest,t.reward)).collect::<Vec<_>>();
 let mut revisions=vec![];
 for decision in ["accept","reject"] {
  storage.database().with_conn(|c|record_local_review(c,&finding,&head,decision,"acceptance-test","Retained evidence review regression")).unwrap();
  let mut filtered=query.clone();filtered["where"].as_array_mut().unwrap().push(json!({"field":"reviewState","op":"eq","value":decision}));
  let refreshed=data.research_query(filtered).await.unwrap();assert_eq!(refreshed.result_count,1);
  let reopened=data.query_snapshot(original.snapshot_id.clone()).await.unwrap();assert_eq!(reopened.result_digest,original.result_digest);assert_eq!(reopened.result_ids,original.result_ids);
  assert_eq!(data.research_source(original.snapshot_id.clone(),original.result_ids[0].clone(),None,0,512).await.unwrap(),old_source);
  revisions.push(refreshed.result_digest);
 }
 assert_ne!(revisions[0],revisions[1]);
 drop(data);drop(storage);
 let storage=Storage::open(&root).unwrap();let data=DataStore::new(storage.database().clone(),ContentStore::new(storage.content_root()));
 assert_eq!(data.query_snapshot(original.snapshot_id).await.unwrap().result_digest,original.result_digest);
 assert_eq!(data.list_traces().await.unwrap().into_iter().map(|t|(t.id,t.digest,t.reward)).collect::<Vec<_>>(),original_traces);
 fs::write(root.join("review-query-acceptance.json"),serde_json::to_vec_pretty(&json!({"status":"passed","decisions":["accept","reject"],"reviewer":"acceptance-test","oldSnapshotAndSourceUnchanged":true,"traceDigestsAndRewardsUnchanged":true,"restartPinned":true,"providerCalls":0,"ordinaryCampaignLaunchVerified":false})).unwrap()).unwrap();
}
