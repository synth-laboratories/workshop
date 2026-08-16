//! Central redaction. Runs once, before the queue, on every diagnostic.
//!
//! The rule this module exists to enforce: a credential that never enters the
//! envelope cannot leak from the journal, the index, an MCP response, the
//! Diagnostics pane, or a support bundle. Redacting at any later stage would
//! leave the earlier ones holding cleartext.
//!
//! Three kinds of rewrite:
//!   1. Secret-shaped **keys** lose their value entirely.
//!   2. Secret-shaped **substrings** are scrubbed out of free text.
//!   3. Prompt-shaped keys collapse to `{length, digest}` — never raw text.

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub const REDACTED: &str = "<redacted>";

/// Key fragments whose value is never safe to keep. Matched case-insensitively
/// against the whole key with `-`/`_`/`.` normalized away, so `api_key`,
/// `apiKey`, `X-Api-Key`, and `openai.api.key` all match `apikey`.
const SECRET_KEY_FRAGMENTS: &[&str] = &[
    "apikey",
    "authorization",
    "accesstoken",
    "refreshtoken",
    "idtoken",
    "sessiontoken",
    "bearertoken",
    "clientsecret",
    "privatekey",
    "secretkey",
    "password",
    "passphrase",
    "cookie",
    "credential",
    "signature",
];

/// Whole-key matches. Kept separate from the fragment list because these words
/// are too short to match as substrings without eating innocent keys
/// (`tokenizer`, `authenticated`, `secretless`).
const SECRET_KEY_EXACT: &[&str] = &["token", "secret", "auth", "key", "session", "sessionkey"];

/// Keys whose value is model-visible text. Never stored raw: replaced by
/// length + digest so an agent can still tell two runs apart.
const PROMPT_KEY_EXACT: &[&str] = &[
    "prompt",
    "prompttext",
    "systemprompt",
    "instructions",
    "messages",
    "input",
    "completion",
    "output",
    "responsetext",
    "content",
    "transcript",
];

/// Keys that carry a whole environment. Dropped wholesale — an env snapshot is
/// a credential store with extra steps.
const ENVIRONMENT_KEY_EXACT: &[&str] = &["env", "environ", "environment", "envvars", "processenv"];

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

fn is_prompt_key(key: &str) -> bool {
    PROMPT_KEY_EXACT.contains(&normalize_key(key).as_str())
}

fn is_environment_key(key: &str) -> bool {
    ENVIRONMENT_KEY_EXACT.contains(&normalize_key(key).as_str())
}

/// `SCREAMING_SNAKE` keys are environment variables even when they arrive
/// loose. Names that look like a variable and read like a secret lose their
/// value regardless of which list matched.
fn is_environment_variable_name(key: &str) -> bool {
    key.len() >= 3
        && key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && key.bytes().any(|byte| byte.is_ascii_uppercase())
}

const ENVIRONMENT_SECRET_FRAGMENTS: &[&str] =
    &["KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH"];

fn is_secret_environment_variable(key: &str) -> bool {
    is_environment_variable_name(key)
        && ENVIRONMENT_SECRET_FRAGMENTS
            .iter()
            .any(|fragment| key.contains(fragment))
}

pub fn digest_metadata(value: &str) -> Value {
    let digest = Sha256::digest(value.as_bytes());
    json!({
        "length": value.chars().count(),
        "digest": format!("sha256:{:x}", digest)[..23].to_owned(),
        "redacted": "prompt_content",
    })
}

/// Scrub secret-shaped substrings out of free text.
///
/// Deliberately conservative on shape, not on context: these token forms are
/// unambiguous enough that a false positive costs a few characters of a
/// message, while a false negative writes a live credential to disk.
pub fn redact_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some((prefix, secret, remainder)) = next_secret(rest) {
        out.push_str(prefix);
        out.push_str(REDACTED);
        let _ = secret;
        rest = remainder;
    }
    out.push_str(rest);
    out
}

