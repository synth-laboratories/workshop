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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn action(value: serde_json::Value) -> Action {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn terminals_are_denied_by_id_and_by_name() {
        for id in [
            "com.apple.Terminal",
            "com.googlecode.iterm2",
            "com.mitchellh.ghostty",
            "io.alacritty",
            "com.example.MyTerminalEmulator",
        ] {
            assert_eq!(
                classify_app(id).denial(),
                Some(DenialReason::Terminal),
                "{id} should be terminal-class"
            );
        }
    }

    #[test]
    fn credential_stores_and_security_settings_are_denied_too() {
        assert_eq!(
            classify_app("com.apple.keychainaccess").denial(),
            Some(DenialReason::CredentialStore)
        );
        assert_eq!(
            classify_app("com.1password.1password").denial(),
            Some(DenialReason::CredentialStore)
        );
        assert_eq!(
            classify_app("com.apple.systempreferences").denial(),
            Some(DenialReason::SecuritySettings)
        );
    }

    #[test]
    fn ordinary_apps_are_not_denied_and_unknown_apps_are_ordinary() {
        assert!(classify_app("com.apple.mail").denial().is_none());
        assert!(classify_app("com.figma.desktop").denial().is_none());
        assert!(classify_app("com.nobody.never-heard-of-it")
            .denial()
            .is_none());
    }

    #[test]
    fn reading_is_never_a_hazard() {
        assert!(hazard(
            "com.apple.mail",
            &action(json!({"verb":"get_app_state","app":"com.apple.mail"})),
            Some("Send")
        )
        .is_none());
    }

    #[test]
    fn an_irreversible_control_is_a_hazard_in_any_app() {
        let click = action(json!({"verb":"click","app":"com.figma.desktop","element_index":3}));
        assert!(matches!(
            hazard("com.figma.desktop", &click, Some("Delete Page")),
            Some(HazardReason::IrreversibleControl { .. })
        ));
        assert!(hazard("com.figma.desktop", &click, Some("Zoom In")).is_none());
    }

    /// "Sender" is a column header in every mail client. Treating it as a send
    /// button would make the hazard card fire on ordinary navigation, and a
    /// card that always fires is a card nobody reads.
    #[test]
    fn a_label_that_merely_contains_a_hazard_word_is_not_one() {
        let click = action(json!({"verb":"click","app":"com.apple.mail","element_index":3}));
        assert!(hazard("com.apple.mail", &click, Some("Sender")).is_none());
        assert!(hazard("com.apple.mail", &click, Some("Suspended")).is_none());
        assert!(matches!(
            hazard("com.apple.mail", &click, Some("Send")),
            Some(HazardReason::IrreversibleControl { .. })
        ));
    }

    /// In a mail client there is no Send button to catch — Return sends.
    #[test]
    fn return_in_a_communication_app_is_a_send() {
        let key = action(json!({"verb":"press_key","app":"com.apple.mail","key":"Return"}));
        assert_eq!(
            hazard("com.apple.mail", &key, None),
            Some(HazardReason::SendsToOthers)
        );
        let elsewhere =
            action(json!({"verb":"press_key","app":"com.figma.desktop","key":"Return"}));
        assert!(hazard("com.figma.desktop", &elsewhere, None).is_none());
    }

    #[test]
    fn typing_into_a_draft_is_not_itself_a_hazard() {
        let typing = action(json!({"verb":"type_text","app":"com.apple.mail","text":"hello"}));
        assert!(hazard("com.apple.mail", &typing, None).is_none());
    }
}
