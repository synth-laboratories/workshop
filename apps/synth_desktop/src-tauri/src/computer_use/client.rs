//! Desktop's side of the helper channel: spawn the verified bundle and speak
//! MCP to it over its own stdio.
//!
//! Stdio is the isolation. The pipe exists only between parent and child, so
//! there is no port for another process to connect to and no socket to guess
//! the path of — which is why this is a pipe and not the `/tmp` socket the
//! reference implementation uses. The helper independently verifies that its
//! parent is us, so possession of the pipe is not the only credential.
//!
//! Calls are serialized by `&mut self`. A computer-use session is inherently
//! sequential — you cannot meaningfully click two things at once — so request
//! correlation is one outstanding request, not a map of pending ids.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// A cold helper may need to launch a target app before it can answer.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// The handshake talks to an already-running process and should be immediate.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Passed to the helper so it can confirm the pipe it is serving belongs to the
/// launch Desktop intended, and reject a stdio pair handed to it by anything
/// else.
pub const LAUNCH_NONCE_ENV: &str = "SYNTH_COMPUTER_USE_LAUNCH_NONCE";
/// Development and explicitly unnotarized friends builds do not have an Apple
/// Team ID. In that lane Desktop passes its exact designated requirement so
/// the helper still authenticates this one immutable parent build.
pub const PARENT_REQUIREMENT_ENV: &str = "SYNTH_COMPUTER_USE_PARENT_REQUIREMENT";

pub struct HelperClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    /// Reported by the helper at handshake; recorded on every trajectory step.
    server_version: String,
}

impl HelperClient {
    /// Launch the helper. The caller must have verified the bundle first —
    /// this deliberately takes an executable path and does no verification of
    /// its own, so there is exactly one place that decides a bundle is
    /// trustworthy and it is never accidentally bypassed by a second entry
    /// point.
    pub async fn spawn(
        executable: &Path,
        launch_nonce: &str,
        parent_requirement: Option<&str>,
    ) -> Result<Self> {
        let mut command = Command::new(executable);
        command.arg("mcp").env(LAUNCH_NONCE_ENV, launch_nonce);
        if let Some(requirement) = parent_requirement {
            command.env(PARENT_REQUIREMENT_ENV, requirement);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited, so helper diagnostics land in Desktop's log rather
            // than filling a pipe nobody drains and deadlocking the child.
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawn computer-use helper at {}", executable.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("helper stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("helper stdout was not piped"))?;
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            server_version: String::new(),
        };
        client.initialize().await?;
        Ok(client)
    }

    pub fn server_version(&self) -> &str {
        &self.server_version
    }

    async fn initialize(&mut self) -> Result<()> {
        let response = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "synth-desktop", "version": env!("CARGO_PKG_VERSION") }
                }),
                HANDSHAKE_TIMEOUT,
            )
            .await
            .context("computer-use helper handshake failed")?;
        self.server_version = response
            .pointer("/serverInfo/version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        Ok(())
    }

    /// Invoke one helper tool.
    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
        let response = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
                CALL_TIMEOUT,
            )
            .await?;
        // MCP reports application failures inside a successful response, so a
        // transport-level success is not a successful action.
        if response
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let detail = response
                .pointer("/content/0/text")
                .and_then(Value::as_str)
                .unwrap_or("helper reported an error with no detail");
            bail!("{detail}");
        }
        Ok(response
            .get("structuredContent")
            .cloned()
            .unwrap_or(response))
    }

    pub async fn list_tools(&mut self) -> Result<Vec<String>> {
        let response = self
            .request("tools/list", json!({}), HANDSHAKE_TIMEOUT)
            .await?;
        Ok(response
            .get("tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let mut line = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;
        line.push('\n');
        // A dead child surfaces as EPIPE on write or as EOF on read, depending
        // on timing. Both mean the same thing to the caller, so both say so.
        if self.stdin.write_all(line.as_bytes()).await.is_err() || self.stdin.flush().await.is_err()
        {
            bail!("computer-use helper stopped before `{method}` could be sent");
        }

        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let mut buffer = String::new();
            let read = tokio::time::timeout_at(deadline, self.stdout.read_line(&mut buffer))
                .await
                .map_err(|_| anyhow!("computer-use helper did not answer `{method}` in time"))?
                .context("read from computer-use helper")?;
            if read == 0 {
                bail!("computer-use helper stopped before answering `{method}`");
            }
            let trimmed = buffer.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
                // The helper's stdout is a protocol stream. Anything unparseable
                // is noise from a library writing where it should not; skipping
                // it is better than tearing down a working session.
                continue;
            };
            // Notifications carry no id and are not answers to this request.
            match message.get("id").and_then(Value::as_u64) {
                Some(answered) if answered == id => {
                    if let Some(error) = message.get("error") {
                        let detail = error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown helper error");
                        bail!("computer-use helper refused `{method}`: {detail}");
                    }
                    return Ok(message.get("result").cloned().unwrap_or(json!({})));
                }
                _ => continue,
            }
        }
    }

    /// Stop the helper. Best effort: the child is also killed on drop, so a
    /// panic between here and there does not leak a process holding TCC grants.
    pub async fn shutdown(mut self) {
        let _ = self.stdin.shutdown().await;
        let _ = self.child.kill().await;
    }
}