/// Find the earliest secret-shaped run in `value`.
fn next_secret(value: &str) -> Option<(&str, &str, &str)> {
    let mut best: Option<(usize, usize)> = None;
    let mut consider = |start: usize, end: usize| {
        if end > start && best.map_or(true, |(current, _)| start < current) {
            best = Some((start, end));
        }
    };

    // `Authorization: Bearer <token>` and bare `Bearer <token>`.
    let lowered = value.to_ascii_lowercase();
    let mut search = 0usize;
    while let Some(found) = lowered[search..].find("bearer ") {
        let start = search + found + "bearer ".len();
        let end = token_end(value, start);
        consider(start, end);
        search = start.max(search + found + 1);
        if search >= value.len() {
            break;
        }
    }

    // Provider key shapes and other self-identifying credential prefixes.
    for prefix in [
        "sk-", "sk_", "rk_", "pk_live_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_", "xoxb-", "xoxp-",
        "xoxa-", "xapp-", "AKIA", "ASIA", "synth_", "eyJ",
    ] {
        let mut search = 0usize;
        while let Some(found) = value[search..].find(prefix) {
            let start = search + found;
            let end = token_end(value, start);
            // Prefix alone is not a credential; require real entropy after it.
            if end - start >= prefix.len() + 8 {
                consider(start, end);
            }
            search = start + prefix.len();
            if search >= value.len() {
                break;
            }
        }
    }

    // `scheme://user:password@host`
    if let Some(at) = value.find('@') {
        if let Some(scheme) = value[..at].rfind("://") {
            let userinfo = scheme + "://".len();
            if value[userinfo..at].contains(':') {
                let start = userinfo + value[userinfo..at].find(':').unwrap_or(0) + 1;
                consider(start, at);
            }
        }
    }

    // PEM blocks: everything between the BEGIN and END markers.
    if let Some(start) = value.find("-----BEGIN") {
        let end = value[start..]
            .find("-----END")
            .map(|offset| {
                let tail = start + offset;
                value[tail..]
                    .find("-----\n")
                    .map(|close| tail + close + "-----\n".len())
                    .unwrap_or(value.len())
            })
            .unwrap_or(value.len());
        consider(start, end);
    }

    let (start, end) = best?;
    Some((&value[..start], &value[start..end], &value[end..]))
}

fn token_end(value: &str, start: usize) -> usize {
    value[start..]
        .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ')' | '}' | ']' | ';'))
        .map(|offset| start + offset)
        .unwrap_or(value.len())
}

/// Redact and bound a `details` object.
pub fn redact_details(details: Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    let mut dropped = 0usize;
    for (key, value) in details {
        if out.len() >= super::event::MAX_DETAILS_KEYS {
            dropped += 1;
            continue;
        }
        out.insert(key.clone(), redact_value(&key, value, 0));
    }
    if dropped > 0 {
        out.insert("details_dropped_keys".into(), json!(dropped));
    }
    // A details object is a summary, never a payload. Anything still oversized
    // after per-key redaction is replaced by its own shape.
    let encoded = Value::Object(out.clone()).to_string();
    if encoded.len() > super::event::MAX_DETAILS_BYTES {
        let keys: Vec<String> = out.keys().cloned().collect();
        let mut bounded = Map::new();
        bounded.insert("details_omitted".into(), json!("oversized"));
        bounded.insert("details_bytes".into(), json!(encoded.len()));
        bounded.insert("details_keys".into(), json!(keys));
        return bounded;
    }
    out
}

