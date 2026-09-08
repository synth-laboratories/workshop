use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{fs, net::SocketAddr, path::PathBuf, time::Duration};

#[derive(Deserialize)]
struct Connection {
    url: String,
    token: String,
}

pub struct RuntimeClient {
    root: PathBuf,
    runtime: tokio::runtime::Runtime,
    client: reqwest::Client,
}

impl RuntimeClient {
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(Self {
            root,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(120))
                .build()?,
        })
    }

    pub fn describe(&self) -> Result<Value> {
        let value = self.request("/v1/workshop/describe", json!({}))?;
        anyhow::ensure!(
            value["contractVersion"] == 1,
            "unsupported Workshop runtime contract; update the selected instance"
        );
        anyhow::ensure!(
            value["dataRoot"].as_str() == self.root.to_str(),
            "runtime instance identity mismatch"
        );
        let tools = value["tools"]
            .as_array()
            .context("runtime did not return a tool catalogue")?;
        anyhow::ensure!(
            !tools.is_empty(),
            "runtime has no registered external operations"
        );
        Ok(value)
    }

    /// Retry suppression belongs to one descriptor generation. A stopped
    /// runtime must not poison this client's tools after a subsequent restart.
    /// This reads file metadata only; no bearer material enters the key.
    pub fn breaker_arguments(&self, arguments: &Value) -> Value {
        let epoch = fs::symlink_metadata(self.root.join("visuals-ipc.json"))
            .ok()
            .filter(|meta| meta.file_type().is_file())
            .and_then(|meta| {
                meta.modified()
                    .ok()
                    .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|modified| format!("{}:{}", modified.as_nanos(), meta.len()))
            })
            .unwrap_or_else(|| "runtime-unavailable".into());
        let mut scoped = arguments.clone();
        if let Some(object) = scoped.as_object_mut() {
            let revision = object
                .get("capability_revision")
                .cloned()
                .unwrap_or(Value::Null);
            object.insert(
                "capability_revision".into(),
                json!(format!("{epoch}:{revision}")),
            );
        }
        scoped
    }

    pub fn call(&self, name: &str, arguments: &Value) -> Result<Value> {
        self.request(
            "/v1/workshop/call",
            json!({"name":name,"arguments":arguments}),
        )
    }

    fn request(&self, route: &str, mut body: Value) -> Result<Value> {
        body["data_root"] = json!(self.root);
        body["contract_version"] = json!(1);
        self.request_route("POST", route, body)
    }

    /// Shared compatibility transport, reachable only through registered tools.
    pub fn request_route(&self, method: &str, route: &str, body: Value) -> Result<Value> {
        anyhow::ensure!(
            route.starts_with("/v1/")
                && !route.chars().any(char::is_control)
                && !route.contains('\\')
                && !route.contains('%')
                && !route.split('/').any(|part| part == "." || part == ".."),
            "invalid Workshop route"
        );
        let method = reqwest::Method::from_bytes(method.as_bytes())?;
        // Read on every call: the running instance rotates its token on restart.
        // A missing descriptor is not permission to attach to another instance.
        let path = self.root.join("visuals-ipc.json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let metadata = fs::symlink_metadata(&path)
                .context("selected Workshop instance is not running; start it and retry")?;
            anyhow::ensure!(
                metadata.file_type().is_file()
                    && metadata.permissions().mode() & 0o077 == 0
                    && metadata.uid() == unsafe { libc::geteuid() },
                "IPC descriptor must be a private file owned by the current user"
            );
        }
        let raw = fs::read(&path)
            .context("selected Workshop instance is not running; start it and retry")?;
        let connection: Connection = serde_json::from_slice(&raw)
            .map_err(|_| anyhow::anyhow!("invalid Workshop IPC descriptor"))?;
        let authority = connection
            .url
            .strip_prefix("http://")
            .context("Workshop IPC must use local HTTP")?;
        let address: SocketAddr = authority
            .parse()
            .context("Workshop IPC requires a literal loopback address and port")?;
        anyhow::ensure!(
            address.ip().is_loopback() && address.port() != 0,
            "Workshop IPC must remain on loopback"
        );
        self.runtime.block_on(async {
            let response = self
                .client
                .request(method, format!("{}{route}", connection.url))
                .bearer_auth(connection.token)
                .json(&body)
                .timeout(Duration::from_secs(if route == "/v1/workshop/describe" {
                    5
                } else {
                    120
                }))
                .send()
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "cannot reach the selected Workshop runtime; check that it is running"
                    )
                })?;
            let status = response.status();
            let bytes = response
                .bytes()
                .await
                .context("incomplete Workshop response")?;
            let result: Value = serde_json::from_slice(&bytes)
                .map_err(|_| anyhow::anyhow!("invalid Workshop runtime response"))?;
            if !status.is_success() {
                if result.get("code").and_then(Value::as_str).is_some() {
                    anyhow::bail!("{}", result);
                }
                anyhow::bail!(
                    "Workshop request failed ({status}): {}",
                    result
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("runtime rejected the request")
                );
            }
            Ok(result)
        })
    }
}
