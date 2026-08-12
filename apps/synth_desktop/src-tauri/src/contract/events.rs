//! Tauri event channel names. Keep in sync with
//! `src/renderer/src/bridge/protocolConstants.ts`.

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
