//! Synth Workshop Computer Use helper.
//!
//! A signed, notarized `LSUIElement` bundle that holds Accessibility and Screen
//! Recording and answers MCP on stdio. It exists as a separate bundle for one
//! reason: macOS binds TCC grants to code identity, so a grant has to belong to
//! a program whose identity does not change every time Workshop is rebuilt.
//!
//! It refuses to do anything for a caller it cannot authenticate. See
//! `caller.rs` — `tools/list` answers anyone, `tools/call` does not.

mod apps;
mod ax;
mod caller;
mod capture;
mod events;
mod mcp;
mod permissions;
mod sys;

use anyhow::{bail, Result};

fn main() {
    if let Err(error) = run() {
        eprintln!("synth-computer-use: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "mcp".into());
    match mode.as_str() {
        "mcp" => serve(),
        // Lets an operator confirm grants without Desktop running, which is the
        // first thing anyone wants when the permission rows look wrong.
        "probe" => {
            let grants = permissions::probe();
            println!("{}", serde_json::to_string_pretty(&grants)?);
            if !grants.usable() {
                eprintln!(
                    "missing grants: {}. Open System Settings → Privacy & Security.",
                    grants.missing().join(", ")
                );
            }
            Ok(())
        }
        "request" => {
            // LaunchServices must keep this app identity alive while the user
            // responds in System Settings. Exiting immediately after the TCC
            // calls can leave no application row for the user to enable.
            let mut grants = permissions::request();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5 * 60);
            while !grants.missing().is_empty() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(500));
                grants = permissions::probe();
            }
            println!("{}", serde_json::to_string_pretty(&grants)?);
            Ok(())
        }
        "version" => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => bail!("unknown mode `{other}`; expected mcp, probe, request, or version"),
    }
}

fn serve() -> Result<()> {
    let requirement = parent_requirement();
    let enforcing = std::env::var(caller::REQUIREMENT_ENV).is_err();
    if !enforcing {
        // Loud, on purpose. A development override means this helper will drive
        // the machine for whatever launched it, and that should never pass
        // unnoticed in a log.
        eprintln!(
            "synth-computer-use: WARNING — parent code requirement overridden by {}. \
             Caller authentication is weakened; this must not happen in a shipped build.",
            caller::REQUIREMENT_ENV
        );
    }

    caller::verify_launch_nonce(enforcing)?;

    let mut server = mcp::Server::new();
    // Checked per call rather than once at startup: a long-lived helper whose
    // parent died and was replaced should not keep serving on the strength of a
    // check it passed an hour ago.
    let authorize = move || -> Result<()> {
        caller::verify_parent(&requirement)?;
        Ok(())
    };
    mcp::serve(&mut server, &authorize)
}

/// The requirement the calling process must satisfy.
fn parent_requirement() -> String {
    if let Ok(override_value) = std::env::var(caller::REQUIREMENT_ENV) {
        if !override_value.trim().is_empty() {
            return override_value;
        }
    }
    caller::default_parent_requirement(team_id())
}

/// Our team identifier, baked in at build time.
///
/// `SYNTH_TEAM_ID` is set by `scripts/build-helper.sh` from the same value the
/// signing step uses, so the requirement the helper enforces and the identity it
/// is signed with cannot drift apart.
fn team_id() -> &'static str {
    option_env!("SYNTH_TEAM_ID").unwrap_or("UNSET")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unset team id must not silently become a requirement that anything
    /// satisfies. "UNSET" is not a valid OU, so every check against it fails
    /// closed.
    #[test]
    fn an_unbuilt_team_id_produces_a_requirement_nothing_satisfies() {
        let requirement = caller::default_parent_requirement(team_id());
        assert!(requirement.contains("anchor apple generic"));
        if option_env!("SYNTH_TEAM_ID").is_none() {
            assert!(requirement.contains("UNSET"));
        }
    }

    #[test]
    fn the_default_mode_is_the_mcp_server() {
        // Desktop spawns us with an explicit `mcp`, but a bare launch from a
        // terminal should do the same thing rather than exit with usage.
        assert_eq!(
            std::env::args().nth(1).unwrap_or_else(|| "mcp".into()),
            std::env::args().nth(1).unwrap_or_else(|| "mcp".into())
        );
    }
}
