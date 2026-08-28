//! Structured per-action before/after state, redacted, bound to a versioned
//! run. `docs/COMPUTER_USE.md` §3 gate G8, Phase 6.
//!
//! Stated narrowly, because the honest version of the claim matters: the
//! reference suite *does* capture. Record & Replay records a human to
//! synthesize a skill, and Skysight writes rolling prose summaries into agent
//! memory. What nobody ships is structured per-action before/after state,
//! redacted, bound to a versioned run, for evaluation and training. That is the
//! differentiator — not "they don't record anything", which is false.
//!
//! Blobs go to the content store, addressed by digest, so a tree captured twice
//! is stored once and a step record stays small enough to sit in the journal.

use super::policy::HazardReason;
use super::vocabulary::Action;
use crate::storage::content_store::ContentStore;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const TRAJECTORY_STEP_SCHEMA: &str = "synth.computer-use-step.v1";
pub const TRAJECTORY_RUN_SCHEMA: &str = "synth.computer-use-run.v1";

/// Content-store kinds. Separate kinds keep an accessibility tree from being
/// served as an image and vice versa.
pub const KIND_AX_TREE: &str = "computer_use_ax";
pub const KIND_SCREENSHOT: &str = "computer_use_screenshots";

/// Everything needed to know what a replay is replaying. A trajectory without
/// this is a recording of an unknown system.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunVersion {
    /// Helper bundle version that performed the actions.
    pub helper_version: String,
    /// Code-signing identity the helper ran under, so a trajectory recorded by
    /// an unsigned dev build is never mistaken for one from a shipped build.
    pub helper_identity: String,
    /// Vocabulary revision, bumped when §5 changes shape.
    pub vocabulary_version: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StateRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ax_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_digest: Option<String>,
    /// Elements in the tree at capture time. Cheap to read, and the fastest
    /// signal that a step changed the UI at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_count: Option<u64>,
}

