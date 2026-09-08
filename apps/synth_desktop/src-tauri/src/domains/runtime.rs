//! Public runtime lifecycle contract. Window/process effects belong to the
//! platform adapter; they do not create another storage or execution owner.
use crate::contract::capabilities::PublicOperation;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusRequest {}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub process_id: u32,
    pub boot_id: String,
    pub desktop_attached: bool,
    pub headless_supported: bool,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ControlAction {
    Attach,
    Detach,
    Stop,
}

impl ControlAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Attach => "attach",
            Self::Detach => "detach",
            Self::Stop => "stop",
        }
    }
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub action: ControlAction,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ControlResult {
    pub action: ControlAction,
    pub requested: bool,
    pub runtime: RuntimeStatus,
}

pub struct Status;
impl PublicOperation for Status {
    type Request = StatusRequest;
    type Response = RuntimeStatus;
    const ID: &'static str = "runtime.status.v1";
    const MCP_NAME: &'static str = "runtime_status";
    const DESCRIPTION: &'static str = "Read the connected runtime process and boot identity and whether a desktop view is attached. Closing the desktop does not stop this runtime.";
    const READ_ONLY: bool = true;
    const DESTRUCTIVE: bool = false;
    const IDEMPOTENT: bool = true;
}

pub struct Control;
impl PublicOperation for Control {
    type Request = ControlRequest;
    type Response = ControlResult;
    const ID: &'static str = "runtime.control.v1";
    const MCP_NAME: &'static str = "runtime_control";
    const DESCRIPTION: &'static str = "Attach or detach the desktop view, or explicitly stop the connected runtime and its managed services. Stop interrupts running work and disconnects all clients. An acknowledgement means the lifecycle request was accepted; poll status to verify attachment or disconnection.";
    const READ_ONLY: bool = false;
    const DESTRUCTIVE: bool = true;
    const IDEMPOTENT: bool = true;
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaptureScope {
    App,
    Plugin,
    Visual,
    Element,
}
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureRequest {
    pub scope: CaptureScope,
    pub target: Option<String>,
}
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct CaptureResult {
    pub receipt: serde_json::Value,
}
pub struct Capture;
impl PublicOperation for Capture {
    type Request = CaptureRequest;
    type Response = CaptureResult;
    const ID: &'static str = "runtime.capture.v1";
    const MCP_NAME: &'static str = "app_capture";
    const DESCRIPTION: &'static str = "Attach the desktop when needed and capture its actual native pixels with app state and audit receipt. Scope app preserves navigation; plugin navigates to a destination; visual isolates a visual; element crops a data-testid. Target is required except for app. Inspect the returned PNG before making visual claims.";
    const READ_ONLY: bool = false;
    const DESTRUCTIVE: bool = false;
    const IDEMPOTENT: bool = true;
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum Destination {
    Landing, Connectors, Inventory, Inference, Plugins, Visuals, Optimizers,
    EnvironmentQa, ComputerUse,
    Settings { section: Option<SettingsSection> },
    Chat { chat_id: String },
    Sync { session_id: String },
    Async { session_id: String },
    Reports { report_id: Option<String> },
    Experiments { experiment_id: Option<String> },
}
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SettingsSection { General, Models, Inference, Context, Voice, Plugins, Account, Secrets, About }
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresentRequest { pub destination: Destination }
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresentResult { pub request_id: String, pub requested: bool }
pub struct Present;
impl PublicOperation for Present {
    type Request = PresentRequest; type Response = PresentResult;
    const ID: &'static str = "runtime.present.v1";
    const MCP_NAME: &'static str = "app_present";
    const DESCRIPTION: &'static str = "Attach the desktop and request navigation to an existing Workshop page or task. Does not start a task or approve anything. Returns requested, not proof of rendering; inspect app_capture afterward. For a specific visual use visual_present.";
    const READ_ONLY: bool = false; const DESTRUCTIVE: bool = false; const IDEMPOTENT: bool = true;
}
