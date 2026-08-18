//! The helper's MCP server: tool schema, dispatch, and auto-settling.
//!
//! Two things here are policy rather than plumbing:
//!
//! * **Settling is the runtime's job.** After a mutating action the helper waits
//!   for the tree to stop changing before it answers. The agent must never
//!   sleep — an agent that has to guess a delay guesses wrong in both
//!   directions, and a `sleep` tool is a tool that gets used to poll.
//! * **Actions prefer accessibility over pixels.** An element with `AXPress` is
//!   pressed, not clicked at coordinates, because a synthetic click at a point
//!   requires the window to be where we last saw it.

use crate::{apps, ax, capture, events, permissions};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

/// Wait at least this long after acting before reading back.
const SETTLE_MIN: Duration = Duration::from_millis(1_000);
/// Keep waiting while the tree is still changing, up to here.
const SETTLE_MAX: Duration = Duration::from_millis(5_000);
const SETTLE_POLL: Duration = Duration::from_millis(150);
const DEFAULT_MAX_CHARS: usize = 16_000;
const HARD_MAX_CHARS: usize = 20_000;

pub const TOOL_NAME: &str = "computer_use";
pub const PERMISSIONS_TOOL: &str = "computer_use_permissions";
/// Deliberately absent from `tools/list`. Desktop's authenticated child pipe
/// uses this recorder-only path to capture canonical state without ever
/// placing that state in an agent tool result.
pub const RECORDER_TOOL: &str = "computer_use_record_state";

pub struct Server {
    /// Last rendered tree per app, for diffing and for resolving element
    /// indexes. Desktop refuses stale indexes, but the helper still has to be
    /// able to say "index 9 is not in the tree I last read".
    last_render: HashMap<String, String>,
    last_tree: HashMap<String, ax::AppTree>,
}

impl Server {
    pub fn new() -> Self {
        Self {
            last_render: HashMap::new(),
            last_tree: HashMap::new(),
        }
    }

