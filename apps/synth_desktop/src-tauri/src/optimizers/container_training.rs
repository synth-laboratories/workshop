//! Bind SFT/CISPO from a registered container's advertised contract.
//!
//! Workshop does not ship task identity. A ready container must advertise
//! `/workshop/manifest` and/or `/info` `optimizer_contracts`. Missing fields
//! fail closed; Workshop never substitutes a named task family.

use super::OptimizerService;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

const FETCH_TIMEOUT: Duration = Duration::from_secs(8);
// Container-owned SFT exports are allowed to stream a full benchmark corpus.
// Keep contract discovery responsive, but do not impose an API-style deadline
// on downloading the declared training data.
const SFT_MATERIALIZATION_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Debug)]
pub struct ReadyTrainingContainer {
    pub id: String,
    pub base_url: String,
}

#[derive(Clone, Debug)]
pub struct CispoContract {
    pub rollout_url: String,
    pub reward_url: Option<String>,
    pub implementation: String,
    pub harness: String,
    pub plan_ref: String,
    pub train_world_ref: String,
    pub heldout_world_ref: String,
    pub token: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SftContract {
    pub train_jsonl_url: String,
    pub eval_jsonl_url: String,
}

#[derive(Clone, Debug)]
pub struct ContainerTrainingBind {
    pub container_id: String,
    pub base_url: String,
    pub task_id: String,
    pub cispo: Option<CispoContract>,
    pub sft: Option<SftContract>,
}

pub async fn bind(
    service: &OptimizerService,
    container_id: Option<&str>,
) -> Result<ContainerTrainingBind> {
    let ready = ready_containers(service).await?;
    let selected = select_container(&ready, container_id)?;
    fetch_bind(&selected).await
}

pub async fn bind_cispo(
    service: &OptimizerService,
    container_id: Option<&str>,
) -> Result<(ContainerTrainingBind, CispoContract)> {
    let bind = bind(service, container_id).await?;
    let cispo = bind.cispo.clone().ok_or_else(|| {
        anyhow!(
            "container `{}` does not advertise a CISPO contract on /workshop/manifest or /info",
            bind.container_id
        )
    })?;
    Ok((bind, cispo))
}

async fn ready_containers(service: &OptimizerService) -> Result<Vec<ReadyTrainingContainer>> {
    let rows = service
        .database()
        .clone()
        .run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, status, base_url
                 FROM containers
                 ORDER BY updated_at DESC, id",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, status, base_url)| {
            let ready = matches!(
                status.to_ascii_lowercase().as_str(),
                "ready" | "healthy" | "running" | "live"
            );
            let base_url = base_url?.trim().trim_end_matches('/').to_string();
            if ready && !base_url.is_empty() {
                Some(ReadyTrainingContainer { id, base_url })
            } else {
                None
            }
        })
        .collect())
}

fn select_container(
    ready: &[ReadyTrainingContainer],
    requested_id: Option<&str>,
) -> Result<ReadyTrainingContainer> {
    if let Some(requested_id) = requested_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return ready
            .iter()
            .find(|container| container.id == requested_id)
            .cloned()
            .ok_or_else(|| {
                anyhow!("requested container `{requested_id}` is not a ready registered pool")
            });
    }
    match ready {
        [] => bail!(
            "local training requires a ready registered container that advertises a training contract; pass containerId"
        ),
        [only] => Ok(only.clone()),
        many => bail!(
            "ambiguous registered training containers: {}. Pass the explicit containerId; refusing to substitute a container.",
            many.iter()
                .map(|container| container.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

async fn fetch_bind(container: &ReadyTrainingContainer) -> Result<ContainerTrainingBind> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .context("training-contract HTTP client")?;
    let info = get_json(&client, &format!("{}/info", container.base_url))
        .await
        .or(get_json(&client, &format!("{}/metadata", container.base_url)).await)
        .ok();
    let manifest_route = info
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/metadata/workshop_manifest_route")
                .or_else(|| value.get("workshop_manifest_route"))
                .and_then(Value::as_str)
        })
        .unwrap_or("/workshop/manifest");
    let manifest = get_json(&client, &join_route(&container.base_url, manifest_route))
        .await
        .ok();
    parse_bind(container, manifest.as_ref(), info.as_ref())
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<Value> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !response.status().is_success() {
        bail!("GET {url} returned {}", response.status());
    }
    response
        .json()
        .await
        .with_context(|| format!("decode {url}"))
}

