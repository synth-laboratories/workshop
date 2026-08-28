//! What may be driven at all, and what may only be driven with a person
//! watching. See `docs/COMPUTER_USE.md` §3 (G5, G6) and §7.
//!
//! Two independent judgments live here and must not be conflated:
//!
//! * **App class** answers "may this app be driven?" and is a hard policy. No
//!   approval, no scope, and no `approval_policy` setting unlocks a denied app.
//! * **Hazard** answers "may this particular action run without a person?" and
//!   is what `ApprovalKind::requires_human` keys on.

use super::vocabulary::Action;
use serde::{Deserialize, Serialize};

/// Why an app may never be driven.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DenialReason {
    /// Terminal emulators. Driving one is arbitrary code execution that routes
    /// around the shell-approval path entirely — the agent would be typing
    /// commands into a shell that Workshop never sees, let alone gates.
    Terminal,
    /// Credential stores. Reading a password out of a window is exfiltration
    /// no matter how ordinary the click looked.
    CredentialStore,
    /// Privacy and security settings. An agent that can drive these can grant
    /// itself the permissions it was refused, which makes every other control
    /// here decorative.
    SecuritySettings,
}

impl DenialReason {
    /// Shown to the agent verbatim, so it stops rather than trying variants.
    pub fn explain(&self) -> &'static str {
        match self {
            Self::Terminal => {
                "Terminal-class apps cannot be driven: typing into a shell would bypass command approval entirely. Use the shell tool, which is gated."
            }
            Self::CredentialStore => {
                "Credential stores cannot be driven. Ask the operator for what you need."
            }
            Self::SecuritySettings => {
                "Privacy and security settings cannot be driven, because an agent that can change them can grant itself anything it was refused."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AppClass {
    Ordinary,
    Denied(DenialReason),
}

impl AppClass {
    pub fn denial(&self) -> Option<DenialReason> {
        match self {
            Self::Denied(reason) => Some(*reason),
            Self::Ordinary => None,
        }
    }
}

/// Terminal emulators by bundle id. The substring fallback below catches the
/// ones not listed; this table exists so the common cases are exact rather
/// than inferred.
const TERMINAL_BUNDLE_IDS: &[&str] = &[
    "com.apple.terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "dev.warp.warp-stable",
    "dev.warp.warp",
    "io.alacritty",
    "net.kovidgoyal.kitty",
    "com.github.wez.wezterm",
    "co.zeit.hyper",
    "com.tabby.app",
    "org.tabby",
    "com.apple.scripteditor2",
    "com.apple.automator",
];

/// Substrings that make an app terminal-class regardless of vendor. Matched
/// against the lowercased bundle id.
const TERMINAL_MARKERS: &[&str] = &["terminal", "iterm", "shell", "console", "tty"];

const CREDENTIAL_BUNDLE_IDS: &[&str] = &[
    "com.apple.keychainaccess",
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.agilebits.onepassword",
    "com.bitwarden.desktop",
    "com.lastpass.lastpassmacdesktop",
    "com.dashlane.dashlanephonefinal",
];

const SECURITY_SETTINGS_BUNDLE_IDS: &[&str] = &[
    "com.apple.systempreferences",
    "com.apple.preference.security",
    "com.apple.settings.privacysecurity",
];

/// Classify a bundle id. Unknown apps are ordinary — the allowlist, not this
/// function, is what keeps an agent out of an app the operator never approved.
pub fn classify_app(bundle_id: &str) -> AppClass {
    let id = bundle_id.trim().to_ascii_lowercase();
    if TERMINAL_BUNDLE_IDS.contains(&id.as_str())
        || TERMINAL_MARKERS.iter().any(|marker| id.contains(marker))
    {
        return AppClass::Denied(DenialReason::Terminal);
    }
    if CREDENTIAL_BUNDLE_IDS.contains(&id.as_str())
        || id.contains("keychain")
        || id.contains("password")
    {
        return AppClass::Denied(DenialReason::CredentialStore);
    }
    if SECURITY_SETTINGS_BUNDLE_IDS.contains(&id.as_str()) {
        return AppClass::Denied(DenialReason::SecuritySettings);
    }
    AppClass::Ordinary
}

/// Apps where committing content sends it to other people. Used only to widen
/// hazard detection, never to deny.
const COMMUNICATION_MARKERS: &[&str] = &[
    "com.apple.mail",
    "com.apple.ichat",
    "com.apple.messages",
    "com.tinyspeck.slackmacgap",
    "com.hnc.discord",
    "com.microsoft.outlook",
    "com.microsoft.teams",
    "com.readdle.smartemail",
    "com.superhuman",
    "org.whispersystems.signal-desktop",
    "ru.keepcoder.telegram",
    "net.whatsapp.whatsapp",
    "com.zoom.xos",
];

/// Element labels that mean the effect leaves the machine or cannot be undone
/// from inside the app.
///
/// This list over-matches on purpose. A false positive costs one click; a false
/// negative sends mail on someone's behalf without asking. It is a safety net,
/// not a classifier, and it is not the only defense — the operator still sees
/// the payload on every hazard card.
const IRREVERSIBLE_LABELS: &[&str] = &[
    "send",
    "submit",
    "post",
    "publish",
    "tweet",
    "reply all",
    "pay",
    "purchase",
    "buy",
    "checkout",
    "place order",
    "transfer",
    "confirm",
    "delete",
    "move to trash",
    "erase",
    "empty trash",
    "archive all",
    "sign",
    "accept",
    "approve",
    "merge",
    "deploy",
];

/// Why an action needs a person, in words the card can show.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HazardReason {
    /// The element's own label says the effect is irreversible.
    IrreversibleControl { label: String },
    /// Committing in a communication app puts content in front of other people.
    SendsToOthers,
}

/// Decide whether an action is hazard-class.
///
/// `element_label` comes from the last accessibility read, not from the agent:
/// self-declared risk would be worth nothing. `None` means the last read did
/// not name the element, which is why the communication-app rules below do not
/// depend on having a label.
pub fn hazard(app: &str, action: &Action, element_label: Option<&str>) -> Option<HazardReason> {
    if action.is_read_only() {
        return None;
    }
    let app_id = app.trim().to_ascii_lowercase();
    let communicates = COMMUNICATION_MARKERS
        .iter()
        .any(|marker| app_id == *marker || app_id.starts_with(marker));

    if let Some(label) = element_label {
        let normalized = label.trim().to_ascii_lowercase();
        if let Some(hit) = IRREVERSIBLE_LABELS
            .iter()
            .find(|needle| label_matches(&normalized, needle))
        {
            return Some(HazardReason::IrreversibleControl {
                label: (*hit).to_owned(),
            });
        }
    }

    if communicates {
        match action {
            // Return in a message compose field is how a message is sent; there
            // is no button click to catch.
            Action::PressKey { key, .. } => {
                let key = key.trim().to_ascii_lowercase();
                if matches!(key.as_str(), "return" | "enter" | "kp_enter") {
                    return Some(HazardReason::SendsToOthers);
                }
            }
            Action::PerformSecondaryAction { action: name, .. } => {
                let name = name.to_ascii_lowercase();
                if name.contains("send") || name.contains("confirm") {
                    return Some(HazardReason::SendsToOthers);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whole-word-ish match so "Send" fires and "Resend later" does too, while
/// "Sender" alone in a column header does not turn every click into a hazard.
fn label_matches(label: &str, needle: &str) -> bool {
    if label == needle {
        return true;
    }
    label
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .windows(needle.split(' ').count())
        .any(|window| window.join(" ") == needle)
}

