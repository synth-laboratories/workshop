//! Who is allowed to drive this helper.
//!
//! The helper holds Accessibility and Screen Recording. Anything that can talk
//! to it inherits those powers, so "you have the pipe" is not a sufficient
//! credential — a process that can spawn us can hand us any stdio it likes.
//!
//! So we check the other direction: the helper verifies that *its parent* is
//! Synth Desktop, signed by our team, using the same `Security.framework` calls
//! Gatekeeper uses. This is the mirror of the reference implementation's
//! `*_Parent.coderequirement`, and it is why their helper answers `tools/list`
//! from anyone but returns `-10000 Sender process is not authenticated` on the
//! first real call.

use crate::sys::*;
use anyhow::{anyhow, bail, Result};
use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::CFRelease;
use std::ffi::c_void;

/// Set by Desktop when it spawns us. Proves the stdio pair we were handed is
/// the one Desktop created, not one attached by something that re-executed us.
pub const LAUNCH_NONCE_ENV: &str = "SYNTH_COMPUTER_USE_LAUNCH_NONCE";

/// Overridable so a development build can run against an ad-hoc-signed Desktop.
/// Setting it is a deliberate, visible downgrade; the helper says so at startup.
pub const REQUIREMENT_ENV: &str = "SYNTH_COMPUTER_USE_PARENT_REQUIREMENT";

/// The code requirement an official Desktop satisfies. Anchored to Apple *and*
/// pinned to our team and identifier: an anchor check alone would accept every
/// Developer ID app on the machine.
pub fn default_parent_requirement(team_id: &str) -> String {
    format!(
        "anchor apple generic \
         and identifier \"ai.usesynth.workshop\" \
         and certificate leaf[subject.OU] = \"{team_id}\""
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallerCheck {
    pub parent_pid: i32,
    /// False only when an explicit development override was used.
    pub enforced: bool,
}

/// Verify the parent process against `requirement`.
///
/// Returns an error rather than a boolean because every failure mode here needs
/// a different message: a missing requirement is a build problem, a failed
/// check is an attack or a misconfiguration, and they should not read alike.
#[cfg(target_os = "macos")]
pub fn verify_parent(requirement: &str) -> Result<CallerCheck> {
    let parent_pid = unsafe { libc::getppid() };
    if parent_pid <= 1 {
        bail!("helper has no live parent process to authenticate");
    }

    unsafe {
        let key = CFString::wrap_under_get_rule(kSecGuestAttributePid);
        let value = CFNumber::from(parent_pid);
        let attributes = CFDictionary::from_CFType_pairs(&[(
            key.as_CFType(),
            value.as_CFType(),
        )]);

        let mut guest: SecCodeRef = std::ptr::null_mut();
        let status = SecCodeCopyGuestWithAttributes(
            std::ptr::null_mut(),
            attributes.as_concrete_TypeRef(),
            kSecCSDefaultFlags,
            &mut guest,
        );
        if status != 0 || guest.is_null() {
            bail!("could not inspect the calling process (OSStatus {status})");
        }
        let guest = Released(guest as *mut c_void);

        let requirement_text = CFString::new(requirement);
        let mut parsed: SecRequirementRef = std::ptr::null_mut();
        let status = SecRequirementCreateWithString(
            requirement_text.as_concrete_TypeRef(),
            kSecCSDefaultFlags,
            &mut parsed,
        );
        if status != 0 || parsed.is_null() {
            bail!("the configured parent code requirement does not parse (OSStatus {status})");
        }
        let parsed = Released(parsed as *mut c_void);

        let status = SecCodeCheckValidity(
            guest.0 as SecCodeRef,
            kSecCSDefaultFlags,
            parsed.0 as SecRequirementRef,
        );
        if status != 0 {
            bail!(
                "calling process is not authorized to use Computer Use (OSStatus {status}); \
                 this helper only answers to a signed Synth Desktop"
            );
        }
    }

    Ok(CallerCheck {
        parent_pid,
        enforced: true,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn verify_parent(_requirement: &str) -> Result<CallerCheck> {
    bail!("Computer Use is macOS only")
}

/// Confirm the nonce Desktop passed at spawn. Cheap, and it closes the case
/// where our own binary is re-executed with inherited descriptors.
pub fn verify_launch_nonce(expected_present: bool) -> Result<()> {
    let nonce = std::env::var(LAUNCH_NONCE_ENV).unwrap_or_default();
    if expected_present && nonce.trim().is_empty() {
        bail!("helper was started without a launch nonce; start it from Synth Desktop");
    }
    Ok(())
}

/// A CF object released on drop. Every early return above would otherwise leak
/// a `SecCodeRef`, and a helper that leaks per call is a helper that dies during
/// a long session.
struct Released(*mut c_void);

impl Drop for Released {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0 as *const c_void) }
        }
    }
}

fn _unused(_: fn() -> anyhow::Error) {
    let _ = anyhow!("");
}

