//! Opt-in live RuneBench comparison with project env credentials and Workshop proxy caps.
use std::{fs,path::PathBuf,sync::Arc,io::{Read,Write,Seek,SeekFrom}};
use crate::{storage::Storage,secrets::{SecretsService,backend::MemoryBackend,capability::ProviderUsePolicy}};
#[tokio::test(flavor="multi_thread")]
#[ignore="requires RuneBench facade and explicit bounded provider experiment authorization"]
async fn live_runebench_uses_workshop_env_proxy() {
    let repo=PathBuf::from(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).unwrap().to_path_buf();
    let root=repo.join("artifacts/trace-research-e2e/runebench-proxy");fs::create_dir_all(&root).unwrap();
    let storage=Storage::open(&root).unwrap();
    let secrets=SecretsService::with_backend(storage.database().clone(),Arc::new(MemoryBackend::new()));
    let source=secrets.load_one_env_source("openrouter","OPENROUTER_API_KEY",&repo.parent().unwrap().join("evals/.env")).unwrap();assert!(source.loaded);
    secrets.start_proxy().unwrap();
    // Reserve against the $3.50 RuneBench allocation within the announced $20 total. Each
    // attempt is at most $0.50, including both arms and uncertain requests.
    let mut ledger=fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(root.join("reconstruction-budget.json")).unwrap();
    fs2::FileExt::lock_exclusive(&ledger).unwrap();
    let mut raw=String::new();ledger.read_to_string(&mut raw).unwrap();
    let used:u64=if raw.is_empty(){0}else{serde_json::from_str(&raw).unwrap()};
    assert!(used+500000<=3500000,"RuneBench aggregate allocation exhausted");
    ledger.seek(SeekFrom::Start(0)).unwrap();ledger.set_len(0).unwrap();ledger.write_all(serde_json::to_string(&(used+500000)).unwrap().as_bytes()).unwrap();ledger.sync_all().unwrap();drop(ledger);
    let attempt=uuid::Uuid::new_v4().simple().to_string();
    let mut leases=vec![];let mut run_ids=vec![];
    for arm in 0..2 {
        let policy=ProviderUsePolicy{operations:vec!["chat.completions.create".into(),"responses.create".into()],models:vec!["openai/gpt-5.6-luna".into()],reasoning_efforts:vec!["low".into()],max_calls:48,max_input_tokens:1000000,max_output_tokens:16000,max_cost_usd:0.25,lifetime_seconds:1800};
        let run_id=format!("research-runebench-{attempt}-{arm}");
        leases.push(secrets.issue_lease("openrouter",&run_id,"research.runebench.live",policy,"authorized-e2e").unwrap());run_ids.push(run_id);
    }
    let path=root.join("leases.json");fs::write(&path,serde_json::to_vec(&leases).unwrap()).unwrap();
    let status=tokio::process::Command::new("python3").arg(repo.join("scripts/trace-research-live-runebench.py")).arg(&path).current_dir(&repo).status().await.unwrap();
    for run_id in &run_ids {secrets.revoke_run(run_id).unwrap();}
    fs::write(root.join(format!("{attempt}.json")),serde_json::to_vec_pretty(&serde_json::json!({"runIds":run_ids,"maximumCostUsd":0.5,"capabilitiesRevoked":true,"processSuccess":status.success(),"actualCostUsd":null})).unwrap()).unwrap();
    fs::remove_file(path).unwrap();assert!(status.success(),"RuneBench live comparison failed; inspect retained receipts");
}