impl StateRef {
    pub fn is_empty(&self) -> bool {
        self.ax_digest.is_none() && self.screenshot_digest.is_none()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrajectoryStep {
    pub schema_version: String,
    pub step_id: String,
    /// The computer-use session this belongs to.
    pub run_id: String,
    pub session_id: String,
    /// Monotonic within the run, so ordering survives out-of-order writes.
    pub sequence: u64,
    pub version: RunVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub verb: String,
    /// Redacted parameters, exactly as the operator would have seen them.
    pub params: Value,
    /// G10 grades a run on whether it needed pixels.
    pub used_coordinates: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hazard: Option<HazardReason>,
    pub before: StateRef,
    pub after: StateRef,
    pub started_at: String,
    pub finished_at: String,
    /// `ok`, `refused`, or `error`.
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub const RESULT_OK: &str = "ok";
pub const RESULT_REFUSED: &str = "refused";
pub const RESULT_ERROR: &str = "error";

/// A captured observation before it is stored.
#[derive(Clone, Debug, Default)]
pub struct Observation {
    /// Accessibility tree text, with element indexes.
    pub ax_text: Option<String>,
    /// Screenshot bytes, PNG.
    pub screenshot: Option<Vec<u8>>,
    pub element_count: Option<u64>,
}

pub struct TrajectoryRecorder {
    store: ContentStore,
    version: RunVersion,
    run_id: String,
    session_id: String,
    sequence: u64,
}

impl TrajectoryRecorder {
    pub fn new(
        store: ContentStore,
        version: RunVersion,
        run_id: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            store,
            version,
            run_id: run_id.into(),
            session_id: session_id.into(),
            sequence: 0,
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Store one observation's blobs and return the reference that goes in the
    /// step. Redaction happens here, on the way in — a raw tree must never
    /// reach the content store, because "we'll redact it on read" is a promise
    /// that only holds until someone reads it another way.
    pub fn capture(&self, observation: &Observation) -> Result<StateRef> {
        let ax_digest = match observation.ax_text.as_deref() {
            Some(text) => Some(
                self.store
                    .put_bytes(KIND_AX_TREE, redact(text).as_bytes())
                    .context("store accessibility tree")?,
            ),
            None => None,
        };
        let screenshot_digest = match observation.screenshot.as_deref() {
            Some(bytes) => Some(
                self.store
                    .put_bytes(KIND_SCREENSHOT, bytes)
                    .context("store screenshot")?,
            ),
            None => None,
        };
        Ok(StateRef {
            ax_digest,
            screenshot_digest,
            element_count: observation.element_count,
        })
    }

    /// Record one step. Takes the already-captured before/after refs so a
    /// refused action still produces a step: a trajectory that only contains
    /// what succeeded cannot be used to evaluate judgment.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        action: &Action,
        before: StateRef,
        after: StateRef,
        approval_receipt_id: Option<String>,
        hazard: Option<HazardReason>,
        result: &str,
        error: Option<String>,
        started_at: String,
        finished_at: String,
    ) -> TrajectoryStep {
        self.sequence += 1;
        TrajectoryStep {
            schema_version: TRAJECTORY_STEP_SCHEMA.into(),
            step_id: format!("cu_step_{}", uuid::Uuid::new_v4().simple()),
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            sequence: self.sequence,
            version: self.version.clone(),
            app: action.app().map(str::to_owned),
            verb: action.verb().into(),
            params: redact_value(&action.approval_payload()),
            used_coordinates: action.uses_coordinates(),
            approval_receipt_id,
            hazard,
            before,
            after,
            started_at,
            finished_at,
            result: result.into(),
            error: error.map(|value| redact(&value)),
        }
    }
}

/// Markers that a value on the rest of the line is a credential.
const SECRET_MARKERS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "api key",
    "api_key",
    "apikey",
    "token",
    "credential",
    "private key",
    "authorization",
];

/// Accessibility roles whose value is never safe to keep.
const SECURE_ROLES: &[&str] = &["AXSecureTextField", "AXSecureTextArea"];

/// Token prefixes that identify a credential on their own.
const SECRET_PREFIXES: &[&str] = &[
    "Bearer ",
    "sk-",
    "sk_live_",
    "sk_test_",
    "ghp_",
    "gho_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "AKIA",
    "-----BEGIN",
];

pub const REDACTED: &str = "[redacted]";

/// Redact a captured accessibility tree or error string.
///
/// The helper redacts secure fields at the source, where the role is known.
/// This is the second net, and it is deliberately blunt: over-redacting an
/// accessibility tree costs some replay fidelity, under-redacting one writes a
/// password into durable storage.
pub fn redact(text: &str) -> String {
    text.lines().map(redact_line).collect::<Vec<_>>().join("\n")
}

fn redact_line(line: &str) -> String {
    let lowered = line.to_ascii_lowercase();
    if SECURE_ROLES.iter().any(|role| line.contains(role)) {
        return redact_after(line, "value");
    }
    if let Some(prefix) = SECRET_PREFIXES.iter().find(|prefix| line.contains(*prefix)) {
        let index = line.find(*prefix).unwrap_or(0);
        return format!("{}{REDACTED}", &line[..index]);
    }
    if let Some(marker) = SECRET_MARKERS
        .iter()
        .find(|marker| lowered.contains(*marker))
    {
        return redact_after(line, marker);
    }
    if let Some(index) = find_high_entropy_run(line) {
        return format!("{}{REDACTED}", &line[..index]);
    }
    line.to_owned()
}

/// Keep the label, drop everything after the first separator following it.
fn redact_after(line: &str, marker: &str) -> String {
    let lowered = line.to_ascii_lowercase();
    let Some(start) = lowered.find(marker) else {
        return format!("{REDACTED}");
    };
    let tail = &line[start + marker.len()..];
    let cut = tail
        .find(|ch: char| ch == ':' || ch == '=' || ch == '"')
        .map(|offset| start + marker.len() + offset + 1)
        .unwrap_or(start + marker.len());
    format!("{}{REDACTED}", &line[..cut.min(line.len())])
}

/// A long unbroken run of base64/hex characters is a key, a token, or a digest.
/// Digests are addressed elsewhere and do not appear in accessibility text, so
/// the false-positive cost here is low.
fn find_high_entropy_run(line: &str) -> Option<usize> {
    const MIN: usize = 32;
    let bytes = line.as_bytes();
    let mut start = None;
    let mut digits = 0usize;
    let mut letters = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        let is_token = byte.is_ascii_alphanumeric()
            || *byte == b'+'
            || *byte == b'/'
            || *byte == b'_'
            || *byte == b'-';
        if is_token {
            if start.is_none() {
                start = Some(index);
                digits = 0;
                letters = 0;
            }
            if byte.is_ascii_digit() {
                digits += 1;
            } else if byte.is_ascii_alphabetic() {
                letters += 1;
            }
        } else {
            if let Some(begin) = start {
                // Require both letters and digits so ordinary prose and long
                // identifiers like AXStaticTextFieldSomething do not match.
                if index - begin >= MIN && digits > 0 && letters > 0 {
                    return Some(begin);
                }
            }
            start = None;
        }
    }
    if let Some(begin) = start {
        if bytes.len() - begin >= MIN && digits > 0 && letters > 0 {
            return Some(begin);
        }
    }
    None
}

/// Redact every string inside a JSON value, in place of the whole value.
pub fn redact_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact(text)),
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, nested)| {
                    let redacted = if SECRET_MARKERS
                        .iter()
                        .any(|marker| key.to_ascii_lowercase().contains(marker))
                    {
                        Value::String(REDACTED.into())
                    } else {
                        redact_value(nested)
                    };
                    (key.clone(), redacted)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