    /// Tool catalog. Mirrors `docs/COMPUTER_USE.md` §5 exactly; Desktop
    /// validates against the same vocabulary, so a verb added in one place and
    /// not the other fails loudly rather than silently doing nothing.
    pub fn tools(&self) -> Value {
        json!({"tools": [
            {
                "name": TOOL_NAME,
                "description": "Observe and drive one macOS app. Prefer element_index over coordinates; re-read state after every action.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "verb": {"type": "string", "enum": [
                            "list_apps", "get_app_state", "get_app_outline", "find_elements",
                            "get_subtree", "click", "set_value", "type_text",
                            "press_key", "scroll", "select_text", "drag", "perform_secondary_action"
                        ]},
                        "app": {"type": "string"},
                        "disable_diff": {"type": "boolean"},
                        "scope": {"type": "string", "enum": ["all", "visible"]},
                        "max_chars": {"type": "integer", "minimum": 256, "maximum": 20000},
                        "cursor": {"type": "integer", "minimum": 0},
                        "role": {"type": "string"},
                        "name": {"type": "string"},
                        "depth": {"type": "integer", "minimum": 0, "maximum": 24},
                        "element_index": {"type": "integer", "minimum": 0},
                        "x": {"type": "number"},
                        "y": {"type": "number"},
                        "mouse_button": {"type": "string", "enum": ["left", "right", "middle"]},
                        "click_count": {"type": "integer", "minimum": 1, "maximum": 3},
                        "value": {"type": "string"},
                        "text": {"type": "string"},
                        "key": {"type": "string"},
                        "direction": {"type": "string", "enum": ["up", "down", "left", "right"]},
                        "pages": {"type": "number"},
                        "prefix": {"type": "string"},
                        "suffix": {"type": "string"},
                        "selection_type": {"type": "string", "enum": ["exact", "line", "paragraph", "all"]},
                        "from_x": {"type": "number"}, "from_y": {"type": "number"},
                        "to_x": {"type": "number"}, "to_y": {"type": "number"},
                        "action": {"type": "string"},
                        "include_screenshot": {"type": "boolean"}
                    },
                    "required": ["verb"],
                    "additionalProperties": false
                }
            },
            {
                "name": PERMISSIONS_TOOL,
                "description": "Report this helper's OS grants. `request` shows the system prompts and is only ever called from the permission wizard.",
                "inputSchema": {
                    "type": "object",
                    "properties": {"operation": {"type": "string", "enum": ["probe", "request"]}},
                    "required": ["operation"],
                    "additionalProperties": false
                }
            }
        ]})
    }

    pub fn call(&mut self, name: &str, arguments: &Value) -> Result<Value> {
        match name {
            PERMISSIONS_TOOL => {
                let grants = match arguments.get("operation").and_then(Value::as_str) {
                    Some("request") => permissions::request(),
                    _ => permissions::probe(),
                };
                Ok(serde_json::to_value(grants)?)
            }
            TOOL_NAME => self.dispatch(arguments),
            RECORDER_TOOL => self.read_record_state(arguments),
            other => bail!("unknown tool `{other}`"),
        }
    }

    fn dispatch(&mut self, arguments: &Value) -> Result<Value> {
        let verb = arguments
            .get("verb")
            .and_then(Value::as_str)
            .context("verb is required")?;

        if verb == "list_apps" {
            return Ok(json!({ "apps": apps::running_apps()? }));
        }

        let app = arguments
            .get("app")
            .and_then(Value::as_str)
            .context("app is required")?
            .to_owned();

        // Reading is allowed to launch the app; §5 says so explicitly, and an
        // agent that must ask a human to open Mail first is not much use.
        let pid = apps::resolve_or_launch(&app)?;

        if matches!(
            verb,
            "get_app_state" | "get_app_outline" | "find_elements" | "get_subtree"
        ) {
            let disable_diff = arguments
                .get("disable_diff")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            return self.read_observation(&app, pid, verb, disable_diff, arguments);
        }

        // Everything below mutates. Resolve the element first so a bad index
        // fails before anything is delivered to the app.
        let element_index = arguments.get("element_index").and_then(Value::as_u64);
        let handle = match element_index {
            Some(index) => Some(self.resolve_element(&app, index)?),
            None => None,
        };

        match verb {
            "click" => match (handle, element_index) {
                (Some(element), _) => self.press_or_click(pid, element)?,
                (None, _) => {
                    let x = number(arguments, "x")?;
                    let y = number(arguments, "y")?;
                    events::click(
                        pid,
                        x,
                        y,
                        arguments
                            .get("mouse_button")
                            .and_then(Value::as_str)
                            .unwrap_or("left"),
                        arguments
                            .get("click_count")
                            .and_then(Value::as_u64)
                            .unwrap_or(1) as u32,
                    )?;
                }
            },
            "set_value" => {
                let value = arguments
                    .get("value")
                    .and_then(Value::as_str)
                    .context("value is required")?;
                ax::set_value(handle.context("element_index is required")?, value)?;
            }
            "type_text" => {
                let text = arguments
                    .get("text")
                    .and_then(Value::as_str)
                    .context("text is required")?;
                events::type_text(pid, text)?;
            }
            "press_key" => {
                let key = arguments
                    .get("key")
                    .and_then(Value::as_str)
                    .context("key is required")?;
                events::press_key(pid, key)?;
            }
            "scroll" => {
                let direction = arguments
                    .get("direction")
                    .and_then(Value::as_str)
                    .context("direction is required")?;
                events::scroll(
                    pid,
                    direction,
                    arguments
                        .get("pages")
                        .and_then(Value::as_f64)
                        .unwrap_or(1.0),
                )?;
            }
            "select_text" => {
                let element = handle.context("element_index is required")?;
                let text = arguments
                    .get("text")
                    .and_then(Value::as_str)
                    .context("text is required")?;
                self.select_text(element, text)?;
            }
            "drag" => {
                events::drag(
                    pid,
                    number(arguments, "from_x")?,
                    number(arguments, "from_y")?,
                    number(arguments, "to_x")?,
                    number(arguments, "to_y")?,
                )?;
            }
            "perform_secondary_action" => {
                let element = handle.context("element_index is required")?;
                let action = arguments
                    .get("action")
                    .and_then(Value::as_str)
                    .context("action is required")?;
                let available = unsafe { ax::actions(element) };
                if !available.iter().any(|name| name == action) {
                    bail!(
                        "`{action}` is not exposed by this element; it offers [{}]",
                        available.join(", ")
                    );
                }
                ax::perform(element, action)?;
            }
            other => bail!("unknown verb `{other}`"),
        }

        // Settle, then read back. The answer to a mutating action is the state
        // it produced, so the agent never has to ask a second time.
        self.settle(&app, pid);
        self.read_observation(&app, pid, "get_app_state", false, arguments)
    }

    /// Prefer the accessibility action over a synthetic click. A click at a
    /// point needs the window to still be where it was; `AXPress` does not.
    fn press_or_click(&self, pid: i32, element: crate::sys::AXUIElementRef) -> Result<()> {
        let available = unsafe { ax::actions(element) };
        if available.iter().any(|name| name == "AXPress") {
            return ax::perform(element, "AXPress");
        }
        let Some(tree_element) = self.element_frame(element) else {
            bail!("this element exposes no press action and reports no position to click");
        };
        events::click(
            pid,
            tree_element[0] + tree_element[2] / 2.0,
            tree_element[1] + tree_element[3] / 2.0,
            "left",
            1,
        )
    }

    fn element_frame(&self, handle: crate::sys::AXUIElementRef) -> Option<[f64; 4]> {
        self.last_tree
            .values()
            .flat_map(|tree| tree.elements.iter())
            .find(|element| element.handle() == handle)
            .and_then(|element| element.frame)
    }

    fn resolve_element(&self, app: &str, index: u64) -> Result<crate::sys::AXUIElementRef> {
        let tree = self
            .last_tree
            .get(app)
            .with_context(|| format!("no accessibility tree has been read for `{app}` yet"))?;
        let element = tree.get(index).with_context(|| {
            format!(
                "element {index} is not in the last tree read for `{app}`, which had {} elements",
                tree.elements.len()
            )
        })?;
        Ok(element.handle())
    }

    #[cfg(target_os = "macos")]
    fn select_text(&self, element: crate::sys::AXUIElementRef, needle: &str) -> Result<()> {
        use crate::sys::*;
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;

        let value = unsafe { ax::copy_attribute(element, "AXValue") }
            .context("this element has no text to select in")?;
        let text = unsafe {
            CFString::wrap_under_get_rule(
                value.as_CFTypeRef() as core_foundation_sys::string::CFStringRef
            )
        }
        .to_string();
        let Some(byte_offset) = text.find(needle) else {
            bail!("`{needle}` does not appear in this element's text");
        };
        // Accessibility ranges are in UTF-16 units, not bytes. Using a byte
        // offset silently selects the wrong span the moment any non-ASCII text
        // appears earlier in the field.
        let location = text[..byte_offset].encode_utf16().count() as isize;
        let length = needle.encode_utf16().count() as isize;
        unsafe {
            let range = CFRangeRaw { location, length };
            let boxed = AXValueCreate(
                kAXValueTypeCFRange,
                &range as *const _ as *const std::ffi::c_void,
            );
            if boxed.is_null() {
                bail!("could not build a selection range");
            }
            let key = CFString::new("AXSelectedTextRange");
            let status = AXUIElementSetAttributeValue(element, key.as_concrete_TypeRef(), boxed);
            core_foundation_sys::base::CFRelease(boxed);
            if status != kAXErrorSuccess {
                bail!("{}", ax_error_message(status));
            }
        }
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn select_text(&self, _element: crate::sys::AXUIElementRef, _needle: &str) -> Result<()> {
        bail!("Computer Use is macOS only")
    }

    /// Wait for the UI to stop changing. Bounded on both ends: always at least
    /// `SETTLE_MIN` so an animation is not caught mid-frame, never more than
    /// `SETTLE_MAX` so a spinner that never stops does not hang the session.
    fn settle(&self, app: &str, pid: i32) {
        let started = Instant::now();
        std::thread::sleep(SETTLE_MIN);
        let mut previous = ax::read_tree(pid)
            .map(|tree| tree.render())
            .unwrap_or_default();
        while started.elapsed() < SETTLE_MAX {
            std::thread::sleep(SETTLE_POLL);
            let current = ax::read_tree(pid)
                .map(|tree| tree.render())
                .unwrap_or_default();
            if current == previous {
                return;
            }
            previous = current;
        }
        let _ = app;
    }

    fn read_observation(
        &mut self,
        app: &str,
        pid: i32,
        verb: &str,
        disable_diff: bool,
        arguments: &Value,
    ) -> Result<Value> {
        let tree = ax::read_tree(pid)?;
        let rendered = tree.render();
        let unbounded = match verb {
            "get_app_state" => {
                let visible_only =
                    arguments.get("scope").and_then(Value::as_str) == Some("visible");
                if visible_only {
                    render_elements(tree.elements.iter().filter(|element| {
                        element
                            .frame
                            .is_some_and(|frame| frame[2] > 0.0 && frame[3] > 0.0)
                    }))
                } else if disable_diff {
                    rendered.clone()
                } else {
                    match self.last_render.get(app) {
                        Some(previous) => tree.diff_from(previous),
                        None => rendered.clone(),
                    }
                }
            }
            "find_elements" => {
                let role = arguments.get("role").and_then(Value::as_str);
                let name = arguments
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_lowercase);
                let action = arguments.get("action").and_then(Value::as_str);
                render_elements(tree.elements.iter().filter(|element| {
                    role.is_none_or(|wanted| element.role.eq_ignore_ascii_case(wanted))
                        && name
                            .as_ref()
                            .is_none_or(|wanted| element.label.to_lowercase().contains(wanted))
                        && action.is_none_or(|wanted| {
                            element.actions.iter().any(|offered| offered == wanted)
                        })
                }))
            }
            "get_subtree" => {
                let root = arguments
                    .get("element_index")
                    .and_then(Value::as_u64)
                    .context("element_index is required")?;
                let max_depth =
                    arguments.get("depth").and_then(Value::as_u64).unwrap_or(3) as usize;
                tree.get(root)
                    .with_context(|| format!("element {root} is not in this tree"))?;
                render_elements(tree.elements.iter().filter(|element| {
                    element.index == root || is_descendant(&tree, element, root, max_depth)
                }))
            }
            "get_app_outline" => render_outline(&tree),
            other => bail!("unknown observation verb `{other}`"),
        };
        let element_count = tree.elements.len() as u64;
        let truncated = tree.truncated;
        self.last_render.insert(app.to_owned(), rendered);
        self.last_tree.insert(app.to_owned(), tree);

        let max_chars = arguments
            .get("max_chars")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_MAX_CHARS as u64)
            .clamp(256, HARD_MAX_CHARS as u64) as usize;
        let cursor = arguments.get("cursor").and_then(Value::as_u64).unwrap_or(0) as usize;
        let (text, continuation_cursor, response_truncated) =
            bounded_text(&unbounded, cursor, max_chars);

        Ok(agent_state_payload(
            app,
            pid,
            text,
            element_count,
            truncated || response_truncated,
            continuation_cursor,
            max_chars,
        ))
    }

    fn read_record_state(&mut self, arguments: &Value) -> Result<Value> {
        let app = arguments
            .get("app")
            .and_then(Value::as_str)
            .context("app is required")?;
        let pid = apps::resolve_or_launch(app)?;
        let tree = ax::read_tree(pid)?;
        let rendered = tree.render();
        let screenshot = if arguments
            .get("include_screenshot")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            capture::capture_app(pid).unwrap_or(None)
        } else {
            None
        };
        Ok(json!({
            "app": app,
            "pid": pid,
            "fullText": rendered,
            "elementCount": tree.elements.len() as u64,
            "truncated": tree.truncated,
            "screenshotPng": screenshot.map(|bytes| base64(&bytes)),
        }))
    }
}

