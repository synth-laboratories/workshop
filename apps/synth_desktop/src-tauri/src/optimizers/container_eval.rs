//! Workshop-owned baseline container evals. These are not GEPA campaigns:
//! maxGenerations=0, no candidate generation, no uplift claim. They talk to a
//! registered container over HTTP, retain rewards/evidence, and mint a chat-owned
//! `experiment.overview.v1` visual. They do not require the Optimizers sidecar.

use super::{
    models::{
        OptimizerCapabilities, OptimizerCreateRequest, OptimizerEventEnvelope,
        OptimizerExecutionBinding, OptimizerRecipeRunRequest, OptimizerResourceRef,
        OptimizerRunRecord, OPTIMIZER_EVENT_SCHEMA_VERSION,
    },
    recipes::{BANKING77_EVAL_BASELINE_RECIPE, HEALTHBENCH_EVAL_SMOKE_RECIPE},
    OptimizerService,
};
use crate::container_stream::{
    authoritative_poll_telemetry, declared_poll_url, declared_stream_descriptor,
    refuse_auto_transport, resolve_declared_url, wait_for_stream_subscribed, StreamDiagnostics,
    SUBSCRIBE_READY_TIMEOUT,
};
use crate::visuals::{
    VisualCreateRequest, VisualStatus, VisualUpdateRequest, VISUAL_BINDINGS_SCHEMA_VERSION,
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tokio::task::JoinSet;
use uuid::Uuid;

const EXPERIMENT_TEMPLATE: &str = "experiment.overview.v1";
const EXPERIMENT_SCHEMA: &str = "synth.experiment.overview.v1";
const EVAL_ALGORITHM_ID: &str = "eval";
const COST_CEILING_USD: f64 = 0.50;
const POLL_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(80);

#[derive(Clone, Copy)]
struct EvalSpec {
    recipe_id: &'static str,
    family: &'static str,
    title: &'static str,
    question: &'static str,
    world_ref: &'static str,
    evaluation_plan_ref: &'static str,
    harness: &'static str,
    policy_config: &'static str,
    concurrency: usize,
    train: &'static [i64],
    heldout: &'static [i64],
}

const BANKING77_TRAIN: [i64; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
const HEALTHBENCH_TRAIN: [i64; 2] = [0, 1];
const HEALTHBENCH_HELDOUT: [i64; 2] = [100, 101];

impl EvalSpec {
    fn for_recipe(recipe_id: &str) -> Result<Self> {
        match recipe_id {
            BANKING77_EVAL_BASELINE_RECIPE => Ok(Self {
                recipe_id: BANKING77_EVAL_BASELINE_RECIPE,
                family: "banking77",
                title: "Banking77 baseline eval",
                question: "What is the scored accuracy of the current classify policy on 10 labeled train examples?",
                world_ref: "world:banking77@train",
                evaluation_plan_ref: "banking77_eval.v1",
                harness: "classify",
                policy_config: "classify",
                concurrency: 10,
                train: &BANKING77_TRAIN,
                heldout: &[],
            }),
            HEALTHBENCH_EVAL_SMOKE_RECIPE => Ok(Self {
                recipe_id: HEALTHBENCH_EVAL_SMOKE_RECIPE,
                family: "healthbench",
                title: "HealthBench 2 zero-generation smoke",
                question: "What is the physician-rubric score of gpt-4.1-mini on the eval_smoke.toml pool?",
                world_ref: "world:healthbench@eval",
                evaluation_plan_ref: "healthbench_eval.v1",
                harness: "chat_completion",
                policy_config: "openai_gpt41_mini",
                concurrency: 2,
                train: &HEALTHBENCH_TRAIN,
                heldout: &HEALTHBENCH_HELDOUT,
            }),
            other => bail!("unknown container eval recipe: {other}"),
        }
    }

    fn examples(self) -> Vec<EvalExample> {
        self.train
            .iter()
            .map(|seed| EvalExample {
                pool: "train",
                seed: *seed,
            })
            .chain(self.heldout.iter().map(|seed| EvalExample {
                pool: "heldout",
                seed: *seed,
            }))
            .collect()
    }

    fn policy_config_body(self) -> Option<Value> {
        if self.family != "healthbench" {
            return None;
        }
        Some(json!({
            "config_id": self.policy_config,
            "harness": self.harness,
            "config": {
                "provider": "openai",
                "model": "gpt-4.1-mini-2025-04-14",
                "api_key_env": "OPENAI_API_KEY",
                "base_url": "https://api.openai.com/v1",
                "max_tokens": 1536,
            }
        }))
    }
}

#[derive(Clone, Copy)]
struct EvalExample {
    pool: &'static str,
    seed: i64,
}

#[derive(Clone)]
struct ReadyContainer {
    id: String,
    base_url: String,
}

pub(super) async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    let spec = EvalSpec::for_recipe(&request.recipe_id)?;
    let container = find_ready_container(service, spec.family).await?;
    let examples = spec.examples();
    let suffix = Uuid::new_v4().simple().to_string();
    let run_id = format!("opt_eval_{}_{}", spec.family, &suffix[..12]);
    let summary = json!({
        "recipeId": spec.recipe_id,
        "task": spec.family,
        "semantics": "baseline_eval",
        "containerProtocol": "synth_optimizers.gepa.v2",
        "containerId": container.id,
        "containerBaseUrl": container.base_url,
        "expectedVisual": EXPERIMENT_TEMPLATE,
        "policyRef": { "harness": spec.harness, "config": spec.policy_config },
        "taskPools": { "train": spec.train.len(), "heldout": spec.heldout.len() },
        "concurrency": spec.concurrency,
        "costCeilingUsd": COST_CEILING_USD,
        "records": [],
        "progress": { "completed": 0, "total": examples.len(), "failed": 0 },
    });
    let create = OptimizerCreateRequest {
        algorithm_id: EVAL_ALGORITHM_ID.into(),
        algorithm_version: Some("1".into()),
        objective: Some(spec.title.into()),
        source: Some("local".into()),
        project_ref: Some(format!("{}@eval", spec.family)),
        session_ref: request.session_ref.clone(),
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "container_http".into(),
            id: container.id.clone(),
            label: Some(format!("{} container", spec.family)),
            status: Some("starting".into()),
            metadata: json!({
                "recipeId": spec.recipe_id,
                "baseUrl": container.base_url,
            }),
        }]),
        input_refs: Some(vec![
            OptimizerResourceRef {
                kind: "recipe".into(),
                id: spec.recipe_id.into(),
                digest: None,
                role: Some("configuration".into()),
                title: Some(spec.title.into()),
                metadata: json!({ "semantics": "baseline_eval" }),
            },
            OptimizerResourceRef {
                kind: "container".into(),
                id: container.id.clone(),
                digest: None,
                role: Some("runtime".into()),
                title: Some(format!("Registered {} container", spec.family)),
                metadata: json!({ "baseUrl": container.base_url }),
            },
        ]),
        capabilities: Some(OptimizerCapabilities::for_algorithm(EVAL_ALGORITHM_ID)),
        summary: Some(summary),
        open_visual: Some(false),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;
    let visual_id = mint_experiment_visual(service, &run, spec, examples.len()).await?;
    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run.id.clone(), cancel_tx)
        .await;
    let worker = service.clone();
    let worker_run_id = run.id.clone();
    tokio::spawn(async move {
        if let Err(error) = run_eval_worker(
            worker.clone(),
            worker_run_id.clone(),
            spec,
            container,
            examples,
            visual_id,
            cancel_rx,
        )
        .await
        {
            let _ = append_terminal(&worker, &worker_run_id, true, error.to_string()).await;
        }
        worker.unregister_local_recipe(&worker_run_id).await;
    });
    Ok((service.get(run.id).await?, event))
}

