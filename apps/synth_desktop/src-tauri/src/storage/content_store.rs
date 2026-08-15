use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Immutable content-addressed blob store under `store/{kind}/ab/abcdef…`.
#[derive(Clone, Debug)]
pub struct ContentStore {
    root: PathBuf,
}

impl ContentStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put_bytes(&self, kind: &str, bytes: &[u8]) -> Result<String> {
        validate_kind(kind)?;
        let digest = hex_sha256(bytes);
        let path = self.path_for(kind, &digest);
        if path.exists() {
            return Ok(digest);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        {
            let mut file =
                File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        fs::rename(&tmp, &path).with_context(|| format!("rename into {}", path.display()))?;
        Ok(digest)
    }

    pub fn get_bytes(&self, kind: &str, digest: &str) -> Result<Vec<u8>> {
        validate_kind(kind)?;
        validate_digest(digest)?;
        let path = self.path_for(kind, digest);
        let mut file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let actual = hex_sha256(&bytes);
        if actual != digest.to_ascii_lowercase() {
            bail!(
                "content-addressed blob failed digest verification: expected {digest}, got {actual}"
            );
        }
        Ok(bytes)
    }

    pub fn path_for(&self, kind: &str, digest: &str) -> PathBuf {
        let prefix = digest.get(..2).unwrap_or("00");
        self.root.join(kind).join(prefix).join(digest)
    }

    pub fn exists(&self, kind: &str, digest: &str) -> bool {
        validate_kind(kind).is_ok()
            && validate_digest(digest).is_ok()
            && self.path_for(kind, digest).exists()
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn validate_kind(kind: &str) -> Result<()> {
    match kind {
        "blobs" | "previews" | "traces" | "trace_imports" | "exports" | "artifact_bundles"
        | "report_bundles" => Ok(()),
        _ => bail!("unsupported content store kind: {kind}"),
    }
}

fn validate_digest(digest: &str) -> Result<()> {
    if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid content digest");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn put_is_idempotent_and_addressed() {
        let dir = tempdir().unwrap();
        let store = ContentStore::new(dir.path());
        let digest = store.put_bytes("blobs", b"hello visual").unwrap();
        assert_eq!(digest.len(), 64);
        assert!(store.exists("blobs", &digest));
        let again = store.put_bytes("blobs", b"hello visual").unwrap();
        assert_eq!(digest, again);
        assert_eq!(store.get_bytes("blobs", &digest).unwrap(), b"hello visual");
    }

    #[test]
    fn trace_imports_are_quarantined_in_their_own_kind() {
        let dir = tempdir().unwrap();
        let store = ContentStore::new(dir.path());
        let digest = store
            .put_bytes("trace_imports", b"untrusted input")
            .unwrap();
        assert!(store.exists("trace_imports", &digest));
        assert!(!store.exists("traces", &digest));
    }

    #[test]
    fn reads_fail_closed_when_stored_bytes_are_tampered() {
        let dir = tempdir().unwrap();
        let store = ContentStore::new(dir.path());
        let digest = store.put_bytes("report_bundles", b"sealed report").unwrap();
        fs::write(store.path_for("report_bundles", &digest), b"tampered report").unwrap();

        let error = store
            .get_bytes("report_bundles", &digest)
            .expect_err("tampered CAS bytes must not be returned");
        assert!(error.to_string().contains("digest verification"));
    }
}