fn parse_bind(
    container: &ReadyTrainingContainer,
    manifest: Option<&Value>,
    info: Option<&Value>,
) -> Result<ContainerTrainingBind> {
    let task_id = first_string(&[
        manifest.and_then(|value| value.get("task")),
        manifest.and_then(|value| value.get("task_id")),
        info.and_then(|value| value.pointer("/task/task_id")),
        info.and_then(|value| value.pointer("/runtime/runtime_id")),
    ])
    .ok_or_else(|| {
        anyhow!(
            "container `{}` did not advertise a task id on /workshop/manifest or /info",
            container.id
        )
    })?;
    let cispo = parse_cispo(&container.base_url, &task_id, manifest, info);
    let sft = parse_sft(&container.base_url, manifest, info);
    if cispo.is_none() && sft.is_none() {
        bail!(
            "container `{}` advertised task `{task_id}` but no SFT or CISPO contract",
            container.id
        );
    }
    Ok(ContainerTrainingBind {
        container_id: container.id.clone(),
        base_url: container.base_url.clone(),
        task_id,
        cispo,
        sft,
    })
}

fn parse_cispo(
    base_url: &str,
    task_id: &str,
    manifest: Option<&Value>,
    info: Option<&Value>,
) -> Option<CispoContract> {
    let cispo = manifest.and_then(|value| value.get("cispo"));
    let contracts = info.and_then(|value| value.pointer("/metadata/optimizer_contracts/cispo"));
    let capabilities = info.and_then(|value| value.pointer("/capabilities/cispo"));
    if cispo.is_none() && contracts.is_none() && capabilities.is_none() {
        return None;
    }
    let rollout_route = first_string(&[
        cispo.and_then(|value| value.get("rollout_url")),
        cispo.and_then(|value| value.get("rollout_route")),
        contracts.and_then(|value| value.get("rollout_route")),
        contracts.and_then(|value| value.get("rollout_url")),
    ])?;
    let reward_route = first_string(&[
        cispo.and_then(|value| value.get("reward_url")),
        cispo.and_then(|value| value.get("reward_route")),
        contracts.and_then(|value| value.get("reward_route")),
    ]);
    let implementation = first_string(&[
        cispo.and_then(|value| value.get("contract")),
        cispo.and_then(|value| value.get("implementation")),
        capabilities.and_then(|value| value.get("version")),
        contracts.and_then(|value| value.get("version")),
    ])
    .unwrap_or_else(|| "cispo.v1".into());
    let harness = first_string(&[cispo.and_then(|value| value.get("harness"))])
        .unwrap_or_else(|| harness_from_contract(&implementation));
    let plan_ref = first_string(&[cispo.and_then(|value| value.get("plan_ref"))])
        .unwrap_or_else(|| format!("{task_id}_eval.v1"));
    let train_world_ref = first_string(&[cispo.and_then(|value| value.get("train_world_ref"))])
        .unwrap_or_else(|| format!("world:{task_id}@train"));
    let heldout_world_ref = first_string(&[
        cispo.and_then(|value| value.get("heldout_world_ref")),
        cispo.and_then(|value| value.get("eval_world_ref")),
    ])
    .unwrap_or_else(|| format!("world:{task_id}@heldout"));
    Some(CispoContract {
        rollout_url: join_route(base_url, &rollout_route),
        reward_url: reward_route.map(|route| join_route(base_url, &route)),
        implementation,
        harness,
        plan_ref,
        train_world_ref,
        heldout_world_ref,
        token: first_string(&[cispo.and_then(|value| value.get("token"))]),
    })
}

fn parse_sft(
    base_url: &str,
    manifest: Option<&Value>,
    info: Option<&Value>,
) -> Option<SftContract> {
    let sft = manifest.and_then(|value| value.get("sft"));
    let contracts = info.and_then(|value| value.pointer("/metadata/optimizer_contracts/sft"));
    let train = first_string(&[
        sft.and_then(|value| value.pointer("/train/route")),
        sft.and_then(|value| value.get("train_jsonl_url")),
        sft.and_then(|value| value.get("train_jsonl_route")),
        contracts.and_then(|value| value.get("train_jsonl_route")),
    ])?;
    let eval = first_string(&[
        sft.and_then(|value| value.pointer("/evaluation/route")),
        sft.and_then(|value| value.get("eval_jsonl_url")),
        sft.and_then(|value| value.get("eval_jsonl_route")),
        contracts.and_then(|value| value.get("eval_jsonl_route")),
    ])?;
    Some(SftContract {
        train_jsonl_url: join_route(base_url, &train),
        eval_jsonl_url: join_route(base_url, &eval),
    })
}

pub fn harness_from_contract(contract: &str) -> String {
    let normalized = contract.to_ascii_lowercase();
    if normalized.contains("classif") {
        "classify".into()
    } else if normalized.contains("text-trajectory") || normalized.contains("text_trajectory") {
        "text_trajectory".into()
    } else {
        "rollout".into()
    }
}

pub fn join_route(base: &str, route: &str) -> String {
    let route = route.trim();
    if route.starts_with("http://") || route.starts_with("https://") {
        return route.trim_end_matches('/').to_string();
    }
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        route.trim_start_matches('/')
    )
}

