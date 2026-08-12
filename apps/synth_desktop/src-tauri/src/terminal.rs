use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    env,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

const MAX_TERMINALS_PER_WORKSPACE: usize = 8;
const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_SCROLLBACK_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCreateRequest {
    pub workspace_id: String,
    pub workspace_root: String,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInfo {
    pub id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub shell: String,
    pub title: String,
    pub status: String,
    pub created_at: u64,
    pub exit_code: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalEvent {
    pub terminal_id: String,
    pub sequence: u64,
    pub kind: String,
    pub data_base64: Option<String>,
    pub exit_code: Option<u32>,
    pub message: Option<String>,
}

struct TerminalSession {
    info: RwLock<TerminalInfo>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Option<Box<dyn Child + Send + Sync>>>,
    sequence: AtomicU64,
    scrollback: Mutex<VecDeque<(usize, TerminalEvent)>>,
}

impl TerminalSession {
    fn record(&self, mut event: TerminalEvent, bytes: usize) -> TerminalEvent {
        event.sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let mut scrollback = self
            .scrollback
            .lock()
            .expect("terminal scrollback poisoned");
        scrollback.push_back((bytes, event.clone()));
        let mut total: usize = scrollback.iter().map(|(size, _)| size).sum();
        while total > MAX_SCROLLBACK_BYTES {
            if let Some((size, _)) = scrollback.pop_front() {
                total = total.saturating_sub(size);
            } else {
                break;
            }
        }
        event
    }
}

pub struct TerminalManager {
    sessions: RwLock<HashMap<String, Arc<TerminalSession>>>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub fn create(&self, app: AppHandle, request: TerminalCreateRequest) -> Result<TerminalInfo> {
        let cwd = validate_cwd(&request.workspace_root, request.cwd.as_deref())?;
        let count = self
            .sessions
            .read()
            .expect("terminal map poisoned")
            .values()
            .filter(|session| {
                session
                    .info
                    .read()
                    .expect("terminal info poisoned")
                    .workspace_id
                    == request.workspace_id
            })
            .count();
        if count >= MAX_TERMINALS_PER_WORKSPACE {
            return Err(anyhow!(
                "A workspace may have at most {MAX_TERMINALS_PER_WORKSPACE} terminals"
            ));
        }
        let shell = default_shell();
        let cols = clamp_cols(request.cols.unwrap_or(100));
        let rows = clamp_rows(request.rows.unwrap_or(28));
        let pair = native_pty_system().openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(&shell);
        command.cwd(&cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("SYNTH_TERMINAL", "1");
        let child = pair
            .slave
            .spawn_command(command)
            .context("failed to start shell")?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let id = Uuid::new_v4().to_string();
        let title = cwd
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Terminal")
            .to_owned();
        let info = TerminalInfo {
            id: id.clone(),
            workspace_id: request.workspace_id,
            cwd: cwd.to_string_lossy().into_owned(),
            shell: shell.to_string_lossy().into_owned(),
            title,
            status: "running".into(),
            created_at: now_millis(),
            exit_code: None,
        };
        let session = Arc::new(TerminalSession {
            info: RwLock::new(info.clone()),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            child: Mutex::new(Some(child)),
            sequence: AtomicU64::new(0),
            scrollback: Mutex::new(VecDeque::new()),
        });
        self.sessions
            .write()
            .expect("terminal map poisoned")
            .insert(id, session.clone());
        spawn_reader(app, session, reader);
        Ok(info)
    }

    pub fn list(&self, workspace_id: Option<&str>) -> Vec<TerminalInfo> {
        let mut terminals: Vec<_> = self
            .sessions
            .read()
            .expect("terminal map poisoned")
            .values()
            .filter_map(|session| {
                let info = session.info.read().ok()?.clone();
                (workspace_id.is_none() || workspace_id == Some(info.workspace_id.as_str()))
                    .then_some(info)
            })
            .collect();
        terminals.sort_by_key(|terminal| terminal.created_at);
        terminals
    }

    pub fn snapshot(&self, id: &str, after: u64) -> Result<Vec<TerminalEvent>> {
        let session = self.session(id)?;
        let events = session
            .scrollback
            .lock()
            .map_err(|_| anyhow!("terminal scrollback unavailable"))?
            .iter()
            .filter(|(_, event)| event.sequence > after)
            .map(|(_, event)| event.clone())
            .collect();
        Ok(events)
    }

    pub fn write(&self, id: &str, data: &str) -> Result<()> {
        if data.len() > MAX_INPUT_BYTES {
            return Err(anyhow!("terminal input exceeds 64 KiB"));
        }
        self.session(id)?
            .writer
            .lock()
            .map_err(|_| anyhow!("terminal writer unavailable"))?
            .write_all(data.as_bytes())?;
        Ok(())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        self.session(id)?
            .master
            .lock()
            .map_err(|_| anyhow!("terminal unavailable"))?
            .resize(PtySize {
                rows: clamp_rows(rows),
                cols: clamp_cols(cols),
                pixel_width: 0,
                pixel_height: 0,
            })?;
        Ok(())
    }

    pub fn close(&self, id: &str) -> Result<()> {
        let session = self
            .sessions
            .write()
            .map_err(|_| anyhow!("terminal map unavailable"))?
            .remove(id)
            .ok_or_else(|| anyhow!("Unknown terminal: {id}"))?;
        if let Some(child) = session
            .child
            .lock()
            .map_err(|_| anyhow!("terminal process unavailable"))?
            .as_mut()
        {
            child.kill()?;
        }
        Ok(())
    }

    fn session(&self, id: &str) -> Result<Arc<TerminalSession>> {
        self.sessions
            .read()
            .map_err(|_| anyhow!("terminal map unavailable"))?
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown terminal: {id}"))
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        if let Ok(sessions) = self.sessions.read() {
            for session in sessions.values() {
                if let Ok(mut child) = session.child.lock() {
                    if let Some(child) = child.as_mut() {
                        let _ = child.kill();
                    }
                }
            }
        }
    }
}

fn spawn_reader(app: AppHandle, session: Arc<TerminalSession>, mut reader: Box<dyn Read + Send>) {
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    let event = session.record(
                        TerminalEvent {
                            terminal_id: session.info.read().unwrap().id.clone(),
                            sequence: 0,
                            kind: "output".into(),
                            data_base64: Some(STANDARD.encode(&buffer[..size])),
                            exit_code: None,
                            message: None,
                        },
                        size,
                    );
                    let _ = app.emit(crate::contract::events::EventChannel::TERMINAL, event);
                }
                Err(error) => {
                    let event = session.record(
                        TerminalEvent {
                            terminal_id: session.info.read().unwrap().id.clone(),
                            sequence: 0,
                            kind: "error".into(),
                            data_base64: None,
                            exit_code: None,
                            message: Some(error.to_string()),
                        },
                        0,
                    );
                    let _ = app.emit(crate::contract::events::EventChannel::TERMINAL, event);
                    break;
                }
            }
        }
        let exit_code = session
            .child
            .lock()
            .ok()
            .and_then(|mut child| child.as_mut().and_then(|value| value.wait().ok()))
            .map(|status| status.exit_code());
        if let Ok(mut info) = session.info.write() {
            info.status = "exited".into();
            info.exit_code = exit_code;
        }
        let event = session.record(
            TerminalEvent {
                terminal_id: session.info.read().unwrap().id.clone(),
                sequence: 0,
                kind: "exit".into(),
                data_base64: None,
                exit_code,
                message: None,
            },
            0,
        );
        let _ = app.emit(crate::contract::events::EventChannel::TERMINAL, event);
    });
}

