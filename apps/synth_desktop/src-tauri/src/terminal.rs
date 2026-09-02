use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    env,
    ffi::{c_void, CString},
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

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCreateRequest {
    pub workspace_id: String,
    pub workspace_root: String,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInfo {
    pub id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub shell: String,
    pub title: String,
    pub status: String,
    #[specta(type = specta_typescript::Number)]
    pub created_at: u64,
    pub exit_code: Option<u32>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TerminalEvent {
    pub terminal_id: String,
    #[specta(type = specta_typescript::Number)]
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
    #[cfg(target_os = "macos")]
    ghostty: Mutex<Option<GhosttySurface>>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalFrame {
    pub x: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalMountRequest {
    pub terminal_id: String,
    pub frame: NativeTerminalFrame,
    pub font_family: String,
    pub font_size: f32,
}

impl NativeTerminalFrame {
    #[cfg(target_os = "macos")]
    fn appkit_rect(&self) -> (f64, f64, f64, f64) {
        (
            self.x.max(0.0),
            self.top.max(0.0),
            self.width.max(1.0),
            self.height.max(1.0),
        )
    }
}

#[cfg(target_os = "macos")]
struct GhosttyCallbackContext {
    session: Arc<TerminalSession>,
}

#[cfg(target_os = "macos")]
struct GhosttySurface {
    handle: *mut c_void,
    _callback: Box<GhosttyCallbackContext>,
}

#[cfg(target_os = "macos")]
unsafe impl Send for GhosttySurface {}

#[cfg(target_os = "macos")]
unsafe impl Sync for GhosttySurface {}

#[cfg(target_os = "macos")]
extern "C" {
    fn synth_ghostty_host_create(
        parent: *mut c_void,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        font_family: *const std::ffi::c_char,
        font_size: f32,
        write: Option<extern "C" fn(*const u8, usize, *mut c_void)>,
        resize: Option<extern "C" fn(u16, u16, *mut c_void)>,
        userdata: *mut c_void,
    ) -> *mut c_void;
    fn synth_ghostty_host_receive(handle: *mut c_void, bytes: *const u8, count: usize);
    fn synth_ghostty_host_finish(handle: *mut c_void, exit_code: u32, runtime_ms: u64);
    fn synth_ghostty_host_set_frame(
        handle: *mut c_void,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    );
    fn synth_ghostty_host_set_visible(handle: *mut c_void, visible: bool);
    fn synth_ghostty_host_focus(handle: *mut c_void);
    fn synth_ghostty_host_destroy(handle: *mut c_void);
}

#[cfg(target_os = "macos")]
impl GhosttySurface {
    fn new(
        parent: *mut c_void,
        session: Arc<TerminalSession>,
        frame: &NativeTerminalFrame,
        font_family: &str,
        font_size: f32,
    ) -> Result<Self> {
        let mut callback = Box::new(GhosttyCallbackContext { session });
        let userdata = (&mut *callback as *mut GhosttyCallbackContext).cast::<c_void>();
        let font_family = CString::new(font_family)
            .unwrap_or_else(|_| CString::new("Menlo").expect("static font family is valid"));
        let (x, y, width, height) = frame.appkit_rect();
        let handle = unsafe {
            synth_ghostty_host_create(
                parent,
                x,
                y,
                width,
                height,
                font_family.as_ptr(),
                font_size.clamp(10.0, 20.0),
                Some(ghostty_write),
                Some(ghostty_resize),
                userdata,
            )
        };
        if handle.is_null() {
            return Err(anyhow!("libghostty could not create a terminal surface"));
        }
        Ok(Self {
            handle,
            _callback: callback,
        })
    }

    fn receive(&self, bytes: &[u8]) {
        if !bytes.is_empty() {
            unsafe { synth_ghostty_host_receive(self.handle, bytes.as_ptr(), bytes.len()) };
        }
    }

    fn finish(&self, exit_code: u32, runtime_ms: u64) {
        unsafe { synth_ghostty_host_finish(self.handle, exit_code, runtime_ms) };
    }

    fn set_frame(&self, frame: &NativeTerminalFrame) {
        let (x, y, width, height) = frame.appkit_rect();
        unsafe { synth_ghostty_host_set_frame(self.handle, x, y, width, height) };
    }

    fn set_visible(&self, visible: bool) {
        unsafe { synth_ghostty_host_set_visible(self.handle, visible) };
    }

    fn focus(&self) {
        unsafe { synth_ghostty_host_focus(self.handle) };
    }
}

#[cfg(target_os = "macos")]
impl Drop for GhosttySurface {
    fn drop(&mut self) {
        unsafe { synth_ghostty_host_destroy(self.handle) };
    }
}

#[cfg(target_os = "macos")]
extern "C" fn ghostty_write(bytes: *const u8, count: usize, userdata: *mut c_void) {
    if bytes.is_null() || userdata.is_null() || count == 0 {
        return;
    }
    let context = unsafe { &*userdata.cast::<GhosttyCallbackContext>() };
    let data = unsafe { std::slice::from_raw_parts(bytes, count) };
    if let Ok(mut writer) = context.session.writer.lock() {
        let _ = writer.write_all(data);
        let _ = writer.flush();
    }
}

#[cfg(target_os = "macos")]
extern "C" fn ghostty_resize(cols: u16, rows: u16, userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    let context = unsafe { &*userdata.cast::<GhosttyCallbackContext>() };
    if let Ok(master) = context.session.master.lock() {
        let _ = master.resize(PtySize {
            rows: clamp_rows(rows),
            cols: clamp_cols(cols),
            pixel_width: 0,
            pixel_height: 0,
        });
    }
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

    #[cfg(target_os = "macos")]
    fn feed_ghostty(&self, bytes: &[u8]) {
        if let Ok(surface) = self.ghostty.lock() {
            if let Some(surface) = surface.as_ref() {
                surface.receive(bytes);
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn finish_ghostty(&self, exit_code: u32) {
        let runtime_ms = self
            .info
            .read()
            .ok()
            .and_then(|info| now_millis().checked_sub(info.created_at))
            .unwrap_or(0);
        if let Ok(surface) = self.ghostty.lock() {
            if let Some(surface) = surface.as_ref() {
                surface.finish(exit_code, runtime_ms);
            }
        }
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
            #[cfg(target_os = "macos")]
            ghostty: Mutex::new(None),
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

    pub fn mount_native(
        &self,
        id: &str,
        parent: *mut c_void,
        frame: &NativeTerminalFrame,
        font_family: &str,
        font_size: f32,
    ) -> Result<bool> {
        #[cfg(target_os = "macos")]
        {
            let session = self.session(id)?;
            let mut mounted = session
                .ghostty
                .lock()
                .map_err(|_| anyhow!("libghostty surface unavailable"))?;
            if let Some(surface) = mounted.as_ref() {
                surface.set_frame(frame);
                surface.set_visible(true);
                return Ok(true);
            }
            let surface = GhosttySurface::new(
                parent,
                session.clone(),
                frame,
                font_family,
                font_size,
            )?;
            if let Ok(scrollback) = session.scrollback.lock() {
                for (_, event) in scrollback.iter() {
                    if let Some(encoded) = event.data_base64.as_deref() {
                        if let Ok(bytes) = STANDARD.decode(encoded) {
                            surface.receive(&bytes);
                        }
                    }
                }
            }
            surface.set_visible(true);
            surface.focus();
            *mounted = Some(surface);
            return Ok(true);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (id, parent, frame, font_family, font_size);
            Ok(false)
        }
    }

    pub fn set_native_frame(&self, id: &str, frame: &NativeTerminalFrame) -> Result<()> {
        #[cfg(target_os = "macos")]
        if let Some(surface) = self
            .session(id)?
            .ghostty
            .lock()
            .map_err(|_| anyhow!("libghostty surface unavailable"))?
            .as_ref()
        {
            surface.set_frame(frame);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (id, frame);
        Ok(())
    }

    pub fn set_native_visible(&self, id: &str, visible: bool) -> Result<()> {
        #[cfg(target_os = "macos")]
        if let Some(surface) = self
            .session(id)?
            .ghostty
            .lock()
            .map_err(|_| anyhow!("libghostty surface unavailable"))?
            .as_ref()
        {
            surface.set_visible(visible);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (id, visible);
        Ok(())
    }

    pub fn focus_native(&self, id: &str) -> Result<()> {
        #[cfg(target_os = "macos")]
        if let Some(surface) = self
            .session(id)?
            .ghostty
            .lock()
            .map_err(|_| anyhow!("libghostty surface unavailable"))?
            .as_ref()
        {
            surface.focus();
        }
        #[cfg(not(target_os = "macos"))]
        let _ = id;
        Ok(())
    }

    pub fn unmount_native(&self, id: &str) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            self.session(id)?
                .ghostty
                .lock()
                .map_err(|_| anyhow!("libghostty surface unavailable"))?
                .take();
        }
        #[cfg(not(target_os = "macos"))]
        let _ = id;
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
                    #[cfg(target_os = "macos")]
                    session.feed_ghostty(&buffer[..size]);
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
        #[cfg(target_os = "macos")]
        session.finish_ghostty(exit_code.unwrap_or(0));
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
