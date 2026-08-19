//! Synth Cloud hosted optimizer client (GEPA OSS + hosted GELO / SFT).
//!
//! SFT runs through the public Optimizers service; its beta executor remains internal. Workshop mirrors
//! incremental `optimizer_event.v1` pages into the local OptimizerService.

use super::models::{
    HostedTrainingModelCatalog, OptimizerRunOutputs, SavedLoraCheckpoint, SavedLoraCheckpointPage,
    SavedLoraCheckpointQuery, SavedLoraDownload, SavedLoraRunPage,
};
use super::training::TrainingEvent;
use anyhow::{anyhow, bail, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct CloudOptimizerClient {
    client: Client,
    base_url: String,
    api_key: String,
}

impl CloudOptimizerClient {
    pub fn from_config() -> Result<Self> {
        let backend = crate::synth_config::resolve().context("resolve Synth backend")?;
        let api_key = backend
            .api_key
            .ok_or_else(|| anyhow!("Synth API key is not configured"))?;
        Ok(Self::new(backend.backend_url, api_key))
    }

    pub fn new(backend_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: crate::http::http_client_with_timeout(crate::limits::OPTIMIZERS_CLOUD_TIMEOUT),
            base_url: backend_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    pub async fn list_runs(
        &self,
        algorithm: Option<&str>,
        status: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        let mut url = format!("{}/api/v1/optimizers/runs", self.base_url);
        let mut query = Vec::new();
        if let Some(algorithm) = algorithm {
            query.push(format!("algorithm={}", urlencoding_lite(algorithm)));
        }
        if let Some(status) = status {
            query.push(format!("status={}", urlencoding_lite(status)));
        }
        if let Some(limit) = limit {
            query.push(format!("limit={limit}"));
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }
        let payload = self.get_json(&url).await?;
        if let Some(arr) = payload.as_array() {
            return Ok(arr.clone());
        }
        if let Some(arr) = payload.get("runs").and_then(Value::as_array) {
            return Ok(arr.clone());
        }
        Ok(vec![payload])
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Value> {
        let url = format!("{}/api/v1/optimizers/runs/{}", self.base_url, run_id);
        self.get_json(&url).await
    }

    pub async fn create_run(
        &self,
        algorithm: &str,
        config: Value,
        project_id: Option<&str>,
        run_id: Option<&str>,
    ) -> Result<Value> {
        let algorithm = match algorithm {
            "gelo" | "goex" | "go_ex" => "go-ex",
            other => other,
        };
        let mut body = json!({
            "algorithm": algorithm,
        });
        let obj = body.as_object_mut().unwrap();
        if let Some(run_id) = run_id {
            obj.insert("run_id".into(), json!(run_id));
            obj.insert("idempotency_key".into(), json!(run_id));
        }
        if let Some(project_id) = project_id {
            obj.insert("project_id".into(), json!(project_id));
        }
        if let Some(toml) = config.get("config_toml").and_then(Value::as_str) {
            obj.insert("config_toml".into(), json!(toml));
        } else if let Some(config_json) = config.get("config_json") {
            obj.insert("config_json".into(), config_json.clone());
        } else if config.is_object() {
            obj.insert("config_json".into(), config);
        } else {
            bail!("cloud create requires config_toml or config_json");
        }
        let url = format!("{}/api/v1/optimizers/runs", self.base_url);
        self.post_json(&url, body).await
    }

    pub async fn cancel_run(&self, run_id: &str) -> Result<Value> {
        let url = format!("{}/api/v1/optimizers/runs/{}/cancel", self.base_url, run_id);
        self.post_json(&url, json!({})).await
    }

    /// Bounded NDJSON backfill from the canonical hosted event log.
    pub async fn events_after(
        &self,
        run_id: &str,
        after_seq: u64,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        let limit = limit.unwrap_or(500).clamp(1, 5000);
        let url = format!(
            "{}/api/v1/optimizers/runs/{}/events?after_seq={after_seq}&limit={limit}&stream=false",
            self.base_url, run_id
        );
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Accept", "application/x-ndjson, application/json")
            .send()
            .await
            .context("cloud optimizer events request")?;
        let status = response.status();
        let text = response.text().await.context("read cloud events body")?;
        if !status.is_success() {
            bail!("cloud optimizer events failed ({status}): {text}");
        }
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            if let Some(arr) = value.as_array() {
                return Ok(arr.clone());
            }
            if let Some(arr) = value.get("events").and_then(Value::as_array) {
                return Ok(arr.clone());
            }
            return Ok(vec![value]);
        }
        let mut events = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            events.push(serde_json::from_str(line).context("parse cloud event ndjson")?);
        }
        Ok(events)
    }

    /// Durable canonical training-event replay. Live reconnect uses the same
    /// sequence as SSE `Last-Event-ID`, so polling and streaming cannot fork.
    pub async fn training_events_after(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: Option<i64>,
    ) -> Result<Vec<TrainingEvent>> {
        let limit = limit.unwrap_or(500).clamp(1, 5_000);
        let url = format!(
            "{}/api/v1/optimizers/runs/{}/training-events?after_sequence={after_sequence}&limit={limit}",
            self.base_url, run_id
        );
        let payload = self.get_json(&url).await?;
        let rows = payload
            .get("events")
            .and_then(Value::as_array)
            .or_else(|| payload.as_array())
            .ok_or_else(|| anyhow!("cloud training event replay is not an event array"))?;
        rows.iter()
            .map(|row| {
                let event: TrainingEvent =
                    serde_json::from_value(row.clone()).context("decode training.event.v1")?;
                event.validate().map_err(anyhow::Error::msg)?;
                Ok(event)
            })
            .collect()
    }

    pub async fn get_state_batch(&self, run_id: &str, slices: &[String]) -> Result<Value> {
        let joined = slices.join(",");
        let url = format!(
            "{}/api/v1/optimizers/runs/{}/state/batch?slices={}",
            self.base_url,
            run_id,
            urlencoding_lite(&joined)
        );
        self.get_json(&url).await
    }

    pub async fn search_saved_lora_checkpoints(
        &self,
        query: SavedLoraCheckpointQuery,
    ) -> Result<SavedLoraCheckpointPage> {
        let mut params = vec![format!(
            "scope={}",
            urlencoding_lite(query.scope.as_deref().unwrap_or("all"))
        )];
        for (name, value) in [
            ("q", query.search.as_deref()),
            ("provider", query.provider.as_deref()),
            ("checkpoint_kind", query.checkpoint_kind.as_deref()),
            ("base_model", query.base_model.as_deref()),
            ("run_id", query.run_id.as_deref()),
            ("attempt_id", query.attempt_id.as_deref()),
            (
                "source_checkpoint_id",
                query.source_checkpoint_id.as_deref(),
            ),
            ("optimizer_algorithm", query.optimizer_algorithm.as_deref()),
            ("status", query.status.as_deref()),
        ] {
            if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
                params.push(format!("{name}={}", urlencoding_lite(value.trim())));
            }
        }
        for tag in query.tags.unwrap_or_default().into_iter().take(32) {
            if !tag.trim().is_empty() {
                params.push(format!("tags={}", urlencoding_lite(tag.trim())));
            }
        }
        params.push(format!("limit={}", query.limit.unwrap_or(50).clamp(1, 100)));
        params.push(format!("offset={}", query.offset.unwrap_or(0)));
        let url = format!(
            "{}/api/v1/optimizers/checkpoints?{}",
            self.base_url,
            params.join("&")
        );
        let payload = self.get_json(&url).await?;
        serde_json::from_value(payload).context("decode saved LoRA checkpoint page")
    }

    pub async fn saved_lora_checkpoints_for_run(&self, run_id: &str) -> Result<SavedLoraRunPage> {
        let url = format!(
            "{}/api/v1/optimizers/runs/{}/saved-checkpoints?status=ready&limit=100",
            self.base_url,
            urlencoding_lite(run_id)
        );
        let payload = self.get_json(&url).await?;
        serde_json::from_value(payload).context("decode run saved LoRA checkpoint page")
    }

    pub async fn run_outputs(&self, run_id: &str) -> Result<OptimizerRunOutputs> {
        let url = format!(
            "{}/api/v1/optimizers/runs/{}/outputs",
            self.base_url,
            urlencoding_lite(run_id)
        );
        let payload = self.get_json(&url).await?;
        serde_json::from_value(payload).context("decode optimizer run outputs")
    }

    pub async fn hosted_training_models(&self) -> Result<HostedTrainingModelCatalog> {
        let url = format!("{}/api/v1/optimizers/models/training", self.base_url);
        let payload = self.get_json(&url).await?;
        serde_json::from_value(payload).context("decode hosted training model catalog")
    }

    pub async fn archive_saved_lora_checkpoint(
        &self,
        checkpoint_id: &str,
    ) -> Result<SavedLoraCheckpoint> {
        let url = format!(
            "{}/api/v1/optimizers/checkpoints/{}",
            self.base_url,
            urlencoding_lite(checkpoint_id)
        );
        let payload = self.delete_json(&url).await?;
        serde_json::from_value(payload).context("decode archived saved LoRA checkpoint")
    }

    pub async fn saved_lora_download(&self, checkpoint_id: &str) -> Result<SavedLoraDownload> {
        let url = format!(
            "{}/api/v1/optimizers/checkpoints/{}/download",
            self.base_url,
            urlencoding_lite(checkpoint_id)
        );
        let payload = self.get_json(&url).await?;
        serde_json::from_value(payload).context("decode saved LoRA download")
    }

    async fn get_json(&self, url: &str) -> Result<Value> {
        let response = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        let status = response.status();
        let text = response.text().await.context("read response body")?;
        if !status.is_success() {
            bail!("cloud optimizer GET failed ({status}): {text}");
        }
        serde_json::from_str(&text).context("decode cloud optimizer JSON")
    }

    async fn post_json(&self, url: &str, body: Value) -> Result<Value> {
        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = response.status();
        let text = response.text().await.context("read response body")?;
        if !status.is_success() {
            bail!("cloud optimizer POST failed ({status}): {text}");
        }
        if text.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&text).context("decode cloud optimizer JSON")
    }

    async fn delete_json(&self, url: &str) -> Result<Value> {
        let response = self
            .client
            .delete(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("DELETE {url}"))?;
        let status = response.status();
        let text = response.text().await.context("read response body")?;
        if !status.is_success() {
            bail!("cloud optimizer DELETE failed ({status}): {text}");
        }
        serde_json::from_str(&text).context("decode cloud optimizer JSON")
    }
}

fn urlencoding_lite(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | ',' => out.push(ch),
            _ => out.push_str(&format!("%{:02X}", ch as u8)),
        }
    }
    out
}
