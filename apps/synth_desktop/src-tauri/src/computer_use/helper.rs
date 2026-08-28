//! Locating, verifying, installing, and removing the signed helper.
//! `docs/COMPUTER_USE.md` §4 item 5, gates G1, G2, G7.
//!
//! Everything here exists because macOS binds TCC grants to **code identity**,
//! not to a path. A helper that is replaced by a differently-signed binary at
//! the same path is a different program as far as the OS is concerned, and a
//! helper we launch without checking its identity is whatever happens to be
//! sitting at that path. So: verify before launch, every launch.
//!
//! Verification shells out to the system tools rather than reimplementing
//! `Security.framework`, because `codesign` and `spctl` are the same authority
//! Gatekeeper consults. The command runner is injected so the parsing — which
//! is where the bugs live — is testable without a signed bundle.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Team identifier every official build must carry. A helper signed by anyone
/// else is refused even if macOS would happily run it.
pub const EXPECTED_TEAM_ID_ENV: &str = "SYNTH_COMPUTER_USE_TEAM_ID";

/// Bundle identifier. Stable across versions on purpose: it is half of the code
/// identity TCC remembers, so changing it silently revokes every grant.
pub const HELPER_BUNDLE_ID: &str = "ai.usesynth.workshop.ComputerUseHelper";

pub const HELPER_BUNDLE_NAME: &str = "Synth Computer Use.app";

/// What the system tools say about a bundle.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HelperIdentity {
    pub bundle_id: String,
    pub team_id: String,
    pub version: String,
    /// The hash TCC keys grants on. Logged so a lost grant can be explained
    /// rather than guessed at.
    pub cdhash: String,
    /// Hardened runtime. Notarization is refused without it.
    pub hardened_runtime: bool,
    /// Gatekeeper accepted it as notarized.
    pub notarized: bool,
    /// The ticket is stapled to the bundle, so first launch works offline.
    /// The reference implementation ships unstapled; we do not.
    pub stapled: bool,
}

