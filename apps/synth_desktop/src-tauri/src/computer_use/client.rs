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
    pub async fn spawn(executable: &Path, launch_nonce: &str) -> Result<Self> {
        let mut child = Command::new(executable)
            .arg("mcp")
            .env(LAUNCH_NONCE_ENV, launch_nonce)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    /// A stand-in helper: a shell script speaking just enough MCP to exercise
    /// the transport. Testing the real helper needs a signed bundle and a TCC
    /// grant; testing the framing does not, and framing is where the bugs are.
    fn fake_helper(dir: &Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("fake-helper.sh");
        let mut file = std::fs::File::create(&path).unwrap();
        write!(file, "#!/bin/sh\n{body}\n").unwrap();
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    /// Answers every request with a canned result keyed on the request id it
    /// read, and emits an unsolicited notification first.
    const ECHO_SERVER: &str = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  method=$(printf '%s' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
  printf '{"jsonrpc":"2.0","method":"notifications/progress","params":{}}\n'
  case "$method" in
    initialize) printf '{"jsonrpc":"2.0","id":%s,"result":{"serverInfo":{"name":"fake","version":"9.9.9"}}}\n' "$id" ;;
    tools/list) printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"computer_use"}]}}\n' "$id" ;;
    tools/call) printf '{"jsonrpc":"2.0","id":%s,"result":{"structuredContent":{"ok":true}}}\n' "$id" ;;
    *) printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"no such method"}}\n' "$id" ;;
  esac
done
"#;

    #[tokio::test]
    async fn the_handshake_records_the_helper_version() {
        let dir = tempdir().unwrap();
        let client = HelperClient::spawn(&fake_helper(dir.path(), ECHO_SERVER), "nonce")
            .await
            .unwrap();
        assert_eq!(client.server_version(), "9.9.9");
        client.shutdown().await;
    }

    /// Notifications and stray output must not be mistaken for the answer to
    /// the request in flight.
    #[tokio::test]
    async fn unsolicited_messages_are_skipped_while_waiting_for_an_answer() {
        let dir = tempdir().unwrap();
        let mut client = HelperClient::spawn(&fake_helper(dir.path(), ECHO_SERVER), "nonce")
            .await
            .unwrap();
        assert_eq!(client.list_tools().await.unwrap(), vec!["computer_use"]);
        let result = client.call_tool("computer_use", json!({})).await.unwrap();
        assert_eq!(result["ok"], true);
        client.shutdown().await;
    }

    #[tokio::test]
    async fn a_helper_that_reports_an_error_result_is_a_failed_call() {
        let dir = tempdir().unwrap();
        let server = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  method=$(printf '%s' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
  case "$method" in
    initialize) printf '{"jsonrpc":"2.0","id":%s,"result":{"serverInfo":{"version":"1"}}}\n' "$id" ;;
    *) printf '{"jsonrpc":"2.0","id":%s,"result":{"isError":true,"content":[{"type":"text","text":"Accessibility is not granted"}]}}\n' "$id" ;;
  esac
done
"#;
        let mut client = HelperClient::spawn(&fake_helper(dir.path(), server), "nonce")
            .await
            .unwrap();
        let error = client
            .call_tool("computer_use", json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("Accessibility is not granted"), "{error}");
        client.shutdown().await;
    }

    #[tokio::test]
    async fn a_jsonrpc_error_names_the_method_that_was_refused() {
        let dir = tempdir().unwrap();
        let server = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  method=$(printf '%s' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
  case "$method" in
    initialize) printf '{"jsonrpc":"2.0","id":%s,"result":{"serverInfo":{"version":"1"}}}\n' "$id" ;;
    *) printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"caller is not authorized"}}\n' "$id" ;;
  esac
done
"#;
        let mut client = HelperClient::spawn(&fake_helper(dir.path(), server), "nonce")
            .await
            .unwrap();
        let error = client
            .call_tool("computer_use", json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("tools/call"), "{error}");
        assert!(error.contains("caller is not authorized"), "{error}");
        client.shutdown().await;
    }

    /// A helper that dies mid-call must surface as a failed call rather than
    /// hanging until the timeout.
    #[tokio::test]
    async fn a_helper_that_exits_is_reported_immediately() {
        let dir = tempdir().unwrap();
        let server = r#"
IFS= read -r line
id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
printf '{"jsonrpc":"2.0","id":%s,"result":{"serverInfo":{"version":"1"}}}\n' "$id"
exit 0
"#;
        let mut client = HelperClient::spawn(&fake_helper(dir.path(), server), "nonce")
            .await
            .unwrap();
        let error = client
            .call_tool("computer_use", json!({}))
            .await
            .unwrap_err()
            .to_string();
        // Whether this lands on the write or the read is a race; the caller is
        // told the same thing either way.
        assert!(error.contains("helper stopped"), "{error}");
    }

    #[tokio::test]
    async fn spawning_a_helper_that_is_not_there_fails_with_its_path() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nope");
        let error = match HelperClient::spawn(&missing, "nonce").await {
            Ok(_) => panic!("spawning a helper that is not installed must fail"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("nope"), "{error}");
    }
}
