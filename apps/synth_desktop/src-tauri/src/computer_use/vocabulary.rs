//! The agent-facing action vocabulary, normative per `docs/COMPUTER_USE.md` §5.
//!
//! Element-indexed targeting is not decoration: pixel grounding needs a
//! foreground window and a stable cursor, accessibility actions need neither.
//! That is the whole reason background driving works, so the type system treats
//! coordinates as the exception and records every use of them.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A mouse button, named rather than numbered so a wire value cannot silently
/// mean "button 7".
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelectionType {
    #[default]
    Exact,
    Line,
    Paragraph,
    All,
}

/// One action from the vocabulary. Deserialized from the MCP tool call, so the
/// shape here is the contract the agent sees.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum Action {
    ListApps,
    Launch {
        app: String,
    },
    GetAppState {
        app: String,
        /// Force a full tree instead of a diff against the previous read.
        #[serde(default)]
        disable_diff: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_chars: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<u64>,
    },
    GetAppOutline {
        app: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_chars: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<u64>,
    },
    FindElements {
        app: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_chars: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<u64>,
    },
    GetSubtree {
        app: String,
        element_index: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depth: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_chars: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<u64>,
    },
    Click {
        app: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        element_index: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y: Option<f64>,
        #[serde(default)]
        mouse_button: MouseButton,
        #[serde(default = "one")]
        click_count: u32,
    },
    SetValue {
        app: String,
        element_index: u64,
        value: String,
    },
    TypeText {
        app: String,
        text: String,
    },
    PressKey {
        app: String,
        /// xdotool-style keysym. App-scoped, so it cannot invoke a global
        /// shortcut — that is what keeps the operator's session intact.
        key: String,
    },
    Scroll {
        app: String,
        element_index: u64,
        direction: ScrollDirection,
        #[serde(default = "one_page")]
        pages: f64,
    },
    SelectText {
        app: String,
        element_index: u64,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefix: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        suffix: Option<String>,
        #[serde(default)]
        selection_type: SelectionType,
    },
    Drag {
        app: String,
        from_x: f64,
        from_y: f64,
        to_x: f64,
        to_y: f64,
    },
    PerformSecondaryAction {
        app: String,
        element_index: u64,
        /// An action the element actually exposes. Guessed names are refused by
        /// the helper rather than approximated.
        action: String,
    },
}

fn one() -> u32 {
    1
}

fn one_page() -> f64 {
    1.0
}

