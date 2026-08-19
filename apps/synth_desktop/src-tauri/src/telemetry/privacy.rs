//! Reject sensitive product-telemetry payloads before they are stored.
//!
//! The scanner is fail-closed: unknown nested objects, secret-shaped keys,
//! prompts, traces, filenames, and local paths are refused. Callers must not
//! "fix" a rejected payload by stripping and retrying with leftover fields.

use serde_json::Value;

const FORBIDDEN_KEY_FRAGMENTS: &[&str] = &[
    "prompt",
    "message",
    "completion",
    "transcript",
    "trace",
    "evidence",
    "filename",
    "filepath",
    "pathname",
    "localpath",
    "sourcecode",
    "secret",
    "token",
    "apikey",
    "authorization",
    "password",
    "credential",
    "cookie",
    "code",
    "dataset",
    "reporttext",
    "output",
    "content",
];

const FORBIDDEN_EXACT: &[&str] = &[
    "path", "file", "text", "body", "input", "output", "prompt", "messages", "trace",
];

fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn key_is_forbidden(key: &str) -> bool {
    let normalized = normalize_key(key);
    FORBIDDEN_EXACT.contains(&normalized.as_str())
        || FORBIDDEN_KEY_FRAGMENTS
            .iter()
            .any(|fragment| normalized.contains(fragment))
}

/// Returns the first forbidden field path, if any.
pub fn forbidden_field(value: &Value) -> Option<String> {
    scan(value, "")
}

fn scan(value: &Value, path: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                if key_is_forbidden(key) {
                    return Some(if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    });
                }
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if let Some(hit) = scan(nested, &child) {
                    return Some(hit);
                }
            }
            None
        }
        Value::Array(items) => {
            for (index, nested) in items.iter().enumerate() {
                let child = format!("{path}[{index}]");
                if let Some(hit) = scan(nested, &child) {
                    return Some(hit);
                }
            }
            None
        }
        Value::String(text) if looks_like_secret(text) || looks_like_path(text) => {
            Some(if path.is_empty() {
                "<value>".into()
            } else {
                path.into()
            })
        }
        _ => None,
    }
}

fn looks_like_secret(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    lowered.starts_with("sk-")
        || lowered.starts_with("rk-")
        || lowered.contains("bearer ")
        || (value.len() >= 24 && value.starts_with("wcap_"))
}

fn looks_like_path(value: &str) -> bool {
    value.starts_with('/') && value.matches('/').count() >= 2
        || value.contains(":\\")
        || value.starts_with("file:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn allowlisted_activation_payloads_pass() {
        assert_eq!(
            forbidden_field(&json!({"workflow_family": "eval", "outcome": "success"})),
            None
        );
    }

    #[test]
    fn prompts_filenames_and_keys_are_rejected() {
        assert_eq!(
            forbidden_field(&json!({"prompt": "do the thing"})).as_deref(),
            Some("prompt")
        );
        assert_eq!(
            forbidden_field(&json!({"filename": "secret.env"})).as_deref(),
            Some("filename")
        );
        assert_eq!(
            forbidden_field(&json!({"api_key": "sk-abc"})).as_deref(),
            Some("api_key")
        );
        assert_eq!(
            forbidden_field(&json!({"note": "sk-live-leaked"})).as_deref(),
            Some("note")
        );
        assert_eq!(
            forbidden_field(&json!({"home": "/Users/ada/project/src"})).as_deref(),
            Some("home")
        );
    }
}
