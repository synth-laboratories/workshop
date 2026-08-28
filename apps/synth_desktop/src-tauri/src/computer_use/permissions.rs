//! The OS grants this plugin needs, and how their state becomes a plugin phase.
//! `docs/COMPUTER_USE.md` §6 step 5, gates G1 and G4.
//!
//! Desktop never calls the TCC APIs itself. Grants bind to the *code identity
//! that asks*, so a grant Desktop holds is not a grant the helper holds. The
//! helper probes and reports; this module owns the catalog, the wording, and
//! the rule that turns states into a phase.

use crate::plugins::types::{PluginPermission, PHASE_NEEDS_PERMISSIONS};
use serde::{Deserialize, Serialize};

pub const ACCESSIBILITY: &str = "accessibility";
pub const SCREEN_RECORDING: &str = "screen_recording";
pub const APPLE_EVENTS: &str = "apple_events";

pub const STATE_GRANTED: &str = "granted";
pub const STATE_DENIED: &str = "denied";
pub const STATE_NOT_DETERMINED: &str = "not_determined";
pub const STATE_NOT_APPLICABLE: &str = "not_applicable";

/// What the helper reports for one grant.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GrantState {
    Granted,
    Denied,
    #[default]
    NotDetermined,
    /// Asked per-target at first use rather than held globally, so there is no
    /// single answer to show. Apple Events is the only one today.
    NotApplicable,
}

impl GrantState {
    pub fn wire(&self) -> &'static str {
        match self {
            Self::Granted => STATE_GRANTED,
            Self::Denied => STATE_DENIED,
            Self::NotDetermined => STATE_NOT_DETERMINED,
            Self::NotApplicable => STATE_NOT_APPLICABLE,
        }
    }

    pub fn from_wire(value: &str) -> Self {
        match value {
            STATE_GRANTED => Self::Granted,
            STATE_DENIED => Self::Denied,
            STATE_NOT_APPLICABLE => Self::NotApplicable,
            _ => Self::NotDetermined,
        }
    }

    /// Whether this state blocks the plugin from reaching `ready`.
    fn blocks_ready(&self) -> bool {
        matches!(self, Self::Denied | Self::NotDetermined)
    }
}

struct GrantSpec {
    id: &'static str,
    /// Apple's own wording. Ours would not be findable in System Settings.
    label: &'static str,
    settings_url: &'static str,
    detail: &'static str,
    /// False for grants asked per-target at first use.
    required_up_front: bool,
}

/// Everything the helper asks the OS for. Order is the order the rows render.
const CATALOG: &[GrantSpec] = &[
    GrantSpec {
        id: ACCESSIBILITY,
        label: "Accessibility",
        settings_url:
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        detail: "Read window contents and send clicks and keystrokes to apps you allow.",
        required_up_front: true,
    },
    GrantSpec {
        id: SCREEN_RECORDING,
        label: "Screen Recording",
        settings_url:
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
        detail: "Capture the before and after image of each action.",
        required_up_front: true,
    },
    GrantSpec {
        id: APPLE_EVENTS,
        label: "Automation",
        settings_url: "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation",
        detail: "Asked once per app, the first time an app is driven.",
        required_up_front: false,
    },
];

/// Build the rows shown in the plugin's permission list.
///
/// `observed` is what the helper last reported. A grant missing from `observed`
/// renders as not determined rather than being hidden: a row that disappears
/// reads as "not needed", which is the opposite of the truth.
pub fn rows(observed: &[(String, GrantState)]) -> Vec<PluginPermission> {
    CATALOG
        .iter()
        .map(|spec| {
            let state = observed
                .iter()
                .find(|(id, _)| id == spec.id)
                .map(|(_, state)| *state)
                .unwrap_or(if spec.required_up_front {
                    GrantState::NotDetermined
                } else {
                    GrantState::NotApplicable
                });
            PluginPermission {
                id: spec.id.to_owned(),
                label: spec.label.to_owned(),
                state: state.wire().to_owned(),
                settings_url: Some(spec.settings_url.to_owned()),
                detail: Some(spec.detail.to_owned()),
            }
        })
        .collect()
}

/// Ids of grants that are required and not yet held. This is what a
/// `PluginNotReady` refusal names, so an agent can tell the operator exactly
/// which switch to flip rather than reporting a generic failure. G4.
pub fn missing(observed: &[(String, GrantState)]) -> Vec<String> {
    CATALOG
        .iter()
        .filter(|spec| spec.required_up_front)
        .filter(|spec| {
            observed
                .iter()
                .find(|(id, _)| id == spec.id)
                .map(|(_, state)| state.blocks_ready())
                .unwrap_or(true)
        })
        .map(|spec| spec.id.to_owned())
        .collect()
}

/// Whether an installed helper is actually usable, or is only installed.
pub fn is_ready(observed: &[(String, GrantState)]) -> bool {
    missing(observed).is_empty()
}

/// Refine an installed-and-running phase with what the OS has actually granted.
///
/// Only phases that would otherwise claim usability are refined. A helper that
/// is downloading or erroring has a more specific truth to report than
/// "needs permission".
pub fn refine_phase(phase: &str, observed: &[(String, GrantState)]) -> String {
    match phase {
        "installed" | "ready" | "starting" if !is_ready(observed) => {
            PHASE_NEEDS_PERMISSIONS.to_owned()
        }
        other => other.to_owned(),
    }
}

/// The System Settings pane for one grant, for the row's button.
pub fn settings_url(permission_id: &str) -> Option<&'static str> {
    CATALOG
        .iter()
        .find(|spec| spec.id == permission_id)
        .map(|spec| spec.settings_url)
}

