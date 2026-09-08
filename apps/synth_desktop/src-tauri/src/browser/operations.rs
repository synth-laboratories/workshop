//! Managed browser operation catalogue and its single runtime-owned backend.
//! The legacy stdio executable forwards here; no client owns Chromium lifetime.
use super::{DEFAULT_MAX_CHARS, HARD_MAX_CHARS};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{future::Future, pin::Pin, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};

fn target_schema(optional: bool) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "ref": {
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"}, "tab_id": {"type": "string"},
                    "document_revision": {"type": "string"}, "element_id": {"type": "string"}
                },
                "required": ["session_id", "tab_id", "document_revision", "element_id"],
                "additionalProperties": false
            },
            "locator": {
                "type": "object",
                "properties": {"role": {"type": "string"}, "name": {"type": "string"}, "exact": {"type": "boolean"}},
                "required": ["role", "name"], "additionalProperties": false
            }
        },
        "oneOf": [{"required": ["ref"]}, {"required": ["locator"]}],
        "additionalProperties": false
    });
    if optional {
        schema["description"] =
            json!("Optional for browser_press; omit to send the key to the page.");
    }
    schema
}

fn base_properties(tab: bool) -> serde_json::Map<String, Value> {
    let mut properties =
        serde_json::Map::from_iter([("session_id".into(), json!({"type":"string"}))]);
    if tab {
        properties.insert("tab_id".into(), json!({"type":"string"}));
    }
    properties
}

fn schema(properties: serde_json::Map<String, Value>, required: &[&str]) -> Value {
    json!({"type":"object", "properties": properties, "required": required, "additionalProperties": false})
}