fn redact_value(key: &str, value: Value, depth: usize) -> Value {
    if depth >= super::event::MAX_DETAILS_DEPTH {
        return json!("<depth-limited>");
    }
    if is_environment_key(key) {
        return json!({"redacted": "environment_snapshot"});
    }
    if is_secret_key(key) || is_secret_environment_variable(key) {
        return json!(REDACTED);
    }
    if is_prompt_key(key) {
        return match value {
            Value::String(text) => digest_metadata(&text),
            other => digest_metadata(&other.to_string()),
        };
    }
    match value {
        Value::String(text) => Value::String(super::event::truncate_chars(
            &redact_text(&text),
            super::event::MAX_MESSAGE_CHARS,
        )),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .take(64)
                .map(|item| redact_value(key, item, depth + 1))
                .collect(),
        ),
        Value::Object(fields) => {
            // A nested object of environment-variable names is an env dump no
            // matter what its parent key was called.
            let variable_like = fields
                .keys()
                .filter(|name| is_environment_variable_name(name))
                .count();
            if fields.len() >= 8 && variable_like * 2 >= fields.len() {
                return json!({"redacted": "environment_snapshot"});
            }
            Value::Object(
                fields
                    .into_iter()
                    .take(super::event::MAX_DETAILS_KEYS)
                    .map(|(name, item)| {
                        let redacted = redact_value(&name, item, depth + 1);
                        (name, redacted)
                    })
                    .collect(),
            )
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn details(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn scrubs_authorization_headers_from_free_text() {
        let text = "MCP call failed: Authorization: Bearer sk-proj-abcdef0123456789 returned 401";
        let redacted = redact_text(text);
        assert!(!redacted.contains("sk-proj-abcdef0123456789"), "{redacted}");
        assert!(redacted.contains(REDACTED));
        assert!(redacted.contains("returned 401"));
    }

    #[test]
    fn scrubs_every_known_credential_shape() {
        for secret in [
            "sk-abcdefghijklmnop",
            "sk_live_abcdefghijklmnop",
            "ghp_abcdefghijklmnopqrstuvwxyz0123",
            "xoxb-1234567890-abcdefghij",
            "AKIAIOSFODNN7EXAMPLE",
            "synth_vis_9f8e7d6c5b4a3210",
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIn0.c2ln",
        ] {
            let redacted = redact_text(&format!("failure with {secret} in play"));
            assert!(!redacted.contains(secret), "{secret} survived: {redacted}");
        }
    }

    #[test]
    fn scrubs_url_userinfo_and_private_key_blocks() {
        let url = redact_text("connecting to https://user:hunter2@containers.local/health");
        assert!(!url.contains("hunter2"), "{url}");
        let pem =
            redact_text("-----BEGIN PRIVATE KEY-----\nMIIBVgIBADAN\n-----END PRIVATE KEY-----\n");
        assert!(!pem.contains("MIIBVgIBADAN"), "{pem}");
    }

    #[test]
    fn leaves_ordinary_diagnostic_text_alone() {
        let text =
            "Unsupported trace projection schema: synth.trace.v5 (visual vis_9, revision 14)";
        assert_eq!(redact_text(text), text);
    }

    #[test]
    fn secret_shaped_keys_lose_their_value_in_any_casing() {
        let redacted = redact_details(details(&[
            ("apiKey", json!("sk-live-1")),
            ("X-Api-Key", json!("abc")),
            ("Authorization", json!("Bearer abc")),
            ("refresh_token", json!("rt")),
            ("cookie", json!("session=1")),
            ("SYNTH_API_KEY", json!("plain")),
            (
                "expected_schemas",
                json!(["synth.trace-projection.rollout-inspector.v1"]),
            ),
        ]));
        for key in [
            "apiKey",
            "X-Api-Key",
            "Authorization",
            "refresh_token",
            "cookie",
            "SYNTH_API_KEY",
        ] {
            assert_eq!(redacted[key], json!(REDACTED), "{key}");
        }
        assert_eq!(
            redacted["expected_schemas"],
            json!(["synth.trace-projection.rollout-inspector.v1"])
        );
    }

    #[test]
    fn prompt_content_becomes_length_and_digest_only() {
        let redacted = redact_details(details(&[("prompt", json!("solve the maze carefully"))]));
        let prompt = &redacted["prompt"];
        assert_eq!(prompt["length"], json!(24));
        assert_eq!(prompt["redacted"], json!("prompt_content"));
        assert!(prompt["digest"].as_str().unwrap().starts_with("sha256:"));
        assert!(!Value::Object(redacted)
            .to_string()
            .contains("solve the maze"));
    }

    #[test]
    fn environment_snapshots_are_dropped_whole() {
        let mut environment = Map::new();
        for index in 0..10 {
            environment.insert(format!("SOME_VAR_{index}"), json!("value"));
        }
        let by_key = redact_details(details(&[("env", json!({"PATH": "/usr/bin"}))]));
        assert_eq!(by_key["env"], json!({"redacted": "environment_snapshot"}));
        let by_shape = redact_details(details(&[("captured", Value::Object(environment))]));
        assert_eq!(
            by_shape["captured"],
            json!({"redacted": "environment_snapshot"})
        );
    }

    #[test]
    fn nested_secrets_are_redacted_at_depth() {
        let redacted = redact_details(details(&[(
            "request",
            json!({"headers": {"authorization": "Bearer sk-abcdefghijkl"}, "status": 401}),
        )]));
        let encoded = Value::Object(redacted).to_string();
        assert!(!encoded.contains("sk-abcdefghijkl"), "{encoded}");
        assert!(encoded.contains("401"));
    }

    #[test]
    fn oversized_details_collapse_to_their_shape() {
        let mut wide: Vec<(&str, Value)> = vec![("status", json!(500))];
        let blob = json!("x".repeat(super::super::event::MAX_MESSAGE_CHARS));
        for key in ["a", "b", "c", "d", "e", "f"] {
            wide.push((key, blob.clone()));
        }
        let redacted = redact_details(details(&wide));
        assert_eq!(redacted["details_omitted"], json!("oversized"));
        assert!(redacted["details_keys"]
            .as_array()
            .unwrap()
            .contains(&json!("status")));
    }

    #[test]
    fn key_count_is_bounded() {
        let wide: Map<String, Value> = (0..super::super::event::MAX_DETAILS_KEYS * 2)
            .map(|index| (format!("field_{index}"), json!(index)))
            .collect();
        let redacted = redact_details(wide);
        assert!(redacted.len() <= super::super::event::MAX_DETAILS_KEYS + 1);
        assert!(redacted.contains_key("details_dropped_keys"));
    }
}
