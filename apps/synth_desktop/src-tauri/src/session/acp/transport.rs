//! ACP v1 NDJSON transport. Requests and peer callbacks are independent lanes;
//! permission requests must not block cancellation or the stdout reader.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{mpsc, oneshot, Mutex},
};

const MAX_FRAME: usize = 1024 * 1024;
type Pending = HashMap<u64, oneshot::Sender<Result<Value>>>;

pub struct Inbound {
    pub message: Value,
    done: Option<oneshot::Sender<()>>,
}
impl Drop for Inbound {
    fn drop(&mut self) {
        if let Some(done) = self.done.take() {
            let _ = done.send(());
        }
    }
}

pub struct Peer {
    child: Mutex<Child>,
    input: Mutex<ChildStdin>,
    pending: Arc<Mutex<Pending>>,
    sequence: AtomicU64,
}

impl Peer {
    pub fn spawn(mut command: Command) -> Result<(Arc<Self>, mpsc::Receiver<Inbound>)> {
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().context("start configured ACP backend")?;
        let input = child.stdin.take().context("ACP stdin unavailable")?;
        let output = child.stdout.take().context("ACP stdout unavailable")?;
        let pending: Arc<Mutex<Pending>> = Arc::new(Mutex::new(HashMap::new()));
        let (events, receive) = mpsc::channel(128);
        let requests = pending.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(output);
            loop {
                let mut line = Vec::new();
                let read = (&mut reader)
                    .take(MAX_FRAME as u64 + 1)
                    .read_until(b'\n', &mut line)
                    .await;
                if !matches!(read, Ok(size) if size > 0 && size <= MAX_FRAME) {
                    break;
                }
                let Ok(message) = serde_json::from_slice::<Value>(&line) else {
                    break;
                };
                if message["jsonrpc"] != "2.0" {
                    break;
                }
                if message.get("method").is_some() {
                    let (done, processed) = oneshot::channel();
                    if events
                        .send(Inbound {
                            message,
                            done: Some(done),
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                    // A following prompt response cannot overtake a preceding
                    // streamed update's durable journal append. Permission
                    // handlers acknowledge before awaiting a human decision.
                    if processed.await.is_err() {
                        break;
                    }
                } else if let Some(id) = message["id"].as_u64() {
                    if let Some(reply) = requests.lock().await.remove(&id) {
                        let outcome = if let Some(error) = message.get("error") {
                            Err(anyhow::anyhow!("ACP request rejected: {}", error))
                        } else if let Some(result) = message.get("result") {
                            Ok(result.clone())
                        } else {
                            Err(anyhow::anyhow!("invalid ACP response"))
                        };
                        let _ = reply.send(outcome);
                    }
                }
            }
            for (_, reply) in requests.lock().await.drain() {
                let _ = reply.send(Err(anyhow::anyhow!(
                    "ACP connection closed; request outcome may be uncertain"
                )));
            }
        });
        Ok((
            Arc::new(Self {
                child: Mutex::new(child),
                input: Mutex::new(input),
                pending,
                sequence: AtomicU64::new(1),
            }),
            receive,
        ))
    }

    pub async fn process_id(&self) -> Option<u32> {
        self.child.lock().await.id()
    }

    pub async fn write(&self, message: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&message)?;
        anyhow::ensure!(bytes.len() < MAX_FRAME, "ACP message exceeds 1 MiB");
        bytes.push(b'\n');
        let mut input = self.input.lock().await;
        tokio::time::timeout(Duration::from_secs(5), async {
            input.write_all(&bytes).await?;
            input.flush().await
        })
        .await
        .context("ACP stdin stopped accepting frames")??;
        Ok(())
    }

    /// Write a request and hand back a handle for its reply.
    ///
    /// The frame is on the wire when this returns. A caller that must order a
    /// later notification after this request — a cancellation, which is only
    /// meaningful once the peer has the prompt — can await this before
    /// releasing whatever lock serialises the two.
    pub async fn begin_request(&self, method: &str, params: Value) -> Result<PendingRequest> {
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            anyhow::ensure!(pending.len() < 32, "too many pending ACP requests");
            pending.insert(id, tx);
        }
        if let Err(error) = self
            .write(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))
            .await
        {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        Ok(PendingRequest {
            id,
            reply: rx,
            pending: self.pending.clone(),
        })
    }

    pub async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.begin_request(method, params).await?.wait(timeout).await
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.write(json!({"jsonrpc":"2.0", "method":method, "params":params}))
            .await
    }

    pub async fn stop(&self) -> Result<()> {
        let mut child = self.child.lock().await;

        #[cfg(unix)]
        {
            // The unreaped owned child prevents PID reuse. Never signal our
            // own group or a process which was not started as a group leader.
            if let Some(pid) = child
                .id()
                .and_then(|id| i32::try_from(id).ok())
                .filter(|id| *id > 1)
            {
                unsafe {
                    if libc::getpgid(pid) == pid && libc::getpgrp() != pid {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                }
            }
        }
        child.start_kill()?;
        child.wait().await?;
        Ok(())
    }
}

/// One request already written, awaiting its reply.
pub struct PendingRequest {
    id: u64,
    reply: oneshot::Receiver<Result<Value>>,
    pending: Arc<Mutex<Pending>>,
}

impl PendingRequest {
    pub async fn wait(self, timeout: Duration) -> Result<Value> {
        let Self { id, reply, pending } = self;
        let outcome = tokio::time::timeout(timeout, reply).await;
        pending.lock().await.remove(&id);
        outcome
            .context("ACP request timed out; do not blindly retry an uncertain mutation")?
            .context("ACP response channel closed")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Command {
        let mut command = Command::new("python3");
        command.arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../scripts/fixtures/workshop-acp-agent.py"),
        );
        command
    }
    #[tokio::test]
    async fn callbacks_do_not_block_cancel_and_eof_fails_pending_requests() {
        let (peer, mut events) = Peer::spawn(fixture()).unwrap();
        let init = peer
            .request(
                "initialize",
                json!({"protocolVersion":1}),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert_eq!(init["protocolVersion"], 1);
        let request_peer = peer.clone();
        let turn = tokio::spawn(async move {
            request_peer.request("session/prompt", json!({"sessionId":"fixture-session","prompt":[{"type":"text","text":"permission"}]}), Duration::from_secs(5)).await
        });
        let callback = tokio::time::timeout(Duration::from_secs(3), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(callback.message["method"], "session/request_permission");
        peer.notify("session/cancel", json!({"sessionId":"fixture-session"}))
            .await
            .unwrap();
        peer.write(json!({"jsonrpc":"2.0","id":callback.message["id"],"result":{"outcome":{"outcome":"cancelled"}}})).await.unwrap();
        drop(callback);
        assert_eq!(turn.await.unwrap().unwrap()["stopReason"], "cancelled");
        let crash = peer
            .request(
                "session/prompt",
                json!({"sessionId":"fixture-session","prompt":[{"type":"text","text":"crash"}]}),
                Duration::from_secs(5),
            )
            .await;
        assert!(crash.unwrap_err().to_string().contains("closed"));
        peer.stop().await.unwrap();
    }
    #[tokio::test]
    async fn malformed_and_oversized_frames_close_the_connection() {
        for code in [
            "print('not json', flush=True)",
            "print('x' * (1024 * 1024 + 1), flush=True)",
        ] {
            let mut command = Command::new("python3");
            command.args(["-c", code]);
            let (peer, mut events) = Peer::spawn(command).unwrap();
            assert!(tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
                .is_none());
            peer.stop().await.unwrap();
        }
    }
}