#[allow(clippy::too_many_arguments)]
fn agent_state_payload(
    app: &str,
    pid: i32,
    text: String,
    element_count: u64,
    truncated: bool,
    continuation_cursor: Option<usize>,
    max_chars: usize,
) -> Value {
    json!({
        "app": app,
        "pid": pid,
        "text": text,
        "elementCount": element_count,
        "truncated": truncated,
        "continuationCursor": continuation_cursor,
        "maxChars": max_chars,
    })
}

fn render_elements<'a>(elements: impl Iterator<Item = &'a ax::Element>) -> String {
    let mut out = String::new();
    for element in elements {
        out.push_str(&format!("[{}] {}", element.index, element.role));
        if !element.label.is_empty() {
            out.push_str(&format!(" \"{}\"", element.label));
        }
        if !element.actions.is_empty() {
            out.push_str(&format!(" actions=[{}]", element.actions.join(",")));
        }
        out.push('\n');
    }
    out
}

fn is_descendant(tree: &ax::AppTree, element: &ax::Element, root: u64, max_depth: usize) -> bool {
    let mut parent = element.parent_index;
    let mut distance = 0usize;
    while let Some(index) = parent {
        distance += 1;
        if index == root {
            return distance <= max_depth;
        }
        if distance >= max_depth {
            return false;
        }
        parent = tree.get(index).and_then(|candidate| candidate.parent_index);
    }
    false
}

