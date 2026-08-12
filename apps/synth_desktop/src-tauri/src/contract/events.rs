//! Tauri event channel names + origin tagging. Keep in sync with
//! `src/renderer/src/bridge/protocolConstants.ts`.
//!
//! Drift: `scripts/check-desktop-contract-drift.sh`.

use serde::{Deserialize, Serialize};

/// Named event channels crossing the Rust ↔ renderer boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventChannel;

impl EventChannel {
    pub const RUNTIME: &'static str = "runtime:event";
    pub const CODEX: &'static str = "codex:event";
    pub const VISUAL_SHOW: &'static str = "visual:show";
    pub const TERMINAL: &'static str = "terminal:event";
    pub const LAGUNA_STATUS: &'static str = "laguna:status";
    pub const LAGUNA_DOWNLOAD: &'static str = "laguna:download";
    pub const LAGUNA_INFERENCE: &'static str = "laguna:inference";
    pub const WHISPER_RUNTIME: &'static str = "whisper:runtime";
    pub const WHISPER_DOWNLOAD: &'static str = "whisper:download";
}

/// All known channels (for drift checks / docs).
pub const EVENT_CHANNELS: &[&str] = &[
    EventChannel::RUNTIME,
    EventChannel::CODEX,
    EventChannel::VISUAL_SHOW,
    EventChannel::TERMINAL,
    EventChannel::LAGUNA_STATUS,
    EventChannel::LAGUNA_DOWNLOAD,
    EventChannel::LAGUNA_INFERENCE,
    EventChannel::WHISPER_RUNTIME,
    EventChannel::WHISPER_DOWNLOAD,
];

/// Who produced a boundary event.
///
/// Wave 2b stub: new emission paths should tag `Provider` (codex/app-server)
/// vs `Desktop` (synthetic session/approval/health). Full dual-channel collapse
/// (`codex:event` + `runtime:event` → one origin-tagged stream) lands after
/// renderer consumers migrate off the parallel channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventOrigin {
    Provider,
    Desktop,
}

impl EventOrigin {
    pub const PROVIDER: &'static str = "provider";
    pub const DESKTOP: &'static str = "desktop";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provider => Self::PROVIDER,
            Self::Desktop => Self::DESKTOP,
        }
    }
}

/// Envelope for origin-tagged payloads. Prefer this over bare channel emits
/// once consumers read `origin`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OriginTagged<T> {
    pub origin: EventOrigin,
    pub payload: T,
}

impl<T> OriginTagged<T> {
    pub fn provider(payload: T) -> Self {
        Self {
            origin: EventOrigin::Provider,
            payload,
        }
    }

    pub fn desktop(payload: T) -> Self {
        Self {
            origin: EventOrigin::Desktop,
            payload,
        }
    }
}

/// Stub emission helper: wraps `payload` with origin. Call sites still choose
/// the channel until dual emission is collapsed.
pub fn tag_event<T>(origin: EventOrigin, payload: T) -> OriginTagged<T> {
    OriginTagged { origin, payload }
}
