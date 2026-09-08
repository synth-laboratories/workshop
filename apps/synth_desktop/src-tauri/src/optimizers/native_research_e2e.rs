// Opt-in actual engine acceptance. Only session/container setup is inserted;
// all runs, collection rows, and trace imports must be produced by start_recipe.
use super::*;
use crate::{data::{ContainerRegisterRequest, DataStore}, storage::{ContentStore, EventJournal, Storage}, visuals::VisualRegistry};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires trace-research-native-launch-servers.py real engines"]
async fn real_native_eval_launch_to_query_source_and_restart() {
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).unwrap().to_owned();
    let rune = std::env::var("SYNTH_RESEARCH_NATIVE_RUNEBENCH_URL").ok();
    let root = repo.join(if rune.is_some() {"artifacts/trace-research-e2e/native-runebench"} else {"artifacts/trace-research-e2e/native-launch"});
    std::fs::create_dir_all(&root).unwrap();
    let storage = Storage::open(root.join("native-store")).unwrap();
    let secrets = if rune.is_some() {
        use std::io::{Read,Write,Seek,SeekFrom};
        let mut ledger=std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(root.join("budget.json")).unwrap();
        fs2::FileExt::lock_exclusive(&ledger).unwrap();
        let mut raw=String::new();ledger.read_to_string(&mut raw).unwrap();
        let reserved:u64=if raw.is_empty(){0}else{serde_json::from_str(&raw).unwrap()};
        // Dedicated $1 allocation; earlier retained allocations plus this stay below $20.
        assert!(reserved+250000<=1250000,"native RuneBench allocation exhausted");
        ledger.seek(SeekFrom::Start(0)).unwrap();ledger.set_len(0).unwrap();ledger.write_all((reserved+250000).to_string().as_bytes()).unwrap();ledger.sync_all().unwrap();
        let service=Arc::new(crate::secrets::SecretsService::with_backend(storage.database().clone(),Arc::new(crate::secrets::MemoryBackend::new())));
        assert!(service.load_one_env_source("openrouter","OPENROUTER_API_KEY",&repo.parent().unwrap().join("evals/.env")).unwrap().loaded);
        service.start_proxy().unwrap();crate::secrets::install_live(service.clone());Some(service)
    } else {None};
    let journal = EventJournal::new(storage.database().clone());
    let content = ContentStore::new(storage.content_root());
    let visuals = VisualRegistry::new(storage.database().clone(), journal.clone(), content.clone());
    let (events_tx, _events_rx) = tokio::sync::broadcast::channel(1024);
    let svc = OptimizerService::new_with_manager(storage.database().clone(), journal, visuals, events_tx,
        Arc::new(crate::optimizers::OptimizerManager::with_home(root.join("optimizer-home"))));
    let data = DataStore::new(storage.database().clone(), content);
    let servers: Vec<Value> = if let Some(base)=&rune {vec![json!({"environment":"runebench","baseUrl":base})]} else {serde_json::from_slice(&std::fs::read(root.join("servers.json")).unwrap()).unwrap()};
    let workspace = root.join("workspace");
    std::fs::create_dir_all(workspace.join("workshop.recipes")).unwrap();
    let session = format!("native_research_{}", uuid::Uuid::new_v4().simple());
    storage.database().with_conn(|conn| {
        conn.execute("INSERT INTO sessions(id,title,target_json,status,metadata_json,created_at,updated_at) VALUES(?1,?1,'{}','ready',?2,datetime('now'),datetime('now'))",
            params![session, json!({"workspace":workspace}).to_string()])?; Ok(())
    }).unwrap();
    crate::workspace_scope::provision(storage.database(), &session, workspace.to_str().unwrap()).await.unwrap();
    let mut jobs = vec![];
    for server in servers {
        let family = server["environment"].as_str().unwrap();
        let base = server["baseUrl"].as_str().unwrap();
        let info: Value = reqwest::get(format!("{base}/info")).await.unwrap().error_for_status().unwrap().json().await.unwrap();
        let (container, _) = data.upsert_container(ContainerRegisterRequest {
            name: Some(format!("Actual {family} acceptance")), base_url: base.into(), location: Some("local".into()),
            task_family: Some(family.into()), metadata: None,
        }, "ready".into(), json!({"ok":true}), json!({"info":info,"capabilities":info["capabilities"],"runtime_family":family}), Some(family.into())).await.unwrap();
        let recipe_id = format!("eval.{family}.research_native.v1");
        std::fs::write(workspace.join(format!("workshop.recipes/{family}.toml")), format!(r#"
id = "{recipe_id}"
algorithm = "eval"
title = "Actual {family} research acceptance"
container = "{family}"
provider = "none"
model = "code-policy"
locality = "host"
family = "{family}"
harness = "isolated_policy_process"
policy_config = "heuristic"
concurrency = 1
train_seeds = [0, 1]
[policy]
max_steps = 12
max_calls = 24
[bounds]
max_cost_usd = 0.01
max_total_rollouts = 2
"#)).unwrap();
        if rune.is_some() {
            std::fs::write(workspace.join(format!("workshop.recipes/{family}.toml")),format!(r#"
id = "{recipe_id}"
algorithm = "eval"
title = "Actual native RuneBench research acceptance"
container = "runebench"
provider = "openrouter"
model = "openai/gpt-5.6-luna"
locality = "host"
family = "runebench"
harness = "harbor_fused"
policy_config = "luna_low"
concurrency = 1
train_seeds = [780039]
[policy]
effort = "low"
max_steps = 24
max_calls = 48
context_token_budget = 25000
answer_max_tokens = 512
timeout_seconds = 30
[bounds]
max_cost_usd = 0.25
max_total_rollouts = 1
"#)).unwrap();
        }
        let (run, _) = svc.start_recipe(OptimizerRecipeRunRequest {
            recipe_id, session_ref: Some(session.clone()), open_visual: Some(false), base_model: None,
            dataset_shard: None, candidate_set_id: None, container_id: Some(container.id),
            training_artifact_id: None, search: None, plan_override: None,
        }).await.expect("native start_recipe on real advertised code policy");
        let settled = tokio::time::timeout(Duration::from_secs(if rune.is_some(){1200}else{240}), async {
            loop {
                let record = svc.get(run.id.clone()).await.unwrap();
                if matches!(record.status.as_str(), "completed" | "failed" | "cancelled" | "degraded") { break record; }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }).await.expect("native run bounded settlement");
        std::fs::write(root.join(format!("{family}-native-run.json")), serde_json::to_vec_pretty(&settled).unwrap()).unwrap();
        if let Some(secrets)=&secrets {
            secrets.revoke_run(&run.id).unwrap();
            std::fs::write(root.join("provider-usage.json"),serde_json::to_vec_pretty(&secrets.provider_usage_receipt(&run.id).unwrap()).unwrap()).unwrap();
        }
        assert_eq!(settled.status, "completed", "inspect retained native run {family}");
        jobs.push(run.id);
    }
    let query = json!({"schemaVersion":"synth.trace-query.v2","evalJobIds":jobs,"grain":"episodes","limit":100});
    let snapshot = data.research_query(query.clone()).await.unwrap();
    std::fs::write(root.join("native-snapshot.json"), serde_json::to_vec_pretty(&snapshot).unwrap()).unwrap();
    assert_eq!(snapshot.result_count, if rune.is_some(){1}else{4});
    for row in snapshot.facets["rows"].as_array().unwrap() {
        assert_eq!(row["traceAvailability"], "available");
        assert_eq!(row["analysisState"], "not_requested");
        assert!(row["reward"].is_number(), "real reward: {row}");
        if rune.is_some() {
            assert_eq!(row["taskId"],"rb-woodcutting-xp-5m-780040");
            assert_eq!(row["repeat"],780040);assert!(row["seed"].is_null());
            assert!(row["environmentVersion"].as_str().unwrap().starts_with("sha256:"));
            assert_eq!(row["model"],"openai/gpt-5.6-luna");assert_eq!(row["effort"],"low");
        }
    }
    let entities = data.research_query(json!({"schemaVersion":"synth.trace-query.v2","evalJobIds":jobs,"grain":"entities","limit":1})).await.unwrap();
    assert!(entities.result_count > 4);
    let source = data.research_source(entities.snapshot_id.clone(), entities.result_ids[0].clone(), None, 0, 512).await.unwrap();
    assert_eq!(source["resolved"], true);
    drop(data); drop(svc); drop(storage);
    let reopened = Storage::open(root.join("native-store")).unwrap();
    let data = DataStore::new(reopened.database().clone(), ContentStore::new(reopened.content_root()));
    assert_eq!(data.research_query(query).await.unwrap().snapshot_id, snapshot.snapshot_id);
    std::fs::write(root.join("native-launch-acceptance.json"), serde_json::to_vec_pretty(&json!({
        "status":"passed","jobs":jobs,"rollouts":if rune.is_some(){1}else{4},"launch":"OptimizerService.start_recipe",
        "injectedJobRows":false,"providerCalls":if rune.is_some(){Value::Null}else{json!(0)},"annotations":false,"jesterky":false,
        "sourceResolved":true,"restart":true,"snapshotId":snapshot.snapshot_id
    })).unwrap()).unwrap();
}