fn validate_cwd(root: &str, cwd: Option<&str>) -> Result<PathBuf> {
    let root = Path::new(root)
        .canonicalize()
        .context("workspace folder does not exist")?;
    let cwd = Path::new(cwd.unwrap_or(root.to_string_lossy().as_ref()))
        .canonicalize()
        .context("terminal folder does not exist")?;
    if !cwd.is_dir() || !cwd.starts_with(&root) {
        return Err(anyhow!("terminal folder must be inside the workspace"));
    }
    Ok(cwd)
}

fn default_shell() -> PathBuf {
    env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .or_else(|| {
            ["/bin/zsh", "/bin/sh"]
                .into_iter()
                .map(PathBuf::from)
                .find(|path| path.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

fn clamp_cols(value: u16) -> u16 {
    value.clamp(2, 500)
}
fn clamp_rows(value: u16) -> u16 {
    value.clamp(1, 300)
}
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimensions_are_bounded() {
        assert_eq!(clamp_cols(0), 2);
        assert_eq!(clamp_cols(999), 500);
        assert_eq!(clamp_rows(0), 1);
    }
    #[test]
    fn cwd_must_stay_in_workspace() {
        let root = std::env::temp_dir();
        assert!(validate_cwd(root.to_str().unwrap(), Some("/")).is_err() || root == Path::new("/"));
    }
}