fn first_string(candidates: &[Option<&Value>]) -> Option<String> {
    candidates.iter().find_map(|value| {
        value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

pub async fn materialize_jsonl(url: &str, destination: PathBuf) -> Result<PathBuf> {
    let client = reqwest::Client::builder()
        .timeout(SFT_MATERIALIZATION_TIMEOUT)
        .build()
        .context("SFT JSONL HTTP client")?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !response.status().is_success() {
        bail!("GET {url} returned {}", response.status());
    }
    let bytes = response.bytes().await.context("read SFT JSONL")?;
    if bytes.is_empty() {
        bail!("SFT JSONL at {url} is empty");
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(&destination, &bytes)
        .with_context(|| format!("write {}", destination.display()))?;
    Ok(destination)
}

pub async fn materialize_sft_jsonl(bind: &ContainerTrainingBind) -> Result<(PathBuf, PathBuf)> {
    let sft = bind.sft.as_ref().ok_or_else(|| {
        anyhow!(
            "container `{}` does not advertise SFT JSONL routes",
            bind.container_id
        )
    })?;
    let root = crate::instance::data_root()
        .join("optimizers/datasets")
        .join(&bind.container_id);
    let train = materialize_jsonl(&sft.train_jsonl_url, root.join("train.jsonl")).await?;
    let eval = materialize_jsonl(&sft.eval_jsonl_url, root.join("eval.jsonl")).await?;
    Ok((train, eval))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest_cispo_and_sft_without_host_task_names() {
        let container = ReadyTrainingContainer {
            id: "ctr_1".into(),
            base_url: "http://127.0.0.1:9".into(),
        };
        let manifest = json!({
            "schema_version": "workshop.training.v1",
            "task": "household.text.v1",
            "sft": {
                "train": {"route": "/sft/train.jsonl"},
                "evaluation": {"route": "/sft/eval.jsonl"}
            },
            "cispo": {
                "contract": "cispo.text-trajectory.v1",
                "rollout_route": "/rollout",
                "reward_route": "/reward",
                "train_world_ref": "world:household@train",
                "heldout_world_ref": "world:household@test"
            }
        });
        let bind = parse_bind(&container, Some(&manifest), None).unwrap();
        assert_eq!(bind.task_id, "household.text.v1");
        let cispo = bind.cispo.unwrap();
        assert_eq!(cispo.rollout_url, "http://127.0.0.1:9/rollout");
        assert_eq!(
            cispo.reward_url.as_deref(),
            Some("http://127.0.0.1:9/reward")
        );
        assert_eq!(cispo.implementation, "cispo.text-trajectory.v1");
        assert_eq!(cispo.harness, "text_trajectory");
        assert_eq!(cispo.train_world_ref, "world:household@train");
        assert_eq!(cispo.heldout_world_ref, "world:household@test");
        let sft = bind.sft.unwrap();
        assert_eq!(sft.train_jsonl_url, "http://127.0.0.1:9/sft/train.jsonl");
        assert_eq!(sft.eval_jsonl_url, "http://127.0.0.1:9/sft/eval.jsonl");
    }

    #[test]
    fn falls_back_to_info_optimizer_contracts() {
        let container = ReadyTrainingContainer {
            id: "ctr_2".into(),
            base_url: "http://127.0.0.1:9".into(),
        };
        let info = json!({
            "runtime": {"runtime_id": "classify.v1"},
            "capabilities": {"cispo": {"version": "cispo.classify.v1"}},
            "metadata": {
                "optimizer_contracts": {
                    "cispo": {"rollout_route": "/rollout", "reward_route": "/reward"},
                    "sft": {
                        "train_jsonl_route": "/sft/train.jsonl",
                        "eval_jsonl_route": "/sft/eval.jsonl"
                    }
                }
            }
        });
        let bind = parse_bind(&container, None, Some(&info)).unwrap();
        assert_eq!(bind.task_id, "classify.v1");
        let cispo = bind.cispo.unwrap();
        assert_eq!(cispo.harness, "classify");
        assert_eq!(cispo.train_world_ref, "world:classify.v1@train");
        assert!(bind.sft.is_some());
    }

    #[test]
    fn refuses_a_container_with_no_training_contract() {
        let container = ReadyTrainingContainer {
            id: "ctr_3".into(),
            base_url: "http://127.0.0.1:9".into(),
        };
        let err = parse_bind(&container, Some(&json!({"task": "only-eval.v1"})), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no SFT or CISPO contract"), "{err}");
    }

    #[test]
    fn join_route_keeps_absolute_urls() {
        assert_eq!(
            join_route("http://127.0.0.1:9", "https://example.test/rollout"),
            "https://example.test/rollout"
        );
        assert_eq!(
            join_route("http://127.0.0.1:9/", "/rollout"),
            "http://127.0.0.1:9/rollout"
        );
    }
}