fn render_outline(tree: &ax::AppTree) -> String {
    let mut roles = std::collections::BTreeMap::<&str, usize>::new();
    for element in &tree.elements {
        *roles.entry(&element.role).or_default() += 1;
    }
    let mut out = format!("{} elements", tree.elements.len());
    if tree.truncated {
        out.push_str(" (canonical snapshot truncated)");
    }
    out.push('\n');
    for (role, count) in roles {
        out.push_str(&format!("{role}: {count}\n"));
    }
    out.push_str("Top-level controls:\n");
    out.push_str(&render_elements(
        tree.elements
            .iter()
            .filter(|element| element.parent_index.is_none() || element.depth <= 2)
            .take(100),
    ));
    out
}

fn bounded_text(
    text: &str,
    requested_cursor: usize,
    max_chars: usize,
) -> (String, Option<usize>, bool) {
    let mut start = requested_cursor.min(text.len());
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (start + max_chars).min(text.len());
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = end < text.len();
    (
        text[start..end].to_owned(),
        truncated.then_some(end),
        truncated,
    )
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

fn number(arguments: &Value, key: &str) -> Result<f64> {
    arguments
        .get(key)
        .and_then(Value::as_f64)
        .with_context(|| format!("`{key}` is required and must be a number"))
}

/// Minimal base64. The helper has three dependencies on purpose; adding a
/// crate to encode one field would not be a good trade.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let triple = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Serve MCP on stdio until the input closes.
pub fn serve(server: &mut Server, authorize: &dyn Fn() -> Result<()>) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.context("read stdin")?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = message.get("id").cloned();
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();

        // Notifications expect no answer.
        if id.is_none() {
            continue;
        }

        let response = match method.as_str() {
            "initialize" => json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {
                    "name": "synth-computer-use",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
            // The handshake and the catalog answer anyone. Every call that can
            // touch the machine goes through `authorize` first — the same split
            // the reference implementation makes, and the reason its tools/list
            // succeeds from any caller while its first real call does not.
            "tools/list" => server.tools(),
            "tools/call" => {
                if let Err(error) = authorize() {
                    write_error(&mut stdout, &id, -32000, &error.to_string())?;
                    continue;
                }
                let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match server.call(name, &arguments) {
                    Ok(result) => json!({
                        "content": [{"type": "text", "text": result.to_string()}],
                        "structuredContent": result,
                        "isError": false
                    }),
                    Err(error) => json!({
                        "content": [{"type": "text", "text": error.to_string()}],
                        "isError": true
                    }),
                }
            }
            other => {
                write_error(
                    &mut stdout,
                    &id,
                    -32601,
                    &format!("unknown method `{other}`"),
                )?;
                continue;
            }
        };
        let payload = json!({"jsonrpc": "2.0", "id": id, "result": response});
        writeln!(stdout, "{payload}").context("write stdout")?;
        stdout.flush().context("flush stdout")?;
    }
    Ok(())
}

