//! Where a session runs inference / agent substrate.
//!
//! Distinct from [`super::session_run`] status machines and from SessionKind
//! (Codex | Intern — Wave 1). Historical TS name: `ExecutionTarget`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// Laguna on-device model id used by LocalRuntime payloads.
pub const LOCAL_LAGUNA_MODEL: &str = "laguna-xs-2.1";

/// Sync vs async Intern wire mode.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum InternMode {
    Sync,
    Async,
}

impl InternMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Async => "async",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "sync" => Some(Self::Sync),
            "async" => Some(Self::Async),
            _ => None,
        }
    }
}

/// Optional Intern factory / effort binding (renderer camelCase).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InternBinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

/// Inference / agent substrate for a Session (`runs on` edge).
///
/// Wire `kind` tags stay short (`local` | `remote` | `cloud` | `intern`) so
/// existing renderer payloads remain camelCase-compatible. Legacy
/// `{ kind: "remote", provider: "synth-cloud", ... }` deserializes as
/// [`RuntimeTarget::CloudRuntime`].
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeTarget {
    /// On-device Laguna (MLX).
    LocalRuntime {
        model: String,
        adapter: Option<String>,
    },
    /// OpenRouter-hosted model ids.
    RemoteRuntime {
        model: String,
        adapter: Option<String>,
    },
    /// Synth gateway (Credential lease → cloud).
    CloudRuntime {
        model: String,
        adapter: Option<String>,
    },
    /// Synth Intern sync | async.
    InternRuntime {
        mode: InternMode,
        binding: Option<InternBinding>,
    },
}

impl RuntimeTarget {
    pub fn local_laguna() -> Self {
        Self::LocalRuntime {
            model: LOCAL_LAGUNA_MODEL.into(),
            adapter: None,
        }
    }

    /// Map Codex app-server provider names onto RuntimeTarget variants.
    /// SessionKind remains Codex; this is only the inference substrate.
    pub fn from_codex_provider(provider_name: &str, model: &str) -> Self {
        match provider_name {
            "local-laguna" => Self::local_laguna(),
            "synth-cloud" => Self::CloudRuntime {
                model: model.to_owned(),
                adapter: None,
            },
            "openrouter" => Self::RemoteRuntime {
                model: model.to_owned(),
                adapter: None,
            },
            _ => {
                // Unknown / custom providers still run through the remote path.
                Self::RemoteRuntime {
                    model: model.to_owned(),
                    adapter: None,
                }
            }
        }
    }

    /// Stable kind token for DB / index columns (`runtime_target_kind`).
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::LocalRuntime { .. } => "local",
            Self::RemoteRuntime { .. } => "remote",
            Self::CloudRuntime { .. } => "cloud",
            Self::InternRuntime { .. } => "intern",
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::LocalRuntime { .. })
    }

    pub fn is_remote(&self) -> bool {
        matches!(self, Self::RemoteRuntime { .. })
    }

    pub fn is_cloud(&self) -> bool {
        matches!(self, Self::CloudRuntime { .. })
    }

    pub fn is_intern(&self) -> bool {
        matches!(self, Self::InternRuntime { .. })
    }

    pub fn intern_mode(&self) -> Option<InternMode> {
        match self {
            Self::InternRuntime { mode, .. } => Some(*mode),
            _ => None,
        }
    }

    pub fn model(&self) -> Option<&str> {
        match self {
            Self::LocalRuntime { model, .. }
            | Self::RemoteRuntime { model, .. }
            | Self::CloudRuntime { model, .. } => Some(model.as_str()),
            Self::InternRuntime { .. } => None,
        }
    }

    /// Canonical JSON object (same shape as serde Serialize).
    pub fn to_json_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// Parse opaque `target_json` bags, including legacy remote+synth-cloud.
    pub fn from_json_value(value: &Value) -> Result<Self, String> {
        serde_json::from_value(value.clone()).map_err(|error| error.to_string())
    }

    /// Best-effort parse for corrupted / pre-v0.2 rows. Unknown bags become LocalRuntime.
    pub fn from_json_value_lenient(value: &Value) -> Self {
        Self::from_json_value(value).unwrap_or_else(|_| Self::local_laguna())
    }
}

impl Serialize for RuntimeTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            Self::LocalRuntime { model, adapter } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("kind", "local")?;
                map.serialize_entry("model", model)?;
                map.serialize_entry("adapter", adapter)?;
                map.end()
            }
            Self::RemoteRuntime { model, adapter } => {
                // Keep provider for renderer payloads that still expect it.
                let mut map = serializer.serialize_map(Some(4))?;
                map.serialize_entry("kind", "remote")?;
                map.serialize_entry("provider", "openrouter")?;
                map.serialize_entry("model", model)?;
                map.serialize_entry("adapter", adapter)?;
                map.end()
            }
            Self::CloudRuntime { model, adapter } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("kind", "cloud")?;
                map.serialize_entry("model", model)?;
                map.serialize_entry("adapter", adapter)?;
                map.end()
            }
            Self::InternRuntime { mode, binding } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("kind", "intern")?;
                map.serialize_entry("mode", mode)?;
                if let Some(binding) = binding {
                    map.serialize_entry("binding", binding)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for RuntimeTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_runtime_target_value(&value).map_err(serde::de::Error::custom)
    }
}

