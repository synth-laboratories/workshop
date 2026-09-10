//! Instance-local authoring source must travel with a seal, not as a CAS pointer.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

pub(super) fn embedded_source(template_id: &str, certified_digest: Option<&str>) -> Result<Option<Value>> {
    let meta = super::templates::resolve_template(template_id)?;
    if meta.source_kind.as_deref() != Some("user") { return Ok(None); }
    if certified_digest != Some(meta.template_digest.as_str()) {
        bail!("user template changed since certification; certify its current source before sealing");
    }
    let path = Path::new(meta.path.as_deref().context("user template source directory missing")?);
    Ok(Some(source_at(path, &meta.template_digest)?))
}

fn source_at(path: &Path, expected_digest: &str) -> Result<Value> {
    let meta = super::templates::instance_template(path)?.context("user template source is missing")?;
    if meta.source_kind.as_deref() != Some("user") { bail!("expected a user TSX template"); }
    let mut members = BTreeMap::new();
    for (name, limit) in [("shell.tsx", 256 * 1024), ("template.json", 1_500_000)] {
        let mut bytes = Vec::new();
        File::open(path.join(name))?.take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit { bail!("user template source exceeds seal limit"); }
        members.insert(name, String::from_utf8(bytes).context("user template source must be UTF-8")?);
    }
    // The manifest travels as exact source text, but its structured contents
    // must pass policy before JSON string encoding can hide sensitive keys.
    super::artifacts::scan_forbidden(&serde_json::from_str(&members["template.json"])?, "$.template_source.manifest")?;
    // Same ordered, length-delimited identity as templates::template_package_digest.
    // An extra unexported file or a concurrent edit therefore refuses, rather
    // than attaching a certified digest to an incomplete/different source set.
    let mut digest = Sha256::new();
    digest.update(b"synth.visual-template-package.v1\0");
    for (name, text) in &members {
        digest.update((name.len() as u64).to_be_bytes()); digest.update(name.as_bytes());
        digest.update((text.len() as u64).to_be_bytes()); digest.update(text.as_bytes());
    }
    if format!("sha256:{:x}", digest.finalize()) != expected_digest {
        bail!("user template source changed or contains files outside the two-file authoring package");
    }
    let members = members.into_iter().map(|(name, text)| json!({
        "logical_path": name, "digest_sha256": super::artifacts::hex_sha256(text.as_bytes()),
        "size_bytes": text.len(), "text": text
    })).collect::<Vec<_>>();
    let source = json!({"template_id":meta.id,"source_kind":"user","version":meta.version,
        "template_digest":expected_digest,"members":members});
    super::live_eval::assert_no_live_secrets(&source)?;
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("user.seal.v1");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("template.json"), r#"{"schemaVersion":"synth.visual-template.v1","id":"user.seal.v1","version":"1.0.0","inputs":[]}"#).unwrap();
        std::fs::write(path.join("shell.tsx"), "export default function Shell() { return null; }").unwrap();
        let digest = super::super::templates::instance_template(&path).unwrap().unwrap().template_digest;
        (dir, path, digest)
    }
    #[test]
    fn embeds_exact_source_members_without_instance_paths() {
        let (_dir, path, digest) = fixture();
        let source = source_at(&path, &digest).unwrap();
        assert_eq!(source["template_digest"], digest);
        assert_eq!(source["members"].as_array().unwrap().len(), 2);
        for member in source["members"].as_array().unwrap() {
            let text = std::fs::read_to_string(path.join(member["logical_path"].as_str().unwrap())).unwrap();
            assert_eq!(member["text"], text);
            assert_eq!(member["digest_sha256"], super::super::artifacts::hex_sha256(text.as_bytes()));
        }
        assert!(!source.to_string().contains(path.to_str().unwrap()));
    }
    #[test]
    fn changed_or_incomplete_source_cannot_reuse_a_certified_digest() {
        let (_dir, path, digest) = fixture();
        std::fs::write(path.join("shell.tsx"), "export default function Changed() { return null; }").unwrap();
        assert!(source_at(&path, &digest).is_err());
        std::fs::write(path.join("extra.ts"), "export const omitted = true;").unwrap();
        let current = super::super::templates::instance_template(&path).unwrap().unwrap().template_digest;
        assert!(source_at(&path, &current).is_err());
    }
    #[test]
    fn manifest_credentials_cannot_hide_inside_encoded_source_text() {
        let (_dir, path, _) = fixture();
        let file = path.join("template.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        manifest["api_key"] = json!("not-a-real-secret");
        std::fs::write(&file, manifest.to_string()).unwrap();
        let digest = super::super::templates::instance_template(&path).unwrap().unwrap().template_digest;
        assert!(source_at(&path, &digest).unwrap_err().to_string().contains("api_key"));
    }
}
