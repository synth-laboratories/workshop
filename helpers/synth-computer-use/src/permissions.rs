//! Probing and requesting the grants this helper needs.
//!
//! These calls only ever answer for *this* process. That is the whole reason
//! the helper exists as a separate signed bundle: a grant Desktop holds is not
//! a grant the helper holds, because TCC keys on code identity.

use crate::sys::*;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GrantState {
    Granted,
    Denied,
    NotDetermined,
    NotApplicable,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Grants {
    pub accessibility: GrantState,
    pub screen_recording: GrantState,
    /// Asked per app at first drive, so there is no global answer.
    pub apple_events: GrantState,
}

impl Grants {
    /// Whether the helper can do its job at all.
    pub fn usable(&self) -> bool {
        self.accessibility == GrantState::Granted
    }

    /// Ids of the grants that are missing, matching Desktop's permission ids.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.accessibility != GrantState::Granted {
            missing.push("accessibility");
        }
        if self.screen_recording != GrantState::Granted {
            missing.push("screen_recording");
        }
        missing
    }
}

/// Read current grant state without prompting.
///
/// Never prompts. A probe that raised a system dialog would make Desktop's
/// permission rows pop a modal every time they refreshed.
#[cfg(target_os = "macos")]
pub fn probe() -> Grants {
    let accessibility = if unsafe { AXIsProcessTrustedWithOptions(std::ptr::null()) } != 0 {
        GrantState::Granted
    } else {
        // macOS does not distinguish "never asked" from "denied" here. Reporting
        // NotDetermined keeps the UI honest: it says "grant this", not "you
        // denied this", and the first is true in both cases.
        GrantState::NotDetermined
    };
    let screen_recording = if unsafe { CGPreflightScreenCaptureAccess() } {
        GrantState::Granted
    } else {
        GrantState::NotDetermined
    };
    Grants {
        accessibility,
        screen_recording,
        apple_events: GrantState::NotApplicable,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn probe() -> Grants {
    Grants {
        accessibility: GrantState::NotApplicable,
        screen_recording: GrantState::NotApplicable,
        apple_events: GrantState::NotApplicable,
    }
}

/// Ask macOS to show the grant prompts. Called only from the permission wizard,
/// never from a tool call: a prompt that appears mid-task is a prompt the
/// operator clicks away without reading.
#[cfg(target_os = "macos")]
pub fn request() -> Grants {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let options = CFDictionary::from_CFType_pairs(&[(
            key.as_CFType(),
            CFBoolean::true_value().as_CFType(),
        )]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef());
        CGRequestScreenCaptureAccess();
    }
    probe()
}

#[cfg(not(target_os = "macos"))]
pub fn request() -> Grants {
    probe()
}