fn write_error(
    stdout: &mut impl Write,
    id: &Option<Value>,
    code: i32,
    message: &str,
) -> Result<()> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message}
    });
    writeln!(stdout, "{payload}").context("write stdout")?;
    stdout.flush().context("flush stdout")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_matches_the_documented_vocabulary() {
        let tools = Server::new().tools();
        let verbs = tools["tools"][0]["inputSchema"]["properties"]["verb"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            verbs,
            vec![
                "list_apps",
                "get_app_state",
                "get_app_outline",
                "find_elements",
                "get_subtree",
                "click",
                "set_value",
                "type_text",
                "press_key",
                "scroll",
                "select_text",
                "drag",
                "perform_secondary_action"
            ]
        );
        // Anything not in the schema must be refused rather than ignored.
        assert_eq!(
            tools["tools"][0]["inputSchema"]["additionalProperties"],
            json!(false)
        );
        assert!(!tools.to_string().contains(RECORDER_TOOL));
    }

    #[test]
    fn agent_state_payload_has_no_canonical_tree_escape_hatch() {
        let payload = agent_state_payload(
            "com.example.App",
            7,
            "[9] AXButton \"Changed\"\n".into(),
            2000,
            true,
            Some(24),
            16_000,
        );
        assert!(payload.get("fullText").is_none());
        assert!(payload.get("screenshotPng").is_none());
        assert!(!payload.to_string().contains("Unchanged secret tree row"));
    }

    #[test]
    fn default_diff_is_smaller_than_the_canonical_tree() {
        let canonical = "[0] AXWindow \"Inbox\"\n[1] AXButton \"Send\"\n";
        let tree = ax::AppTree::test_tree(&[(0, "AXWindow", "Inbox"), (1, "AXButton", "Cancel")]);
        let diff = tree.diff_from(canonical);
        assert!(diff.len() < tree.render().len());
        assert!(diff.contains("Cancel"));
        assert!(!diff.contains("Inbox"));
    }

    #[test]
    fn bounded_observations_are_utf8_safe_and_continuable() {
        let text = "alpha 🦀 beta gamma";
        let (first, cursor, truncated) = bounded_text(text, 0, 9);
        assert!(truncated);
        let (second, next, _) = bounded_text(text, cursor.unwrap(), 20);
        assert_eq!(format!("{first}{second}"), text);
        assert_eq!(next, None);
    }

    #[test]
    fn filtered_rendering_preserves_canonical_indexes() {
        let tree =
            ax::AppTree::test_tree(&[(4, "AXButton", "Cancel"), (91, "AXButton", "Publish")]);
        let rendered = render_elements(tree.elements.iter().filter(|element| element.index == 91));
        assert!(rendered.starts_with("[91]"), "{rendered}");
        assert!(!rendered.contains("[0]"));
    }

    #[test]
    fn an_unknown_tool_is_refused_by_name() {
        let error = Server::new()
            .call("something_else", &json!({}))
            .unwrap_err()
            .to_string();
        assert!(error.contains("something_else"), "{error}");
    }

    /// An index the helper never read cannot be resolved, and the message has
    /// to say which app so the agent knows what to re-read.
    #[test]
    fn an_element_index_with_no_prior_read_is_refused_with_the_app_named() {
        let error = Server::new()
            .resolve_element("com.apple.mail", 3)
            .unwrap_err()
            .to_string();
        assert!(error.contains("com.apple.mail"), "{error}");
        assert!(error.contains("no accessibility tree"), "{error}");
    }

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // Bytes above 0x7f must not be mangled.
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn a_missing_numeric_argument_names_the_field() {
        let error = number(&json!({}), "from_x").unwrap_err().to_string();
        assert!(error.contains("from_x"), "{error}");
    }
}