impl Action {
    pub fn verb(&self) -> &'static str {
        match self {
            Self::ListApps => "list_apps",
            Self::Launch { .. } => "launch",
            Self::GetAppState { .. } => "get_app_state",
            Self::GetAppOutline { .. } => "get_app_outline",
            Self::FindElements { .. } => "find_elements",
            Self::GetSubtree { .. } => "get_subtree",
            Self::Click { .. } => "click",
            Self::SetValue { .. } => "set_value",
            Self::TypeText { .. } => "type_text",
            Self::PressKey { .. } => "press_key",
            Self::Scroll { .. } => "scroll",
            Self::SelectText { .. } => "select_text",
            Self::Drag { .. } => "drag",
            Self::PerformSecondaryAction { .. } => "perform_secondary_action",
        }
    }

    /// Bundle identifier this action targets. `list_apps` targets none.
    pub fn app(&self) -> Option<&str> {
        match self {
            Self::ListApps => None,
            Self::Launch { app }
            | Self::GetAppState { app, .. }
            | Self::GetAppOutline { app, .. }
            | Self::FindElements { app, .. }
            | Self::GetSubtree { app, .. }
            | Self::Click { app, .. }
            | Self::SetValue { app, .. }
            | Self::TypeText { app, .. }
            | Self::PressKey { app, .. }
            | Self::Scroll { app, .. }
            | Self::SelectText { app, .. }
            | Self::Drag { app, .. }
            | Self::PerformSecondaryAction { app, .. } => Some(app),
        }
    }

    /// True when the action only observes. Read-only actions still require the
    /// app to be allowlisted — reading a window is not nothing — but they never
    /// invalidate element indexes.
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::ListApps
                | Self::GetAppState { .. }
                | Self::GetAppOutline { .. }
                | Self::FindElements { .. }
                | Self::GetSubtree { .. }
        )
    }

    /// The element this action targets, when it targets one.
    pub fn element_index(&self) -> Option<u64> {
        match self {
            Self::Click { element_index, .. } => *element_index,
            Self::SetValue { element_index, .. }
            | Self::Scroll { element_index, .. }
            | Self::SelectText { element_index, .. }
            | Self::PerformSecondaryAction { element_index, .. } => Some(*element_index),
            _ => None,
        }
    }

    /// G10. Coordinates are a fallback for canvas-style surfaces — Figma,
    /// Blender, WebGL, custom renderers — not a default. Every use is recorded
    /// so a trajectory can be graded on whether it needed them.
    pub fn uses_coordinates(&self) -> bool {
        match self {
            Self::Click {
                element_index,
                x,
                y,
                ..
            } => element_index.is_none() && (x.is_some() || y.is_some()),
            Self::Drag { .. } => true,
            _ => false,
        }
    }

    /// Rejects requests that are structurally impossible before anything is
    /// sent to the helper, so the failure names the mistake rather than
    /// surfacing as an accessibility error three layers down.
    pub fn validate(&self) -> Result<()> {
        if let Some(app) = self.app() {
            if app.trim().is_empty() {
                bail!("`app` is required and cannot be empty");
            }
        }
        match self {
            Self::Click {
                element_index,
                x,
                y,
                click_count,
                ..
            } => {
                match (element_index, x, y) {
                    (None, None, None) => {
                        bail!("click requires `element_index`, or `x` and `y` together")
                    }
                    (None, Some(_), None) | (None, None, Some(_)) => {
                        bail!("coordinate click requires both `x` and `y`")
                    }
                    (Some(_), Some(_), _) | (Some(_), _, Some(_)) => {
                        bail!("click takes `element_index` or coordinates, not both")
                    }
                    _ => {}
                }
                if *click_count == 0 || *click_count > 3 {
                    bail!("`click_count` must be 1, 2, or 3");
                }
                Ok(())
            }
            Self::TypeText { text, .. } => {
                if text.is_empty() {
                    bail!("`text` cannot be empty");
                }
                Ok(())
            }
            Self::PressKey { key, .. } => {
                if key.trim().is_empty() {
                    bail!("`key` cannot be empty");
                }
                Ok(())
            }
            Self::Scroll { pages, .. } => {
                if !pages.is_finite() || *pages == 0.0 {
                    bail!("`pages` must be a non-zero, finite number");
                }
                Ok(())
            }
            Self::SelectText { text, .. } => {
                if text.is_empty() {
                    bail!("`text` cannot be empty");
                }
                Ok(())
            }
            Self::Drag {
                from_x,
                from_y,
                to_x,
                to_y,
                ..
            } => {
                if ![from_x, from_y, to_x, to_y].iter().all(|v| v.is_finite()) {
                    bail!("drag coordinates must be finite");
                }
                Ok(())
            }
            Self::PerformSecondaryAction { action, .. } => {
                if action.trim().is_empty() {
                    bail!("`action` cannot be empty");
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// What the operator is shown when this action needs consent. Deliberately
    /// concrete: "click element 42" tells them nothing, "click Send" does.
    pub fn approval_payload(&self) -> Value {
        match self {
            Self::ListApps => json!({}),
            Self::Launch { app }
            | Self::GetAppState { app, .. }
            | Self::GetAppOutline { app, .. }
            | Self::FindElements { app, .. }
            | Self::GetSubtree { app, .. } => json!({ "app": app }),
            Self::Click {
                element_index,
                x,
                y,
                mouse_button,
                click_count,
                ..
            } => json!({
                "elementIndex": element_index,
                "x": x,
                "y": y,
                "mouseButton": mouse_button,
                "clickCount": click_count,
            }),
            Self::SetValue {
                element_index,
                value,
                ..
            } => json!({ "elementIndex": element_index, "value": value }),
            Self::TypeText { text, .. } => json!({ "text": text }),
            Self::PressKey { key, .. } => json!({ "key": key }),
            Self::Scroll {
                element_index,
                direction,
                pages,
                ..
            } => json!({ "elementIndex": element_index, "direction": direction, "pages": pages }),
            Self::SelectText {
                element_index,
                text,
                selection_type,
                ..
            } => json!({
                "elementIndex": element_index,
                "text": text,
                "selectionType": selection_type,
            }),
            Self::Drag {
                from_x,
                from_y,
                to_x,
                to_y,
                ..
            } => json!({ "fromX": from_x, "fromY": from_y, "toX": to_x, "toY": to_y }),
            Self::PerformSecondaryAction {
                element_index,
                action,
                ..
            } => json!({ "elementIndex": element_index, "action": action }),
        }
    }
}

/// Every verb, in the order §5 lists them. The MCP schema and the skill are
/// both generated from this, so a verb cannot be advertised in one and missing
/// from the other.
pub const ACTION_VERBS: [&str; 14] = [
    "list_apps",
    "launch",
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
    "perform_secondary_action",
];

