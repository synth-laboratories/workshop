//! SessionKind is the architecture routing law: every Session is exactly one of
//! Codex | Intern (`prefer_hierarchies_of_clear_nouns`, `metadata_bags_are_not_authority`).
//!
//! RuntimeTarget (Local / Remote / Cloud / InternRuntime) is Wave 2 — do not
//! fold inference substrate into this enum.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Product routing kind for a Session. Persisted as a first-class SQLite column
/// (`sessions.kind`); `target_json.kind` is legacy cache, not authority.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Codex,
    Intern,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Intern => "intern",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "codex" => Ok(Self::Codex),
            "intern" => Ok(Self::Intern),
            _ => bail!("unknown session kind: {value}"),
        }
    }

    /// Best-effort recovery from legacy `target_json` bags (pre-column DBs and
    /// ad-hoc inserts). Prefer the typed `sessions.kind` column once migrated.
    pub fn from_target_json(target: &Value) -> Self {
        match target.get("kind").and_then(Value::as_str) {
            Some("intern") => Self::Intern,
            // Historical Codex rows used `"codex"` or `"local"` (Laguna-shaped
            // bags). Both route as Codex under the v0.2 SessionKind law.
            Some("codex") | Some("local") | _ => Self::Codex,
        }
    }
}

impl std::fmt::Display for SessionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

