//! Tauri event channel names + origin tagging. Keep in sync with
//! `src/renderer/src/bridge/protocolConstants.ts`.
//!
//! Drift: `scripts/check-desktop-contract-drift.sh`.

use serde::{Deserialize, Serialize};

/// Named event channels crossing the Rust ↔ renderer boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventChannel;

impl EventChannel {
    /// Single origin-tagged session/runtime stream (Provider | Desktop).
    pub const RUNTIME: &'static str = "runtime:event";
    /// Deprecated alias — producers must not emit here. Renderer may still
    /// listen during transition; remove once compat listen is deleted.
    pub const CODEX: &'static str = "codex:event";
    pub const VISUAL_SHOW: &'static str = "visual:show";
    pub const TERMINAL: &'static str = "terminal:event";
    pub const LAGUNA_STATUS: &'static str = "laguna:status";
    pub const LAGUNA_DOWNLOAD: &'static str = "laguna:download";
    pub const LAGUNA_INFERENCE: &'static str = "laguna:inference";
    pub const TRAINING_MODELS_DOWNLOAD: &'static str = "training-models:download";
    pub const WHISPER_RUNTIME: &'static str = "whisper:runtime";
    pub const WHISPER_DOWNLOAD: &'static str = "whisper:download";
    pub const OPTIMIZER_STATUS: &'static str = "optimizer:status";
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
    EventChannel::TRAINING_MODELS_DOWNLOAD,
    EventChannel::WHISPER_RUNTIME,
    EventChannel::WHISPER_DOWNLOAD,
    EventChannel::OPTIMIZER_STATUS,
];

/// Who produced a boundary event.
///
/// `Provider` = codex/app-server wire notifications.
/// `Desktop` = synthetic session/approval/health (and non-codex sources).
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

/// Envelope for origin-tagged payloads on [`EventChannel::RUNTIME`].
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

/// Wrap `payload` with origin for the single runtime emission channel.
pub fn tag_event<T>(origin: EventOrigin, payload: T) -> OriginTagged<T> {
    OriginTagged { origin, payload }
}

/// Classify a Codex boundary method / journal kind as Provider vs Desktop.
pub fn origin_for_boundary_kind(kind: &str) -> EventOrigin {
    if kind.starts_with("approval.")
        || kind.starts_with("session.")
        || kind.starts_with("run.")
        || kind == "session/unhealthy"
        || kind == "app-server/stderr"
        || kind == "message.created"
    {
        EventOrigin::Desktop
    } else {
        EventOrigin::Provider
    }
}

/// Classify a journaled / forwarded AppEvent by its source + kind.
pub fn origin_for_source_and_kind(source: &str, kind: &str) -> EventOrigin {
    if source == "codex" {
        origin_for_boundary_kind(kind)
    } else {
        EventOrigin::Desktop
    }
}