pub fn tools() -> Value {
    let bounded = || json!({"type":"integer","minimum":256,"maximum":HARD_MAX_CHARS,"default":DEFAULT_MAX_CHARS});
    let mut items = Vec::new();
    items.push(json!({"name":"browser_status","description":"Check the local managed-browser runtime and list human-approved origins without starting Chromium.","inputSchema":schema(serde_json::Map::new(),&[]),"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}));
    items.push(json!({"name":"browser_create_session","description":"Create a visible Workshop-managed Chromium session using a dedicated persistent profile.","inputSchema":schema(serde_json::Map::from_iter([("profile".into(),json!({"type":"string"}))]),&[])}));
    items.push(json!({"name":"browser_close_session","description":"Close only this Workshop-managed session; never touches user browser tabs.","inputSchema":schema(base_properties(false),&["session_id"])}));
    items.push(json!({"name":"browser_list_tabs","description":"List stable tab identities in one managed session.","inputSchema":schema(base_properties(false),&["session_id"])}));
    let mut new_tab = base_properties(false);
    new_tab.insert("url".into(), json!({"type":"string"}));
    items.push(json!({"name":"browser_new_tab","description":"Create a managed tab, optionally navigating to an approved origin.","inputSchema":schema(new_tab,&["session_id"])}));
    items.push(json!({"name":"browser_close_tab","description":"Close one managed tab. Tab IDs are never reused.","inputSchema":schema(base_properties(true),&["session_id","tab_id"])}));
    let mut navigate = base_properties(true);
    navigate.insert("url".into(), json!({"type":"string"}));
    items.push(json!({"name":"browser_navigate","description":"Navigate a managed tab. Unapproved origins fail closed.","inputSchema":schema(navigate,&["session_id","tab_id","url"])}));
    items.push(json!({"name":"browser_back","description":"Navigate one step back in the managed tab history.","inputSchema":schema(base_properties(true),&["session_id","tab_id"])}));
    for (name, description) in [
        (
            "browser_snapshot",
            "Read a bounded semantic snapshot; raw full DOM is never returned.",
        ),
        (
            "browser_query",
            "Query visible semantic elements without reading the full page.",
        ),
    ] {
        let mut props = base_properties(true);
        props.insert("max_chars".into(), bounded());
        props.insert("cursor".into(), json!({"type":"integer","minimum":0}));
        if name == "browser_query" {
            props.insert("role".into(), json!({"type":"string"}));
            props.insert("name".into(), json!({"type":"string"}));
        }
        items.push(json!({"name":name,"description":description,"inputSchema":schema(props,&["session_id","tab_id"])}));
    }
    let mut subtree = base_properties(true);
    subtree.insert("target".into(), target_schema(false));
    subtree.insert("max_chars".into(), bounded());
    subtree.insert("cursor".into(), json!({"type":"integer","minimum":0}));
    items.push(json!({"name":"browser_subtree","description":"Read bounded text beneath a revision-bound ref or unique semantic locator.","inputSchema":schema(subtree,&["session_id","tab_id","target"])}));
    for (name, description) in [("browser_click","Click a unique semantic target; consequential labels fail closed pending host confirmation."),("browser_fill","Fill a unique field without echoing its value."),("browser_upload","Attach explicitly selected files from an operator-approved root."),("browser_download","Click a unique download target and save into the managed profile download directory.")] {
        let mut props=base_properties(true);props.insert("target".into(),target_schema(false));
        if name=="browser_fill" {props.insert("value".into(),json!({"type":"string"}));}
        if name=="browser_upload" {props.insert("file_paths".into(),json!({"type":"array","items":{"type":"string"},"minItems":1}));}
        items.push(json!({"name":name,"description":description,"inputSchema":schema(props,&["session_id","tab_id","target"])}));
    }
    let mut press = base_properties(true);
    press.insert("target".into(), target_schema(true));
    press.insert("key".into(), json!({"type":"string"}));
    items.push(json!({"name":"browser_press","description":"Press a key on a unique target or on the page.","inputSchema":schema(press,&["session_id","tab_id","key"])}));
    let mut scroll = base_properties(true);
    scroll.insert("target".into(), target_schema(true));
    scroll.insert("delta_x".into(), json!({"type":"number"}));
    scroll.insert("delta_y".into(), json!({"type":"number"}));
    items.push(json!({"name":"browser_scroll","description":"Scroll the page or a unique semantic target.","inputSchema":schema(scroll,&["session_id","tab_id"])}));
    let mut shot = base_properties(true);
    shot.insert("full_page".into(), json!({"type":"boolean"}));
    items.push(json!({"name":"browser_screenshot","description":"Capture a screenshot into the managed profile; returns the controlled path.","inputSchema":schema(shot,&["session_id","tab_id"])}));
    json!({"tools":items})
}

pub fn contains(name: &str) -> bool {
    tools()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == name)
}

#[derive(Default)]
pub struct Manager {
    backend: Mutex<Option<Backend>>,
}
struct Backend {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    sequence: u64,
}
impl Backend {
    fn spawn() -> Result<Self> {
        let script = super::backend_script_path();
        anyhow::ensure!(script.is_file(), "managed browser backend is not installed");
        let mut command =
            Command::new(std::env::var_os("SYNTH_BROWSER_NODE").unwrap_or_else(|| "node".into()));
        command.arg(script).env_clear();
        for key in [
            "PATH",
            "HOME",
            "USER",
            "TMPDIR",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
            "SystemRoot",
            "PLAYWRIGHT_BROWSERS_PATH",
            // Explicit operator launch policy; never accepted from tool args.
            "SYNTH_BROWSER_HEADLESS",
            "SYNTH_BROWSER_UPLOAD_ROOTS",
            "SYNTH_BROWSER_ALLOW_CONSEQUENTIAL",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .env("SYNTH_BROWSER_POLICY_FILE", super::policy_path())
            .env("SYNTH_BROWSER_PROFILE_ROOT", super::profile_root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().context("start managed browser backend")?;
        Ok(Self {
            input: child.stdin.take().context("browser stdin")?,
            output: BufReader::new(child.stdout.take().context("browser stdout")?),
            child,
            sequence: 0,
        })
    }
    async fn call(&mut self, operation: &str, arguments: Value) -> Result<Value> {
        self.sequence += 1;
        let wire = serde_json::to_vec(
            &json!({"id":self.sequence,"operation":operation,"arguments":arguments}),
        )?;
        anyhow::ensure!(wire.len() <= 1024 * 1024, "browser request too large");
        self.input.write_all(&wire).await?;
        self.input.write_all(b"\n").await?;
        self.input.flush().await?;
        let mut line = Vec::new();
        let size = (&mut self.output)
            .take(1024 * 1024 + 1)
            .read_until(b'\n', &mut line)
            .await?;
        anyhow::ensure!(
            size > 0 && size <= 1024 * 1024,
            "browser backend disconnected or exceeded frame limit"
        );
        let reply: Value = serde_json::from_slice(&line)?;
        anyhow::ensure!(
            reply["id"].as_u64() == Some(self.sequence),
            "browser response identity mismatch"
        );
        anyhow::ensure!(
            reply["ok"] == true,
            "browser_operation_failed: {}",
            reply["error"]
                .as_str()
                .unwrap_or("browser operation failed")
        );
        Ok(reply["response"].clone())
    }
    async fn stop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            #[cfg(unix)]
            if let Some(pid) = self.child.id() {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            let _ = self.child.start_kill();
        }
        let _ = self.child.wait().await;
    }
}
impl Manager {
    pub async fn call(&self, name: &str, args: Value) -> Result<Value> {
        anyhow::ensure!(contains(name), "unknown browser operation");
        if name == "browser_status" {
            return Ok(serde_json::to_value(
                tokio::task::spawn_blocking(super::runtime_status).await?,
            )?);
        }
        anyhow::ensure!(crate::context::mcp_group_enabled(crate::context::BROWSER_MCP_GROUP), "human_action_required: enable the Browser MCP group in Settings > Context");
        let mut backend = self.backend.lock().await;
        if backend.is_none() {
            *backend = Some(Backend::spawn()?);
        }
        let result = tokio::time::timeout(
            Duration::from_secs(45),
            backend.as_mut().unwrap().call(name, args),
        )
        .await;
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                if !error.to_string().starts_with("browser_operation_failed:") {
                    if let Some(mut child) = backend.take() {
                        child.stop().await;
                    }
                }
                Err(error)
            }
            Err(_) => {
                if let Some(mut child) = backend.take() {
                    child.stop().await;
                }
                anyhow::bail!("browser operation timed out; backend stopped, outcome uncertain")
            }
        }
    }
}
impl crate::services::ManagedService for Manager {
    fn name(&self) -> &'static str {
        "managed-browser"
    }
    fn stop(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            if let Some(mut child) = self.backend.lock().await.take() {
                child.stop().await;
            }
            Ok(())
        })
    }
}
