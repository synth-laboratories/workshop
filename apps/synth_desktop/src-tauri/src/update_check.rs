//! Passive release-version check.
//!
//! v0.1 deliberately ships without an in-app updater: this module only reads a
//! per-channel manifest, compares versions, and lets the UI offer the public
//! download page. The manifest is never trusted for anything but the version
//! number — the download action always opens the fixed public page, so a
//! poisoned manifest cannot redirect the install source. Network failure means
//! "no update known", never an error surface.

use serde::{Deserialize, Serialize};

/// Baked at build time. Channels are the maturity tiers: the compiled
/// envelope (release_tier::BUILD_TIER) names the manifest a build reads, so a
/// beta app checks `/releases/beta/latest.json` without a second knob and
/// tier apps with separate bundle identifiers never cross channels. The env
/// override remains for special lines that intentionally diverge.
pub const CHANNEL: &str = match option_env!("SYNTH_DESKTOP_CHANNEL") {
    Some(channel) => channel,
    None => crate::release_tier::BUILD_TIER.name(),
};

/// The one place an update can send a person. Fixed on purpose — see module
/// docs.
pub const DOWNLOAD_PAGE: &str = "https://usesynth.ai/download";

const DEFAULT_MANIFEST_BASE: &str = "https://usesynth.ai/releases";

#[derive(Debug, Clone, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub channel: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
}

fn manifest_url(base: &str) -> String {
    format!("{}/{CHANNEL}/latest.json", base.trim_end_matches('/'))
}

/// The runtime override exists for development and deterministic tests; it
/// only moves where the version number is read from, never where the
/// download button points.
fn manifest_base() -> String {
    std::env::var("SYNTH_DESKTOP_UPDATE_MANIFEST_BASE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MANIFEST_BASE.into())
}

pub async fn status() -> UpdateStatus {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let latest = fetch_latest_version(&manifest_url(&manifest_base())).await;
    let update_available = latest
        .as_deref()
        .is_some_and(|latest| version_is_newer(latest, &current));
    UpdateStatus {
        current_version: current,
        channel: CHANNEL.into(),
        latest_version: latest,
        update_available,
    }
}

async fn fetch_latest_version(url: &str) -> Option<String> {
    let client = crate::http::http_client_with_timeout(crate::limits::UPDATE_MANIFEST_TIMEOUT);
    let manifest = client
        .get(url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json::<Manifest>()
        .await
        .ok()?;
    let version = manifest.version.trim();
    (!version.is_empty()).then(|| version.to_owned())
}

/// Semver-shaped comparison: dotted numeric core, then prerelease, where a
/// release outranks any prerelease of the same core and prereleases compare
/// segment-wise (numeric segments numerically, otherwise bytewise). Anything
/// unparsable is treated as not-newer — an unreadable manifest must never
/// produce an update nag.
fn version_is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum PrereleaseSegment {
    Number(u64),
    Text(String),
}

fn parse_version(value: &str) -> Option<(Vec<u64>, PrereleaseRank)> {
    let value = value.trim().strip_prefix('v').unwrap_or(value.trim());
    let (core, prerelease) = match value.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (value, None),
    };
    let core = core
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<u64>>>()?;
    if core.is_empty() {
        return None;
    }
    let prerelease = match prerelease {
        // A release sorts after every prerelease of the same core.
        None => PrereleaseRank::Release,
        Some(prerelease) => PrereleaseRank::Prerelease(
            prerelease
                .split('.')
                .map(|segment| match segment.parse::<u64>() {
                    Ok(number) => PrereleaseSegment::Number(number),
                    Err(_) => PrereleaseSegment::Text(segment.to_owned()),
                })
                .collect(),
        ),
    };
    Some((core, prerelease))
}

#[derive(PartialEq, Eq)]
enum PrereleaseRank {
    Prerelease(Vec<PrereleaseSegment>),
    Release,
}

impl PartialOrd for PrereleaseRank {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrereleaseRank {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (Self::Release, Self::Release) => Ordering::Equal,
            (Self::Release, Self::Prerelease(_)) => Ordering::Greater,
            (Self::Prerelease(_), Self::Release) => Ordering::Less,
            (Self::Prerelease(a), Self::Prerelease(b)) => a.cmp(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_channel_is_stable_and_shapes_the_manifest_url() {
        assert_eq!(CHANNEL, "stable");
        assert_eq!(
            manifest_url("https://usesynth.ai/releases"),
            "https://usesynth.ai/releases/stable/latest.json"
        );
        assert_eq!(
            manifest_url("http://127.0.0.1:9/releases/"),
            "http://127.0.0.1:9/releases/stable/latest.json"
        );
    }

    #[test]
    fn newer_versions_are_recognized_and_garbage_never_is() {
        assert!(version_is_newer("0.1.1", "0.1.0"));
        assert!(version_is_newer("0.2.0", "0.1.9"));
        assert!(version_is_newer("v1.0.0", "0.9.9"));
        assert!(!version_is_newer("0.1.0", "0.1.0"));
        assert!(!version_is_newer("0.0.9", "0.1.0"));
        assert!(!version_is_newer("not-a-version", "0.1.0"));
        assert!(!version_is_newer("", "0.1.0"));
        assert!(!version_is_newer("1.0.0", "garbage"));
    }

    #[test]
    fn a_release_outranks_its_own_prereleases_and_prereleases_order_by_date() {
        assert!(version_is_newer("0.2.0", "0.2.0-nightly.20260811"));
        assert!(!version_is_newer("0.2.0-nightly.20260811", "0.2.0"));
        assert!(version_is_newer(
            "0.2.0-nightly.20260812",
            "0.2.0-nightly.20260811"
        ));
        assert!(version_is_newer("0.2.0-nightly.20260811", "0.1.0"));
    }

    #[tokio::test]
    async fn an_unreachable_manifest_reads_as_no_update() {
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/stable/latest.json", dead.local_addr().unwrap());
        drop(dead);
        assert_eq!(fetch_latest_version(&url).await, None);
    }

    #[tokio::test]
    async fn a_live_manifest_yields_its_version() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/stable/latest.json",
            listener.local_addr().unwrap()
        );
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let body = r#"{"version":"9.9.9","notes":"ignored"}"#;
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });
        assert_eq!(fetch_latest_version(&url).await.as_deref(), Some("9.9.9"));
    }
}