impl HelperIdentity {
    /// A one-line identity for the trajectory record.
    pub fn describe(&self) -> String {
        format!("{} ({}) {}", self.bundle_id, self.team_id, self.version)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn ok(&self) -> bool {
        self.status == 0
    }

    /// `codesign` writes its report to stderr; `spctl` splits across both.
    fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput>;
}

pub struct SystemCommands;

impl CommandRunner for SystemCommands {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        let output = Command::new(program)
            .args(args)
            .output()
            .with_context(|| format!("run {program}"))?;
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Where the helper lives once installed.
pub fn install_root() -> PathBuf {
    crate::storage::app_data_root().join("computer-use")
}

pub fn helper_bundle_path() -> PathBuf {
    install_root().join(HELPER_BUNDLE_NAME)
}

/// The executable inside the bundle.
pub fn helper_executable(bundle: &Path) -> PathBuf {
    bundle.join("Contents/MacOS/synth-computer-use")
}

/// The team id an official build must carry.
///
/// Overridable by environment for development builds only. That is a real hole
/// and it is named as one: a developer running an unsigned helper is running
/// with their own TCC grants on their own machine, and `verify` still refuses
/// to call such a build notarized.
pub fn expected_team_id() -> Option<String> {
    std::env::var(EXPECTED_TEAM_ID_ENV)
        .ok()
        .or_else(|| option_env!("SYNTH_COMPUTER_USE_TEAM_ID").map(str::to_owned))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Return the exact designated requirement of the running Workshop binary.
/// This is the parent authority for an unnotarized friends build: it is weaker
/// than an Apple Team requirement but still binds the helper to these exact
/// immutable parent bytes instead of accepting any process with our bundle id.
pub fn current_process_designated_requirement() -> Result<String> {
    let executable = std::env::current_exe().context("locate running Workshop executable")?;
    let output = Command::new("codesign")
        .args(["-d", "-r-"])
        .arg(&executable)
        .output()
        .context("read Workshop designated requirement")?;
    if !output.status.success() {
        bail!(
            "running Workshop executable has no verifiable designated requirement: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let report = String::from_utf8_lossy(&output.stderr);
    report
        .lines()
        .find_map(|line| {
            line.split_once("designated => ")
                .map(|(_, value)| value.trim())
        })
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("codesign returned no designated requirement for Workshop"))
}

/// Full verification. Refuses on the first failure with a message that names
/// which check failed, because "signature invalid" sends people to the wrong
/// place about half the time.
pub fn verify(
    runner: &dyn CommandRunner,
    bundle: &Path,
    expected_team: Option<&str>,
    require_notarized: bool,
) -> Result<HelperIdentity> {
    if !bundle.exists() {
        bail!("no helper is installed at {}", bundle.display());
    }
    let path = bundle.to_string_lossy().into_owned();

    let display = runner.run("codesign", &["--display", "--verbose=4", &path])?;
    if !display.ok() {
        bail!(
            "helper is not signed at all: {}",
            display.stderr.trim().lines().next().unwrap_or("no detail")
        );
    }
    let report = display.combined();
    let identity = HelperIdentity {
        bundle_id: field(&report, "Identifier=")
            .ok_or_else(|| anyhow!("signature report carries no bundle identifier"))?,
        team_id: field(&report, "TeamIdentifier=")
            .filter(|value| value != "not set")
            .unwrap_or_default(),
        version: field(&report, "CFBundleShortVersionString=").unwrap_or_else(|| "0".into()),
        cdhash: field(&report, "CDHash=").unwrap_or_default(),
        hardened_runtime: report.contains("flags=0x10000(runtime)") || report.contains("(runtime)"),
        notarized: false,
        stapled: false,
    };

    if identity.bundle_id != HELPER_BUNDLE_ID {
        bail!(
            "helper bundle identifier is `{}`, expected `{HELPER_BUNDLE_ID}` — a different identifier is a different program to TCC",
            identity.bundle_id
        );
    }

    let verified = runner.run("codesign", &["--verify", "--strict", "--deep", &path])?;
    if !verified.ok() {
        bail!(
            "helper signature does not verify: {}",
            verified.stderr.trim().lines().next().unwrap_or("no detail")
        );
    }

    if let Some(team) = expected_team {
        if identity.team_id != team {
            bail!(
                "helper is signed by team `{}`, expected `{team}`",
                if identity.team_id.is_empty() {
                    "none"
                } else {
                    &identity.team_id
                }
            );
        }
        // Belt and braces: pin the requirement so a bundle that merely *reports*
        // the right team but is not anchored to Apple still fails.
        let requirement =
            format!("anchor apple generic and certificate leaf[subject.OU] = \"{team}\"");
        let pinned = runner.run("codesign", &["--verify", "-R", &requirement, &path])?;
        if !pinned.ok() {
            bail!("helper does not satisfy the pinned code requirement for team `{team}`");
        }
    }

    let assessed = runner.run("spctl", &["--assess", "--type", "execute", "-vv", &path])?;
    let assessment = assessed.combined();
    let identity = HelperIdentity {
        notarized: assessed.ok() && assessment.contains("source=Notarized Developer ID"),
        stapled: runner
            .run("stapler", &["validate", &path])
            .map(|output| output.ok())
            .unwrap_or(false),
        ..identity
    };

    if require_notarized {
        if !identity.hardened_runtime {
            bail!("helper is not built with the hardened runtime, so it cannot be notarized");
        }
        if !identity.notarized {
            bail!(
                "helper is not notarized: {}",
                assessment.trim().lines().last().unwrap_or("no detail")
            );
        }
        // The reference suite ships unstapled and relies on Gatekeeper's online
        // check. We staple, so a first launch offline does not fail.
        if !identity.stapled {
            bail!(
                "helper has no stapled notarization ticket, so a first launch offline would fail"
            );
        }
    }

    Ok(identity)
}

/// Place a verified bundle at the install path.
///
/// Verification happens on the *source*, before anything is moved, so a bundle
/// that fails is never briefly present at the path the helper is launched from.
pub fn install(
    runner: &dyn CommandRunner,
    source: &Path,
    destination: &Path,
    expected_team: Option<&str>,
    require_notarized: bool,
) -> Result<HelperIdentity> {
    let identity = verify(runner, source, expected_team, require_notarized)?;
    let _ = &identity;
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).context("create computer-use install root")?;
    }
    if destination.exists() {
        std::fs::remove_dir_all(destination).context("remove previous helper")?;
    }
    let copy = runner.run(
        "ditto",
        &[&source.to_string_lossy(), &destination.to_string_lossy()],
    )?;
    if !copy.ok() {
        bail!("failed to install helper: {}", copy.stderr.trim());
    }
    // Re-verify at the destination: `ditto` preserves signatures, and an
    // install that silently broke one would show up as a mysterious launch
    // failure much later.
    verify(runner, destination, expected_team, require_notarized)
}

/// What `remove` did, for the receipt. G7.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RemovalReport {
    pub bundle_removed: bool,
    /// TCC services whose grant was reset.
    pub tcc_reset: Vec<String>,
    /// Reset attempts macOS refused. Reported rather than swallowed: a grant
    /// left behind is exactly the uninstall residue G7 exists to catch.
    pub tcc_reset_failed: Vec<String>,
    /// u32 rather than usize: specta forbids BigInt-style types across the
    /// bridge, and nobody has more than four billion allowlist entries.
    pub allowlist_entries_removed: u32,
}

/// TCC service names, as `tccutil` spells them.
const TCC_SERVICES: &[&str] = &["Accessibility", "ScreenCapture", "AppleEvents"];

/// Delete the helper and reset every grant it held.
///
/// Uninstall residue is the standard failure of automation tools: the app is
/// gone from Applications but its entry sits in Privacy & Security forever,
/// and the next install silently inherits a grant nobody re-consented to.
pub fn remove(runner: &dyn CommandRunner, bundle: &Path) -> Result<RemovalReport> {
    let bundle_removed = if bundle.exists() {
        std::fs::remove_dir_all(bundle).context("remove helper bundle")?;
        true
    } else {
        false
    };
    let mut tcc_reset = Vec::new();
    let mut tcc_reset_failed = Vec::new();
    for service in TCC_SERVICES {
        match runner.run("tccutil", &["reset", service, HELPER_BUNDLE_ID]) {
            Ok(output) if output.ok() => tcc_reset.push((*service).to_owned()),
            _ => tcc_reset_failed.push((*service).to_owned()),
        }
    }
    Ok(RemovalReport {
        bundle_removed,
        tcc_reset,
        tcc_reset_failed,
        allowlist_entries_removed: 0,
    })
}

/// Pull `Key=value` out of a `codesign --display` report.
fn field(report: &str, key: &str) -> Option<String> {
    report.lines().find_map(|line| {
        line.trim()
            .strip_prefix(key)
            .map(|value| value.trim().to_owned())
    })
}

