use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{future::Future, pin::Pin};

pub const PROTOCOL_VERSION: &str = "workshop.browser.v1";
pub const DEFAULT_MAX_CHARS: u32 = 16_000;
pub const HARD_MAX_CHARS: u32 = 20_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserMeta {
    pub session_id: String,
    pub tab_id: String,
    pub document_revision: String,
    pub origin: String,
    pub truncated: bool,
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_cursor: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Ref {
        session_id: String,
        tab_id: String,
        document_revision: String,
        element_id: String,
    },
    Locator {
        role: String,
        name: String,
        #[serde(default)]
        exact: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ObservationLimit {
    #[serde(default = "default_max_chars")]
    pub max_chars: u32,
    #[serde(default)]
    pub cursor: u32,
}

fn default_max_chars() -> u32 {
    DEFAULT_MAX_CHARS
}

impl ObservationLimit {
    pub fn validated(&self) -> Result<Self, String> {
        if !(256..=HARD_MAX_CHARS).contains(&self.max_chars) {
            return Err(format!("maxChars must be between 256 and {HARD_MAX_CHARS}"));
        }
        Ok(self.clone())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRequest {
    pub operation: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserResponse {
    pub protocol_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<BrowserMeta>,
    pub result: Value,
}

pub type BackendFuture<'a> =
    Pin<Box<dyn Future<Output = Result<BrowserResponse, String>> + Send + 'a>>;

/// Narrow service boundary intentionally implementable by either a sidecar
/// (Playwright) or an embedded engine (CEF/WKWebView/Servo).
pub trait BrowserBackend: Send {
    fn call<'a>(&'a mut self, request: BrowserRequest) -> BackendFuture<'a>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_hard_observation_ceilings_are_enforced() {
        assert_eq!(default_max_chars(), 16_000);
        assert!(ObservationLimit {
            max_chars: 20_000,
            cursor: 0
        }
        .validated()
        .is_ok());
        assert!(ObservationLimit {
            max_chars: 20_001,
            cursor: 0
        }
        .validated()
        .is_err());
    }

    #[test]
    fn stable_refs_carry_all_staleness_dimensions() {
        let target = Target::Ref {
            session_id: "session-1".into(),
            tab_id: "tab-2".into(),
            document_revision: "doc-3".into(),
            element_id: "element-4".into(),
        };
        let encoded = serde_json::to_value(target).unwrap();
        assert_eq!(encoded["ref"]["document_revision"], "doc-3");
        assert_eq!(encoded["ref"]["tab_id"], "tab-2");
    }
}
