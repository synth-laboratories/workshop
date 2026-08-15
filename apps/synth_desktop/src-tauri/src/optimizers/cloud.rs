//! Synth Cloud hosted optimizer client (GEPA OSS + hosted GELO / SFT).
//!
//! SFT runs through the public Optimizers service; its beta executor remains internal. Workshop mirrors
//! incremental `optimizer_event.v1` pages into the local OptimizerService.

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