async fn mint_experiment_visual(
    service: &OptimizerService,
    run: &OptimizerRunRecord,
    spec: EvalSpec,
    total: usize,
) -> Result<String> {
    let existing = run
        .visual_refs
        .iter()
        .find(|r| r.kind == "visual")
        .map(|r| r.id.clone());
    if let Some(visual_id) = existing {
        service
            .visuals()
            .show(visual_id.clone(), run.session_ref.clone())
            .await?;
        return Ok(visual_id);
    }
    let bindings = experiment_bindings(spec, "running", 0, total, &[], None);
    let (created, _) = service
        .visuals()
        .create(VisualCreateRequest {
            template_id: EXPERIMENT_TEMPLATE.into(),
            title: Some(spec.title.into()),
            bindings: Some(bindings),
            id: None,
            status: Some(VisualStatus::Live),
            renderer_kind: None,
            session_id: run.session_ref.clone(),
            message_id: None,
            run_id: None,
            trace_id: None,
            parent_visual_id: None,
            source_agent_id: None,
            source_model: None,
            content: None,
            metadata: Some(json!({
                "optimizerRunId": run.id,
                "recipeId": spec.recipe_id,
                "semantics": "baseline_eval",
            })),
        })
        .await?;
    let mut stored = service.get(run.id.clone()).await?;
    stored.visual_refs.push(OptimizerResourceRef {
        kind: "visual".into(),
        id: created.id.clone(),
        digest: None,
        role: Some("primary".into()),
        title: Some(created.title.clone()),
        metadata: json!({ "templateId": EXPERIMENT_TEMPLATE }),
    });
    let mut summary = stored.summary.as_object().cloned().unwrap_or_default();
    summary.insert("visualId".into(), json!(created.id));
    stored.summary = Value::Object(summary);
    service.persist_run(stored).await?;
    service
        .visuals()
        .show(created.id.clone(), run.session_ref.clone())
        .await?;
    Ok(created.id)
}

fn experiment_bindings(
    spec: EvalSpec,
    status: &str,
    completed: usize,
    total: usize,
    records: &[Value],
    mean_reward: Option<f64>,
) -> Value {
    let train_mean = mean_for_pool(records, "train");
    let heldout_mean = mean_for_pool(records, "heldout");
    json!({
        "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
        "slots": [{
            "slot": "experiment",
            "kind": "inline",
            "schema": EXPERIMENT_SCHEMA,
            "data": {
                "schemaVersion": EXPERIMENT_SCHEMA,
                "experimentId": spec.recipe_id,
                "title": spec.title,
                "question": spec.question,
                "status": status,
                "progress": {
                    "phase": if status == "completed" { "done" } else { "scoring" },
                    "completed": completed,
                    "total": total,
                },
                "metrics": [
                    {"label": "Train mean", "value": train_mean, "detail": if train_mean.is_none() { "omitted until the split is complete" } else { "mean of present rewards" }},
                    {"label": "Heldout mean", "value": heldout_mean, "detail": if spec.heldout.is_empty() { "this recipe has no heldout pool" } else if heldout_mean.is_none() { "omitted until the split is complete" } else { "mean of present rewards" }},
                    {"label": "Overall mean", "value": mean_reward, "detail": "missing rewards stay missing"}
                ],
                "arms": [{
                    "id": "baseline",
                    "label": spec.policy_config,
                    "baseline": true,
                    "status": status,
                    "score": mean_reward
                }],
                "records": records,
                "limitations": [
                    "Baseline-only. No candidate generation and no uplift claim."
                ]
            }
        }]
    })
}

