//! Redact secrets, headers, and credential-shaped values before persistence or export.

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub const REDACTED: &str = "<redacted>";

const SECRET_KEY_FRAGMENTS: &[&str] = &[
    "apikey",
    "authorization",
    "accesstoken",
    "refreshtoken",
    "bearertoken",
    "clientsecret",
    "privatekey",
    "secretkey",
    "password",
    "passphrase",
    "cookie",
    "credential",
    "keychain",
];

const SECRET_KEY_EXACT: &[&str] = &["token", "secret", "auth", "key"];

fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn is_secret_key(key: &str) -> bool {
    let normalized = normalize_key(key);
    SECRET_KEY_EXACT.contains(&normalized.as_str())
        || SECRET_KEY_FRAGMENTS
            .iter()
            .any(|fragment| normalized.contains(fragment))
}

pub fn redact_value(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, child) in map {
                if is_secret_key(&key) {
                    out.insert(key, json!(REDACTED));
                } else {
                    out.insert(key, redact_value(child));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(redact_value).collect()),
        Value::String(text) => Value::String(redact_text(&text)),
        other => other,
    }
}

pub fn redact_text(text: &str) -> String {
    let mut out = text.to_string();
    for needle in ["sk-", "sess-", "key-", "tok-"] {
        while let Some(start) = out.find(needle) {
            let rest = &out[start..];
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
                .unwrap_or(rest.len());
            if end > needle.len() + 8 {
                out.replace_range(start..start + end, REDACTED);
            } else {
                break;
            }
        }
    }
    out
}

pub fn digest_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