fn parse_runtime_target_value(value: &Value) -> Result<RuntimeTarget, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "RuntimeTarget must be a JSON object".to_string())?;
    let kind = obj
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "RuntimeTarget.kind is required".to_string())?;
    match kind {
        "local" => Ok(RuntimeTarget::LocalRuntime {
            model: string_field(obj, "model").unwrap_or_else(|| LOCAL_LAGUNA_MODEL.into()),
            adapter: optional_string(obj, "adapter"),
        }),
        "remote" => {
            let provider = obj
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or("openrouter");
            let model = string_field(obj, "model")
                .ok_or_else(|| "RuntimeTarget.model is required for remote/cloud".to_string())?;
            let adapter = optional_string(obj, "adapter");
            if provider == "synth-cloud" {
                // Historical synonym: remote + synth-cloud → CloudRuntime.
                Ok(RuntimeTarget::CloudRuntime { model, adapter })
            } else {
                Ok(RuntimeTarget::RemoteRuntime { model, adapter })
            }
        }
        "cloud" => Ok(RuntimeTarget::CloudRuntime {
            model: string_field(obj, "model")
                .ok_or_else(|| "RuntimeTarget.model is required for cloud".to_string())?,
            adapter: optional_string(obj, "adapter"),
        }),
        "intern" => {
            let mode_raw = obj
                .get("mode")
                .and_then(Value::as_str)
                .ok_or_else(|| "RuntimeTarget.mode is required for intern".to_string())?;
            let mode = InternMode::parse(mode_raw)
                .ok_or_else(|| format!("invalid Intern mode: {mode_raw}"))?;
            let binding = match obj.get("binding") {
                None | Some(Value::Null) => None,
                Some(raw) => Some(
                    serde_json::from_value::<InternBinding>(raw.clone())
                        .map_err(|error| format!("invalid Intern binding: {error}"))?,
                ),
            };
            Ok(RuntimeTarget::InternRuntime { mode, binding })
        }
        // Pre-v0.2 CodexManager wrote SessionKind into target_json.kind.
        // Treat as LocalRuntime when no better signal exists; metadata holds
        // workspace / model detail.
        "codex" => Ok(RuntimeTarget::LocalRuntime {
            model: string_field(obj, "model").unwrap_or_else(|| LOCAL_LAGUNA_MODEL.into()),
            adapter: optional_string(obj, "adapter"),
        }),
        other => Err(format!("unknown RuntimeTarget.kind: {other}")),
    }
}

fn string_field(obj: &Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(|value| value.to_owned())
}

fn optional_string(obj: &Map<String, Value>, key: &str) -> Option<String> {
    match obj.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(other) => Some(other.to_string()),
    }
}

/// Default local target JSON used when seeding placeholder sessions.
#[allow(dead_code)]
pub fn default_local_target_json() -> Value {
    RuntimeTarget::local_laguna().to_json_value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_local_remote_cloud_intern() {
        let cases = [
            RuntimeTarget::local_laguna(),
            RuntimeTarget::RemoteRuntime {
                model: "openai/gpt-5.6-luna".into(),
                adapter: None,
            },
            RuntimeTarget::CloudRuntime {
                model: "openrouter/poolside/laguna-s-2.1".into(),
                adapter: None,
            },
            RuntimeTarget::InternRuntime {
                mode: InternMode::Async,
                binding: Some(InternBinding {
                    factory_id: Some("fac_1".into()),
                    project_id: None,
                    effort_id: None,
                    run_id: None,
                }),
            },
        ];
        for original in cases {
            let encoded = serde_json::to_value(&original).unwrap();
            let decoded: RuntimeTarget = serde_json::from_value(encoded).unwrap();
            assert_eq!(decoded, original);
        }
    }

    #[test]
    fn accepts_legacy_remote_synth_cloud_as_cloud() {
        let legacy = json!({
            "kind": "remote",
            "provider": "synth-cloud",
            "model": "openrouter/poolside/laguna-s-2.1",
            "adapter": null
        });
        let decoded: RuntimeTarget = serde_json::from_value(legacy).unwrap();
        assert!(matches!(decoded, RuntimeTarget::CloudRuntime { .. }));
        let reencoded = serde_json::to_value(&decoded).unwrap();
        assert_eq!(reencoded["kind"], "cloud");
        assert!(reencoded.get("provider").is_none());
    }

    #[test]
    fn remote_serializes_openrouter_provider() {
        let target = RuntimeTarget::RemoteRuntime {
            model: "poolside/laguna-s-2.1".into(),
            adapter: None,
        };
        let encoded = serde_json::to_value(&target).unwrap();
        assert_eq!(encoded["kind"], "remote");
        assert_eq!(encoded["provider"], "openrouter");
    }
}