fn mean_for_pool(records: &[Value], pool: &str) -> Option<f64> {
    let values = records
        .iter()
        .filter(|row| row.get("pool").and_then(Value::as_str) == Some(pool))
        .filter_map(|row| row.get("reward").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

async fn run_eval_worker(
    service: OptimizerService,
    run_id: String,
    spec: EvalSpec,
    container: ReadyContainer,
    examples: Vec<EvalExample>,
    visual_id: String,
    cancel: watch::Receiver<bool>,
) -> Result<()> {
    append_status(&service, &run_id, "optimizer.run.started", "running").await?;
    let client = crate::http::http_client();
    let info = match client.get(format!("{}/info", container.base_url)).send().await {
        Ok(response) if response.status().is_success() => {
            response.json::<Value>().await.unwrap_or(json!({}))
        }
        _ => json!({}),
    };
    let policy_pin = register_policy_pin(&client, &container.base_url, spec).await?;
    persist_policy_pin(&service, &run_id, &policy_pin).await?;
    let scale_leases = info
        .get("scale_leases")
        .or_else(|| info.pointer("/metadata/scale_leases"))
        .and_then(Value::as_u64)
        .unwrap_or(spec.concurrency as u64)
        .max(1) as usize;
    let permits = spec.concurrency.min(scale_leases).max(1);
    let total = examples.len();
    let mut remaining = examples.into_iter();
    let mut tasks: JoinSet<(EvalExample, Result<Value>)> = JoinSet::new();
    let mut records = Vec::new();
    let mut halt = None::<String>;

    loop {
        if *cancel.borrow() {
            bail!("container eval cancelled");
        }
        while tasks.len() < permits && halt.is_none() {
            if over_cost_ceiling(&records) {
                halt = Some(format!(
                    "cost ceiling ${COST_CEILING_USD:.2} reached; remaining rollouts were not dispatched"
                ));
                break;
            }
            let Some(example) = remaining.next() else {
                break;
            };
            let client = client.clone();
            let base = container.base_url.clone();
            let pin = policy_pin.clone();
            tasks.spawn(async move {
                let result = run_one_example(&client, &base, spec, example, &pin).await;
                (example, result)
            });
        }
        let Some(joined) = tasks.join_next().await else {
            break;
        };
        match joined {
            Ok((_example, Ok(record))) => records.push(record),
            Ok((example, Err(error))) => records.push(failed_record(example, spec, &policy_pin, error.to_string())),
            Err(error) => records.push(json!({
                "status": "failed",
                "error": error.to_string(),
                "policyRef": policy_pin,
            })),
        }
        persist_progress(
            &service,
            &run_id,
            spec,
            &visual_id,
            &records,
            total,
            "running",
        )
        .await?;
    }

    for example in remaining {
        records.push(failed_record(
            example,
            spec,
            &policy_pin,
            halt.clone().unwrap_or_else(|| {
                "required rollout was not dispatched".into()
            }),
        ));
    }

    let failed = records
        .iter()
        .filter(|row| !is_successful_eval_record(row))
        .count();
    let budget_exceeded = over_cost_ceiling(&records);
    let status = if failed == 0 && records.len() == total && !budget_exceeded {
        "completed"
    } else {
        "failed"
    };
    persist_progress(
        &service,
        &run_id,
        spec,
        &visual_id,
        &records,
        total,
        status,
    )
    .await?;
    let mut detail = String::new();
    if status == "failed" {
        let mut parts = Vec::new();
        if failed != 0 || records.len() != total {
            parts.push(format!("{failed} of {total} required rollouts failed"));
        }
        if budget_exceeded {
            parts.push(format!("cost exceeded ${COST_CEILING_USD:.2}"));
        }
        if let Some(halt) = halt {
            parts.push(halt);
        }
        detail = parts.join("; ");
    }
    append_terminal(&service, &run_id, status == "failed", detail).await?;
    Ok(())
}

async fn register_policy_pin(
    client: &reqwest::Client,
    base: &str,
    spec: EvalSpec,
) -> Result<Value> {
    let pin = json!({ "harness": spec.harness, "config": spec.policy_config });
    let Some(body) = spec.policy_config_body() else {
        return Ok(pin);
    };
    let response = client
        .post(format!("{base}/policy-configs"))
        .json(&body)
        .send()
        .await
        .context("POST /policy-configs")?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        bail!("policy config registration failed: {status} {text}");
    }
    let registered = response.json::<Value>().await.context("decode /policy-configs")?;
    let returned_id = registered
        .get("config_id")
        .or_else(|| registered.get("configId"))
        .and_then(Value::as_str);
    if returned_id != Some(spec.policy_config) {
        bail!(
            "policy config identity mismatch: wanted {}, got {returned_id:?}",
            spec.policy_config
        );
    }
    Ok(json!({
        "harness": spec.harness,
        "config": spec.policy_config,
        "configId": spec.policy_config,
        "immutable": true,
        "model": body.pointer("/config/model").cloned().unwrap_or(Value::Null),
        "provider": body.pointer("/config/provider").cloned().unwrap_or(Value::Null),
    }))
}

async fn persist_policy_pin(
    service: &OptimizerService,
    run_id: &str,
    policy_pin: &Value,
) -> Result<()> {
    let mut run = service.get(run_id.to_string()).await?;
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("policyPin".into(), policy_pin.clone());
    summary.insert("policyRef".into(), policy_pin.clone());
    run.summary = Value::Object(summary);
    service.persist_run(run).await?;
    Ok(())
}

fn failed_record(example: EvalExample, spec: EvalSpec, policy_pin: &Value, error: String) -> Value {
    json!({
        "pool": example.pool,
        "seed": example.seed,
        "taskInstanceId": format!("seed:{}", example.seed),
        "status": "failed",
        "error": error,
        "policyRef": policy_pin,
        "worldRef": spec.world_ref,
    })
}

fn is_successful_eval_record(row: &Value) -> bool {
    row.get("status").and_then(Value::as_str) == Some("completed")
}

fn over_cost_ceiling(records: &[Value]) -> bool {
    recorded_cost_usd(records).is_some_and(|cost| cost >= COST_CEILING_USD)
}

fn recorded_cost_usd(records: &[Value]) -> Option<f64> {
    let mut total = 0.0;
    let mut saw = false;
    for record in records {
        let usage = record.get("usage").unwrap_or(&Value::Null);
        for lane in [usage.get("policy"), usage.get("grader"), Some(usage)] {
            if let Some(cost) = lane.and_then(lane_cost_usd) {
                total += cost;
                saw = true;
            }
        }
    }
    saw.then_some(total)
}

fn lane_cost_usd(blob: &Value) -> Option<f64> {
    if blob.get("policy").is_some() || blob.get("grader").is_some() {
        return None;
    }
    blob.get("cost_usd")
        .or_else(|| blob.get("costUsd"))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
}

async fn persist_progress(
    service: &OptimizerService,
    run_id: &str,
    spec: EvalSpec,
    visual_id: &str,
    records: &[Value],
    total: usize,
    status: &str,
) -> Result<()> {
    let completed = records.len();
    let mean = mean_for_pool(records, "train").or_else(|| mean_reward(records));
    let mut run = service.get(run_id.to_string()).await?;
    let usage = usage_from_records(records);
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("records".into(), json!(records));
    summary.insert(
        "progress".into(),
        json!({
            "completed": completed,
            "total": total,
            "failed": records.iter().filter(|row| !is_successful_eval_record(row)).count()
        }),
    );
    if let Some(mean) = mean {
        summary.insert("meanReward".into(), json!(mean));
    }
    summary.insert("evalStatus".into(), json!(status));
    summary.insert("costCeilingUsd".into(), json!(COST_CEILING_USD));
    if let Some(cost) = usage.cost_usd {
        summary.insert("costUsd".into(), json!(cost));
    }
    summary.insert(
        "usageLanes".into(),
        json!({
            "policy": usage.extra.get("policyUsage").cloned().unwrap_or(Value::Null),
            "grader": usage.extra.get("graderUsage").cloned().unwrap_or(Value::Null),
        }),
    );
    run.summary = Value::Object(summary);
    run.usage = usage;
    service.persist_run(run).await?;

    let visual_status = if status == "failed" {
        VisualStatus::Failed
    } else if status == "completed" {
        VisualStatus::Saved
    } else {
        VisualStatus::Live
    };
    if let Err(error) = service
        .visuals()
        .update(
            visual_id.to_string(),
            VisualUpdateRequest {
                title: None,
                bindings: Some(experiment_bindings(
                    spec, status, completed, total, records, mean,
                )),
                status: Some(visual_status),
                renderer_kind: None,
                message_id: None,
                run_id: None,
                trace_id: None,
                content: None,
                metadata: None,
                bump_revision: Some(true),
            },
        )
        .await
    {
        let mut run = service.get(run_id.to_string()).await?;
        let mut summary = run.summary.as_object().cloned().unwrap_or_default();
        summary.insert("visualProjectionError".into(), json!(error.to_string()));
        run.summary = Value::Object(summary);
        service.persist_run(run).await?;
        if status != "running" {
            bail!("experiment visual projection failed: {error}");
        }
    }
    Ok(())
}

fn usage_from_records(records: &[Value]) -> super::models::OptimizerUsageSummary {
    let mut usage = super::models::OptimizerUsageSummary::default();
    usage.rollouts = records.len() as u64;
    let mut policy = LaneUsage::default();
    let mut grader = LaneUsage::default();
    let mut cost_sum = 0.0;
    let mut cost_complete = true;
    let mut saw_cost = false;
    for record in records {
        let blob = record.get("usage").cloned().unwrap_or(Value::Null);
        if blob.get("policy").is_some() || blob.get("grader").is_some() {
            add_lane(&mut policy, blob.get("policy"));
            add_lane(&mut grader, blob.get("grader"));
        } else {
            add_lane(&mut policy, Some(&blob));
        }
    }
    usage.prompt_tokens = policy.prompt_tokens + grader.prompt_tokens;
    usage.completion_tokens = policy.completion_tokens + grader.completion_tokens;
    for lane in [&policy, &grader] {
        match lane.cost_usd {
            Some(cost) => {
                cost_sum += cost;
                saw_cost = true;
            }
            None if lane.saw_tokens => cost_complete = false,
            None => {}
        }
    }
    usage.cost_usd = if saw_cost && cost_complete {
        Some(cost_sum)
    } else if saw_cost && !cost_complete {
        None
    } else {
        None
    };
    if saw_cost {
        usage
            .extra
            .insert("costTelemetryComplete".into(), json!(cost_complete));
    }
    usage.extra.insert("costCeilingUsd".into(), json!(COST_CEILING_USD));
    usage.extra.insert("policyUsage".into(), policy.to_json());
    usage.extra.insert("graderUsage".into(), grader.to_json());
    usage
}

#[derive(Default)]
struct LaneUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    cost_usd: Option<f64>,
    saw_tokens: bool,
}

