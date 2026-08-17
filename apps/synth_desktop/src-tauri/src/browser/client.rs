use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const CALL_TIMEOUT: Duration = Duration::from_secs(120);

pub struct BrowserClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl BrowserClient {
    pub async fn spawn() -> Result<Self> {
        let script = super::backend_script_path();
        if !script.is_file() {
            bail!("Playwright backend is missing at {}", script.display());
        }
        let mut command = Command::new(super::browser_node_path());
        command
            .arg(&script)
            .env("SYNTH_BROWSER_POLICY_FILE", super::policy_path())
            .env("SYNTH_BROWSER_PROFILE_ROOT", super::profile_root())
            .env("SYNTH_BROWSER_REQUIRE_HOST_APPROVAL", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if let Some(root) = super::bundled_runtime_root() {
            command
                .env("SYNTH_BROWSER_RUNTIME_ROOT", &root)
                .env("PLAYWRIGHT_BROWSERS_PATH", root.join("browsers"));
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("start browser backend at {}", script.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("browser stdin missing"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("browser stdout missing"))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
        })
    }

    pub async fn call(&mut self, operation: &str, arguments: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let mut line = serde_json::to_string(&json!({
            "id": id,
            "operation": operation,
            "arguments": arguments,
        }))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .context("write browser request")?;
        self.stdin.flush().await.context("flush browser request")?;
        let deadline = tokio::time::Instant::now() + CALL_TIMEOUT;
        loop {
            let mut buffer = String::new();
            let read = tokio::time::timeout_at(deadline, self.stdout.read_line(&mut buffer))
                .await
                .map_err(|_| anyhow!("browser backend timed out answering {operation}"))??;
            if read == 0 {
                bail!("browser backend crashed while answering {operation}");
            }
            let Ok(message) = serde_json::from_str::<Value>(buffer.trim()) else {
                continue;
            };
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if message.get("ok").and_then(Value::as_bool) == Some(true) {
                return Ok(message
                    .get("response")
                    .cloned()
                    .unwrap_or_else(|| json!({})));
            }
            bail!(
                "{}",
                message
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("browser backend failed")
            );
        }
    }

    pub async fn stop(mut self) {
        let _ = self.stdin.shutdown().await;
        // Closing stdin asks the backend to close every managed Chromium
        // context before exiting. An immediate kill leaves ProcessSingleton
        // locks and orphaned browser processes behind, corrupting restart.
        if tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }
}
