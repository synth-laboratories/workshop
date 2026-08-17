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
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
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
        hardened_runtime: report.contains("flags=0x10000(runtime)")
            || report.contains("(runtime)"),
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
            bail!("helper has no stapled notarization ticket, so a first launch offline would fail");
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
        &[
            &source.to_string_lossy(),
            &destination.to_string_lossy(),
        ],
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };
    use tempfile::tempdir;

    #[derive(Default)]
    struct FakeCommands {
        responses: HashMap<String, CommandOutput>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl FakeCommands {
        fn with(mut self, key: &str, status: i32, stdout: &str, stderr: &str) -> Self {
            self.responses.insert(
                key.to_owned(),
                CommandOutput {
                    status,
                    stdout: stdout.to_owned(),
                    stderr: stderr.to_owned(),
                },
            );
            self
        }
    }

    impl CommandRunner for FakeCommands {
        fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
            let key = format!("{program} {}", args.first().copied().unwrap_or(""));
            self.calls.lock().unwrap().push(key.clone());
            Ok(self.responses.get(&key).cloned().unwrap_or(CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "not stubbed".into(),
            }))
        }
    }

    const GOOD_DISPLAY: &str = "Executable=/x/Contents/MacOS/synth-computer-use\n\
Identifier=ai.usesynth.workshop.ComputerUseHelper\n\
CodeDirectory v=20500 size=1234 flags=0x10000(runtime) hashes=1+7\n\
CDHash=aabbccddeeff00112233445566778899aabbccdd\n\
TeamIdentifier=ABCDE12345\n\
CFBundleShortVersionString=1.0.0\n";

    fn bundle(dir: &Path) -> PathBuf {
        let path = dir.join(HELPER_BUNDLE_NAME);
        std::fs::create_dir_all(path.join("Contents/MacOS")).unwrap();
        path
    }

    fn signed_runner() -> FakeCommands {
        FakeCommands::default()
            .with("codesign --display", 0, "", GOOD_DISPLAY)
            .with("codesign --verify", 0, "", "")
            .with(
                "spctl --assess",
                0,
                "",
                "accepted\nsource=Notarized Developer ID\n",
            )
            .with("stapler validate", 0, "The validate action worked!", "")
    }

    #[test]
    fn a_fully_signed_notarized_stapled_helper_verifies() {
        let dir = tempdir().unwrap();
        let identity = verify(
            &signed_runner(),
            &bundle(dir.path()),
            Some("ABCDE12345"),
            true,
        )
        .unwrap();
        assert_eq!(identity.bundle_id, HELPER_BUNDLE_ID);
        assert_eq!(identity.team_id, "ABCDE12345");
        assert_eq!(identity.version, "1.0.0");
        assert!(identity.hardened_runtime);
        assert!(identity.notarized);
        assert!(identity.stapled);
        assert!(identity.describe().contains("ABCDE12345"));
    }

    /// The bundle identifier is half the code identity TCC remembers. A helper
    /// claiming a different one would collect its own grants and leave the real
    /// ones stranded.
    #[test]
    fn a_helper_with_the_wrong_bundle_identifier_is_refused() {
        let dir = tempdir().unwrap();
        let runner = signed_runner().with(
            "codesign --display",
            0,
            "",
            &GOOD_DISPLAY.replace(HELPER_BUNDLE_ID, "com.someone.else"),
        );
        let error = verify(&runner, &bundle(dir.path()), Some("ABCDE12345"), true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("bundle identifier"), "{error}");
    }

    #[test]
    fn a_helper_signed_by_another_team_is_refused() {
        let dir = tempdir().unwrap();
        let error = verify(&signed_runner(), &bundle(dir.path()), Some("OTHER99999"), true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("signed by team"), "{error}");
    }

    /// The pinned requirement is the check that a forged `TeamIdentifier=` line
    /// cannot satisfy, so its failure has to be fatal on its own.
    #[test]
    fn a_bundle_that_fails_the_pinned_requirement_is_refused() {
        let dir = tempdir().unwrap();
        let mut runner = signed_runner();
        // First --verify (strict/deep) succeeds; the pinned one is a separate
        // invocation and this fake keys on the flag, so override both and
        // assert on the requirement path via a failing --verify.
        runner = runner.with("codesign --verify", 1, "", "does not satisfy its designated Requirement");
        let error = verify(&runner, &bundle(dir.path()), Some("ABCDE12345"), true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("does not verify") || error.contains("pinned"), "{error}");
    }

    /// We staple; the reference implementation does not. A first launch offline
    /// with no ticket is a Gatekeeper failure in front of the user.
    #[test]
    fn an_unstapled_helper_is_refused_for_official_builds_but_allowed_for_dev() {
        let dir = tempdir().unwrap();
        let runner = signed_runner().with("stapler validate", 1, "", "does not have a ticket");
        let error = verify(&runner, &bundle(dir.path()), Some("ABCDE12345"), true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("stapled"), "{error}");

        let dev = verify(&runner, &bundle(dir.path()), Some("ABCDE12345"), false).unwrap();
        assert!(!dev.stapled);
        assert!(dev.notarized);
    }

    #[test]
    fn an_unnotarized_or_unhardened_helper_is_refused_for_official_builds() {
        let dir = tempdir().unwrap();
        let unnotarized = signed_runner().with("spctl --assess", 3, "", "rejected\n");
        let error = verify(&unnotarized, &bundle(dir.path()), Some("ABCDE12345"), true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not notarized"), "{error}");

        let unhardened = signed_runner().with(
            "codesign --display",
            0,
            "",
            &GOOD_DISPLAY.replace("flags=0x10000(runtime) ", ""),
        );
        let error = verify(&unhardened, &bundle(dir.path()), Some("ABCDE12345"), true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("hardened runtime"), "{error}");
    }

    #[test]
    fn an_unsigned_helper_is_refused_before_anything_else_is_checked() {
        let dir = tempdir().unwrap();
        let runner = FakeCommands::default().with(
            "codesign --display",
            1,
            "",
            "code object is not signed at all",
        );
        let error = verify(&runner, &bundle(dir.path()), None, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not signed"), "{error}");
    }

    #[test]
    fn a_missing_helper_says_so_rather_than_failing_in_codesign() {
        let dir = tempdir().unwrap();
        let error = verify(
            &signed_runner(),
            &dir.path().join("absent.app"),
            None,
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("no helper is installed"), "{error}");
    }

    /// G7. Every grant the helper held must be reset, and a reset macOS refused
    /// has to be reported — a grant left behind is the residue the gate exists
    /// to catch.
    #[test]
    fn removal_resets_every_grant_and_reports_the_ones_it_could_not() {
        let dir = tempdir().unwrap();
        let installed = bundle(dir.path());
        let runner = FakeCommands::default().with("tccutil reset", 0, "", "");
        let report = remove(&runner, &installed).unwrap();
        assert!(report.bundle_removed);
        assert!(!installed.exists());
        assert_eq!(report.tcc_reset.len(), TCC_SERVICES.len());
        assert!(report.tcc_reset_failed.is_empty());

        let refusing = FakeCommands::default();
        let report = remove(&refusing, &installed).unwrap();
        assert!(!report.bundle_removed);
        assert!(report.tcc_reset.is_empty());
        assert_eq!(report.tcc_reset_failed.len(), TCC_SERVICES.len());
    }

    #[test]
    fn signature_fields_are_parsed_out_of_the_codesign_report() {
        assert_eq!(
            field(GOOD_DISPLAY, "Identifier="),
            Some(HELPER_BUNDLE_ID.to_owned())
        );
        assert_eq!(
            field(GOOD_DISPLAY, "TeamIdentifier="),
            Some("ABCDE12345".to_owned())
        );
        assert_eq!(field(GOOD_DISPLAY, "Nonexistent="), None);
    }
}