impl LaneUsage {
    fn to_json(&self) -> Value {
        json!({
            "promptTokens": self.prompt_tokens,
            "completionTokens": self.completion_tokens,
            "costUsd": self.cost_usd,
        })
    }
}

fn add_lane(lane: &mut LaneUsage, blob: Option<&Value>) {
    let Some(blob) = blob.filter(|value| value.is_object()) else {
        return;
    };
    if let Some(tokens) = u64_field(blob, &["prompt_tokens", "promptTokens"]) {
        lane.prompt_tokens += tokens;
        lane.saw_tokens = true;
    }
    if let Some(tokens) = u64_field(blob, &["completion_tokens", "completionTokens"]) {
        lane.completion_tokens += tokens;
        lane.saw_tokens = true;
    }
    if let Some(cost) = lane_cost_usd(blob) {
        lane.cost_usd = Some(lane.cost_usd.unwrap_or(0.0) + cost);
    }
}

fn u64_field(blob: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| blob.get(*key).and_then(Value::as_u64))
}

fn mean_reward(records: &[Value]) -> Option<f64> {
    let values = records
        .iter()
        .filter_map(|row| row.get("reward").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

async fn run_one_example(
    client: &reqwest::Client,
    base: &str,
    spec: EvalSpec,
    example: EvalExample,
    policy_pin: &Value,
) -> Result<Value> {
    let telemetry = {
        let mut telemetry = authoritative_poll_telemetry();
        if let Some(object) = telemetry.as_object_mut() {
            object.insert("retention".into(), json!("run"));
        }
        refuse_auto_transport(&telemetry)?;
        telemetry
    };
    let rollout_id = format!(
        "roll_{}_{}_{}_{}",
        spec.family,
        example.pool,
        example.seed,
        &Uuid::new_v4().simple().to_string()[..8]
    );
    let prepare = client
        .post(format!("{base}/rollouts/prepare"))
        .json(&json!({ "rollout_id": rollout_id, "telemetry": telemetry }))
        .send()
        .await
        .context("POST /rollouts/prepare")?;
    if !prepare.status().is_success() {
        bail!(
            "prepare failed for {} seed {}: {}",
            example.pool,
            example.seed,
            prepare.status()
        );
    }
    let prepared = prepare.json::<Value>().await?;
    let stream = declared_stream_descriptor(&prepared)?
        .context("prepare omitted stream descriptor")?;
    let poll_url = resolve_declared_url(base, &declared_poll_url(&stream)?)?;
    wait_for_stream_subscribed(
        client,
        &poll_url,
        SUBSCRIBE_READY_TIMEOUT,
        &StreamDiagnostics::none(),
    )
    .await?;

    let start_body = json!({
        "rollout_id": rollout_id,
        "submission_mode": "sync",
        "slot": "stream",
        "telemetry": telemetry,
        "task_instance_id": format!("seed:{}", example.seed),
        "world_ref": spec.world_ref,
        "evaluation_plan_ref": spec.evaluation_plan_ref,
        "policy_ref": { "harness": spec.harness, "config": spec.policy_config },
    });
    let started = client
        .post(format!("{base}/rollouts"))
        .json(&start_body)
        .send()
        .await
        .context("POST /rollouts")?;
    if !started.status().is_success() {
        let status = started.status();
        let body = started.text().await.unwrap_or_default();
        bail!("POST /rollouts failed for {} seed {}: {status} {body}", example.pool, example.seed);
    }
    let mut state = started.json::<Value>().await?;
    if !rollout_terminal(&state) {
        state = poll_until_terminal(client, base, &rollout_id).await?;
    }
    let reward = fetch_reward(client, base, &rollout_id, spec.evaluation_plan_ref).await;
    let usage = state.get("usage").cloned().unwrap_or(Value::Null);
    Ok(json!({
        "rolloutId": rollout_id,
        "pool": example.pool,
        "seed": example.seed,
        "taskInstanceId": format!("seed:{}", example.seed),
        "status": state.get("status").cloned().unwrap_or(json!("completed")),
        "terminated": rollout_terminal(&state),
        "reward": reward.get("reward").cloned().unwrap_or(Value::Null),
        "rewardStatus": reward.get("status").cloned().unwrap_or(json!("absent")),
        "usage": usage,
        "policyRef": policy_pin,
        "worldRef": spec.world_ref,
        "trace": state.get("trace").cloned().unwrap_or(Value::Null),
        "evidence": {
            "eventsUrl": format!("{base}/rollouts/{rollout_id}/events"),
            "rewardUrl": format!("{base}/rollouts/{rollout_id}/reward"),
        }
    }))
}

fn rollout_terminal(state: &Value) -> bool {
    state.get("terminated").and_then(Value::as_bool) == Some(true)
        || matches!(
            state.get("status").and_then(Value::as_str),
            Some("completed" | "failed" | "cancelled" | "truncated")
        )
}

async fn poll_until_terminal(
    client: &reqwest::Client,
    base: &str,
    rollout_id: &str,
) -> Result<Value> {
    let deadline = Instant::now() + POLL_TIMEOUT;
    loop {
        let response = client
            .get(format!("{base}/rollouts/{rollout_id}"))
            .send()
            .await
            .context("GET /rollouts/{id}")?;
        if response.status().is_success() {
            let state = response.json::<Value>().await?;
            if rollout_terminal(&state) {
                return Ok(state);
            }
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for terminal state of {rollout_id}");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn fetch_reward(
    client: &reqwest::Client,
    base: &str,
    rollout_id: &str,
    plan_ref: &str,
) -> Value {
    if let Ok(response) = client
        .get(format!("{base}/rollouts/{rollout_id}/reward"))
        .send()
        .await
    {
        if response.status().is_success() {
            if let Ok(body) = response.json::<Value>().await {
                if body.get("reward").and_then(Value::as_f64).is_some()
                    || body.get("status").and_then(Value::as_str) == Some("scored")
                {
                    return body;
                }
            }
        }
    }
    if let Ok(response) = client
        .post(format!("{base}/reward"))
        .json(&json!({
            "rollout_id": rollout_id,
            "mode": "terminal",
            "evaluation_plan_ref": plan_ref,
        }))
        .send()
        .await
    {
        if let Ok(body) = response.json::<Value>().await {
            return body;
        }
    }
    json!({ "status": "absent", "reward": null, "rollout_id": rollout_id })
}

async fn find_ready_container(service: &OptimizerService, family: &str) -> Result<ReadyContainer> {
    let family = family.to_string();
    let rows = service
        .database()
        .clone()
        .run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, status, base_url, task_family, metadata_json
                 FROM containers
                 ORDER BY updated_at DESC, id",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;
    let mut seen = Vec::new();
    for (id, status, base_url, task_family, metadata_json) in rows {
        let metadata: Value = serde_json::from_str(&metadata_json).unwrap_or(json!({}));
        let matches = container_matches_family(task_family.as_deref(), &metadata, &family);
        let ready = matches!(
            status.to_ascii_lowercase().as_str(),
            "ready" | "healthy" | "running" | "live"
        );
        if matches {
            seen.push(format!("{id} ({status})"));
        }
        if matches && ready {
            if let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) {
                return Ok(ReadyContainer {
                    id,
                    base_url: base_url.trim_end_matches('/').to_string(),
                });
            }
        }
    }
    if seen.is_empty() {
        bail!(
            "no registered {family} container. Register a healthy {family} GEPA v2 pool before starting this baseline eval."
        );
    }
    bail!(
        "registered {family} containers are not ready: {}. Probe and wait until status is ready/healthy.",
        seen.join(", ")
    )
}

fn container_matches_family(task_family: Option<&str>, metadata: &Value, family: &str) -> bool {
    let mut candidates = Vec::new();
    if let Some(value) = task_family {
        candidates.push(value.to_ascii_lowercase());
    }
    for key in ["runtime_family", "env_family", "task_family", "target_id"] {
        if let Some(value) = metadata.get(key).and_then(Value::as_str) {
            candidates.push(value.to_ascii_lowercase());
        }
    }
    candidates
        .iter()
        .any(|value| value == family || value.contains(family))
}

async fn append_status(
    service: &OptimizerService,
    run_id: &str,
    event_type: &str,
    status: &str,
) -> Result<()> {
    let run = service.get(run_id.to_string()).await?;
    service
        .append_events(
            run_id.to_string(),
            vec![OptimizerEventEnvelope {
                schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
                event_id: Some(format!("{run_id}:eval:{}", run.cursor_seq + 1)),
                event_type: event_type.into(),
                sequence_number: run.cursor_seq + 1,
                occurred_at: chrono::Utc::now().to_rfc3339(),
                optimizer_run_id: run_id.into(),
                algorithm_id: EVAL_ALGORITHM_ID.into(),
                level: None,
                item: None,
                delta: Map::from_iter([("status".into(), json!(status))]),
                snapshot: None,
                usage_delta: None,
                artifact_refs: vec![],
                error: None,
                raw: json!({ "source": "container_eval" }),
            }],
        )
        .await?;
    Ok(())
}

async fn append_terminal(
    service: &OptimizerService,
    run_id: &str,
    failed: bool,
    detail: String,
) -> Result<()> {
    let run = service.get(run_id.to_string()).await?;
    if matches!(run.status.as_str(), "completed" | "failed" | "cancelled") {
        return Ok(());
    }
    append_status(
        service,
        run_id,
        if failed {
            "optimizer.run.failed"
        } else {
            "optimizer.run.completed"
        },
        if failed { "failed" } else { "completed" },
    )
    .await?;
    if failed && !detail.is_empty() {
        let run = service.get(run_id.to_string()).await?;
        service
            .append_events(
                run_id.to_string(),
                vec![OptimizerEventEnvelope {
                    schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
                    event_id: Some(format!("{run_id}:eval:{}", run.cursor_seq + 1)),
                    event_type: "optimizer.run.error".into(),
                    sequence_number: run.cursor_seq + 1,
                    occurred_at: chrono::Utc::now().to_rfc3339(),
                    optimizer_run_id: run_id.into(),
                    algorithm_id: EVAL_ALGORITHM_ID.into(),
                    level: Some("error".into()),
                    item: None,
                    delta: Map::new(),
                    snapshot: None,
                    usage_delta: None,
                    artifact_refs: vec![],
                    error: Some(json!({ "message": detail })),
                    raw: json!({ "source": "container_eval" }),
                }],
            )
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{serve_json, JsonHttpRequest, JsonHttpResponse};
    use crate::optimizers::models::OptimizerRecipeRunRequest;
    use crate::optimizers::service::tests::service;
    use crate::visuals::VisualQuery;
    use hyper::StatusCode;
    use rusqlite::params;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    #[derive(Clone)]
    struct MockEvalOptions {
        family: &'static str,
        rewards: BTreeMap<(String, i64), f64>,
        policy_status: u16,
        policy_config_id: String,
        fail_seeds: BTreeSet<i64>,
        extra_cost_usd: Option<f64>,
    }

    async fn spawn_eval_mock(
        family: &'static str,
        rewards: BTreeMap<(String, i64), f64>,
    ) -> (String, tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
        spawn_eval_mock_opts(MockEvalOptions {
            family,
            rewards,
            policy_status: 200,
            policy_config_id: "openai_gpt41_mini".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: None,
        })
        .await
    }

    async fn spawn_eval_mock_opts(
        opts: MockEvalOptions,
    ) -> (String, tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
        let starts = Arc::new(AtomicUsize::new(0));
        let starts_h = starts.clone();
        let prepared = Arc::new(Mutex::new(BTreeMap::<String, bool>::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = serve_json(listener, move |request: JsonHttpRequest| {
                let starts = starts_h.clone();
                let prepared = prepared.clone();
                let opts = opts.clone();
                async move {
                    let path = request.path.split('?').next().unwrap_or(&request.path);
                    let family = opts.family;
                    match (request.method.as_str(), path) {
                        ("GET", "/info") => JsonHttpResponse::ok(json!({
                            "runtime_family": family,
                            "scale_leases": 10,
                            "world_ref": if family == "banking77" { "world:banking77@train" } else { "world:healthbench@eval" },
                        })),
                        ("POST", path) if path == "/policy-configs" || path.starts_with("/policy-configs/") => {
                            if opts.policy_status == 200 {
                                JsonHttpResponse::ok(json!({"config_id": opts.policy_config_id}))
                            } else {
                                JsonHttpResponse::error(
                                    StatusCode::from_u16(opts.policy_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                                    "policy config refused",
                                )
                            }
                        }
                        ("POST", "/rollouts/prepare") => {
                            let rollout_id = request
                                .body
                                .get("rollout_id")
                                .and_then(Value::as_str)
                                .unwrap_or("roll_unknown")
                                .to_string();
                            prepared.lock().unwrap().insert(rollout_id.clone(), true);
                            JsonHttpResponse::ok(json!({
                                "rollout_id": rollout_id,
                                "stream": {
                                    "id": format!("stream:{rollout_id}"),
                                    "transports": { "poll": { "url": format!("/rollouts/{rollout_id}/events") } }
                                }
                            }))
                        }
                        ("GET", path) if path.ends_with("/events") => JsonHttpResponse::ok(json!({
                            "events": [{ "kind": "stream.subscribed", "ready": true }]
                        })),
                        ("POST", "/rollouts") => {
                            starts.fetch_add(1, Ordering::SeqCst);
                            let rollout_id = request
                                .body
                                .get("rollout_id")
                                .and_then(Value::as_str)
                                .unwrap_or("roll_unknown")
                                .to_string();
                            let seed = request
                                .body
                                .get("task_instance_id")
                                .and_then(Value::as_str)
                                .and_then(|value| value.rsplit(':').next())
                                .and_then(|value| value.parse::<i64>().ok())
                                .unwrap_or(0);
                            if opts.fail_seeds.contains(&seed) {
                                return JsonHttpResponse::error(
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("forced failure for seed {seed}"),
                                );
                            }
                            let pool = if rollout_id.contains("heldout") {
                                "heldout"
                            } else {
                                "train"
                            };
                            let policy_cost = opts.extra_cost_usd.unwrap_or(0.001);
                            let grader_cost = if opts.extra_cost_usd.is_some() { 0.0 } else { 0.002 };
                            JsonHttpResponse::ok(json!({
                                "rollout_id": rollout_id,
                                "status": "completed",
                                "terminated": true,
                                "usage": if family == "healthbench" {
                                    json!({
                                        "policy": {"prompt_tokens": 11, "completion_tokens": 4, "cost_usd": policy_cost},
                                        "grader": {"prompt_tokens": 40, "completion_tokens": 12, "cost_usd": grader_cost}
                                    })
                                } else if let Some(cost) = opts.extra_cost_usd {
                                    json!({"prompt_tokens": 8, "completion_tokens": 1, "cost_usd": cost})
                                } else {
                                    json!({"prompt_tokens": 8, "completion_tokens": 1})
                                },
                                "trace": { "url": format!("/rollouts/{rollout_id}/trace"), "closed": true },
                                "seed": seed,
                                "pool": pool,
                            }))
                        }
                        ("GET", path) if path.starts_with("/rollouts/") && path.ends_with("/reward") => {
                            let rollout_id = path
                                .trim_start_matches("/rollouts/")
                                .trim_end_matches("/reward")
                                .to_string();
                            let seed = rollout_id
                                .split('_')
                                .rev()
                                .nth(1)
                                .and_then(|value| value.parse::<i64>().ok())
                                .unwrap_or(0);
                            let pool = if rollout_id.contains("heldout") {
                                "heldout".into()
                            } else {
                                "train".into()
                            };
                            let reward = opts.rewards.get(&(pool, seed)).copied().unwrap_or(1.0);
                            JsonHttpResponse::ok(json!({
                                "status": "scored",
                                "reward": reward,
                                "rollout_id": rollout_id
                            }))
                        }
                        ("GET", path) if path.starts_with("/rollouts/") => {
                            JsonHttpResponse::ok(json!({
                                "status": "completed",
                                "terminated": true
                            }))
                        }
                        _ => JsonHttpResponse::error(
                            StatusCode::NOT_FOUND,
                            format!("unexpected {} {path}", request.method),
                        ),
                    }
                }
            })
            .await;
        });
        (format!("http://{addr}"), task, starts)
    }

    async fn insert_container(
        service: &OptimizerService,
        family: &str,
        base_url: &str,
        status: &str,
    ) {
        let id = format!("ctr_{family}_test");
        let family = family.to_string();
        let base_url = base_url.to_string();
        let status = status.to_string();
        service
            .database()
            .clone()
            .run_transaction(move |conn| {
                conn.execute(
                    "INSERT INTO containers(id,name,location,status,base_url,task_family,health_json,metadata_json,created_at,updated_at)
                     VALUES(?1,?2,'local',?3,?4,?5,'{\"ok\":true}',?6,'2026-08-17','2026-08-17')",
                    params![
                        id,
                        format!("{family} test"),
                        status,
                        base_url,
                        family,
                        json!({"runtime_family": family}).to_string()
                    ],
                )?;
                Ok(())
            })
            .await
            .unwrap();
    }

    async fn wait_terminal(service: &OptimizerService, run_id: &str) -> OptimizerRunRecord {
        for _ in 0..200 {
            let run = service.get(run_id.to_string()).await.unwrap();
            if matches!(run.status.as_str(), "completed" | "failed" | "cancelled") {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("run {run_id} did not reach a terminal record");
    }

    #[tokio::test]
    async fn banking77_eval_reaches_terminal_records_and_experiment_visual() {
        let rewards = (0..10)
            .map(|seed| (("train".into(), seed), if seed < 7 { 1.0 } else { 0.0 }))
            .collect();
        let (base, task, starts) = spawn_eval_mock("banking77", rewards).await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "banking77", &base, "ready").await;
        let (run, _) = svc
            .start_recipe(OptimizerRecipeRunRequest {
                recipe_id: BANKING77_EVAL_BASELINE_RECIPE.into(),
                session_ref: Some("sess_banking77_eval".into()),
                open_visual: Some(true),
                base_model: None,
                dataset_shard: None,
                candidate_set_id: None,
            })
            .await
            .unwrap();
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "completed");
        let records = finished
            .summary
            .get("records")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert_eq!(records.len(), 10);
        assert!(records.iter().all(|row| row.get("reward").and_then(Value::as_f64).is_some()));
        assert_eq!(starts.load(Ordering::SeqCst), 10);
        let visual_id = finished
            .summary
            .get("visualId")
            .and_then(Value::as_str)
            .expect("chat-owned experiment visual");
        let visual = svc.visuals().get(visual_id.to_string()).await.unwrap();
        assert_eq!(visual.template_id, EXPERIMENT_TEMPLATE);
        assert_eq!(visual.session_id.as_deref(), Some("sess_banking77_eval"));
        let data = visual
            .bindings
            .pointer("/slots/0/data")
            .cloned()
            .unwrap_or(Value::Null);
        assert_eq!(data["status"], json!("completed"));
        assert_eq!(data["progress"]["completed"], json!(10));
        assert_eq!(finished.usage.prompt_tokens, 80);
        assert_eq!(finished.usage.completion_tokens, 10);
        assert_eq!(finished.usage.rollouts, 10);
        assert_eq!(finished.usage.cost_usd, None);
        task.abort();
    }

    #[tokio::test]
    async fn healthbench_eval_reaches_terminal_records_with_policy_and_grader_usage() {
        let mut rewards = BTreeMap::new();
        rewards.insert(("train".into(), 0), 0.8);
        rewards.insert(("train".into(), 1), 0.6);
        rewards.insert(("heldout".into(), 100), 0.5);
        rewards.insert(("heldout".into(), 101), 0.4);
        let (base, task, starts) = spawn_eval_mock("healthbench", rewards).await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "healthbench", &base, "ready").await;
        let (run, _) = svc
            .start_recipe(OptimizerRecipeRunRequest {
                recipe_id: HEALTHBENCH_EVAL_SMOKE_RECIPE.into(),
                session_ref: Some("sess_healthbench_eval".into()),
                open_visual: Some(true),
                base_model: None,
                dataset_shard: None,
                candidate_set_id: None,
            })
            .await
            .unwrap();
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "completed");
        let records = finished
            .summary
            .get("records")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert_eq!(records.len(), 4);
        assert_eq!(starts.load(Ordering::SeqCst), 4);
        assert!(records.iter().any(|row| row["pool"] == json!("heldout")));
        assert!(records.iter().all(|row| {
            row.pointer("/usage/policy").is_some()
                && row.pointer("/usage/grader").is_some()
                && row.pointer("/policyRef/config") == Some(&json!("openai_gpt41_mini"))
        }));
        assert_eq!(
            finished.summary.pointer("/policyPin/config"),
            Some(&json!("openai_gpt41_mini"))
        );
        assert_eq!(finished.usage.prompt_tokens, 204);
        assert_eq!(finished.usage.completion_tokens, 64);
        assert_eq!(finished.usage.rollouts, 4);
        assert!((finished.usage.cost_usd.unwrap() - 0.012).abs() < 1e-9);
        assert_eq!(
            finished.usage.extra["policyUsage"]["promptTokens"],
            json!(44)
        );
        assert_eq!(
            finished.usage.extra["graderUsage"]["promptTokens"],
            json!(160)
        );
        let listed = svc
            .visuals()
            .list(VisualQuery {
                session_id: Some("sess_healthbench_eval".into()),
                template_id: Some(EXPERIMENT_TEMPLATE.into()),
                ..VisualQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        task.abort();
    }

    #[tokio::test]
    async fn container_eval_fails_fast_without_a_registered_pool() {
        let (svc, _dir, _) = service().await;
        let error = svc
            .start_recipe(OptimizerRecipeRunRequest {
                recipe_id: BANKING77_EVAL_BASELINE_RECIPE.into(),
                session_ref: None,
                open_visual: Some(false),
                base_model: None,
                dataset_shard: None,
                candidate_set_id: None,
            })
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap();
        assert!(
            error.contains("no registered banking77 container"),
            "expected a structured missing-container error, got {error}"
        );
        assert!(!error.contains("unknown optimizer recipe"));
    }

    fn recipe(id: &str, session: &str) -> OptimizerRecipeRunRequest {
        OptimizerRecipeRunRequest {
            recipe_id: id.into(),
            session_ref: Some(session.into()),
            open_visual: Some(true),
            base_model: None,
            dataset_shard: None,
            candidate_set_id: None,
        }
    }

    #[tokio::test]
    async fn healthbench_fails_closed_when_policy_config_registration_is_rejected() {
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "healthbench",
            rewards: BTreeMap::new(),
            policy_status: 503,
            policy_config_id: "openai_gpt41_mini".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: None,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "healthbench", &base, "ready").await;
        let (run, _) = svc
            .start_recipe(recipe(HEALTHBENCH_EVAL_SMOKE_RECIPE, "sess_policy_reject"))
            .await
            .unwrap();
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "failed");
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        task.abort();
    }

    #[tokio::test]
    async fn healthbench_fails_closed_when_policy_config_identity_mismatches() {
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "healthbench",
            rewards: BTreeMap::new(),
            policy_status: 200,
            policy_config_id: "groq_llama31_8b".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: None,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "healthbench", &base, "ready").await;
        let (run, _) = svc
            .start_recipe(recipe(HEALTHBENCH_EVAL_SMOKE_RECIPE, "sess_policy_mismatch"))
            .await
            .unwrap();
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "failed");
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        task.abort();
    }

    #[tokio::test]
    async fn banking77_partial_failure_is_failed_not_completed() {
        let rewards = (0..10)
            .map(|seed| (("train".into(), seed), 1.0))
            .collect();
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "banking77",
            rewards,
            policy_status: 200,
            policy_config_id: "classify".into(),
            fail_seeds: (1..10).collect(),
            extra_cost_usd: None,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "banking77", &base, "ready").await;
        let (run, _) = svc
            .start_recipe(recipe(BANKING77_EVAL_BASELINE_RECIPE, "sess_partial"))
            .await
            .unwrap();
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "failed");
        assert_eq!(starts.load(Ordering::SeqCst), 10);
        assert_eq!(finished.summary["progress"]["failed"], json!(9));
        assert_ne!(finished.summary["evalStatus"], json!("completed"));
        task.abort();
    }

    #[tokio::test]
    async fn healthbench_stops_dispatch_once_cost_ceiling_is_reached() {
        let mut rewards = BTreeMap::new();
        rewards.insert(("train".into(), 0), 0.8);
        rewards.insert(("train".into(), 1), 0.6);
        rewards.insert(("heldout".into(), 100), 0.5);
        rewards.insert(("heldout".into(), 101), 0.4);
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "healthbench",
            rewards,
            policy_status: 200,
            policy_config_id: "openai_gpt41_mini".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: Some(0.50),
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "healthbench", &base, "ready").await;
        let (run, _) = svc
            .start_recipe(recipe(HEALTHBENCH_EVAL_SMOKE_RECIPE, "sess_budget"))
            .await
            .unwrap();
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "failed");
        assert!(starts.load(Ordering::SeqCst) <= 2);
        assert_eq!(
            finished
                .summary
                .get("records")
                .and_then(Value::as_array)
                .map(|rows| rows.len()),
            Some(4)
        );
        task.abort();
    }
}
