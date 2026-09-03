//! Non-reversible fingerprints and in-memory redaction.

use sha2::{Digest, Sha256};
use std::sync::RwLock;

use super::backend::SecretBytes;

pub fn fingerprint(value: &SecretBytes) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("sha256:{}", hex_encode(&digest))
}

/// Last four alphanumeric characters of the secret, for masked UI display.
pub fn display_suffix(value: &SecretBytes) -> String {
    let text = String::from_utf8_lossy(value.as_bytes());
    let chars: Vec<char> = text
        .chars()
        .rev()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(4)
        .collect();
    chars.into_iter().rev().collect::<String>().to_uppercase()
}

pub fn mask_suffix(suffix: &str) -> String {
    if suffix.is_empty() {
        "••••".into()
    } else {
        format!("••••{suffix}")
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// In-memory patterns that must never be persisted. Values are held only so
/// logs, tool results, and crash-adjacent text can be scrubbed before write.
#[derive(Default)]
pub struct RedactionIndex {
    patterns: RwLock<Vec<String>>,
}

impl RedactionIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, value: &SecretBytes) {
        let mut patterns = self.patterns.write().expect("redaction index");
        let utf8 = String::from_utf8_lossy(value.as_bytes()).into_owned();
        if utf8.len() >= 8 && !patterns.iter().any(|existing| existing == &utf8) {
            patterns.push(utf8);
        }
        let digest = fingerprint(value);
        if !patterns.iter().any(|existing| existing == &digest) {
            patterns.push(digest);
        }
    }

    pub fn redact(&self, text: &str) -> String {
        let patterns = self.patterns.read().expect("redaction index");
        let mut out = text.to_owned();
        for pattern in patterns.iter() {
            if pattern.len() >= 8 {
                out = out.replace(pattern, "<redacted-by-workshop>");
            }
        }
        out
    }
}
