//! Installing the Synth-published Laguna adapter.
//!
//! The base weights are distributed as a pinned repository revision the app
//! knows about; a published adapter is distributed the same way, by digest.
//! One curated adapter is served as `synth/Laguna-XS-2.1-ft`.
//!
//! Nothing here trusts the transport. Every file is checked against the
//! manifest and the tree is re-digested afterwards, because a truncated object
//! and a complete one produce the same plausible tokens — the failure would
//! otherwise surface as a quality regression nobody could attribute.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// The model id the curated finetune is served under.
pub const FT_MODEL_ID: &str = "synth/Laguna-XS-2.1-ft";
/// Manifest schema this build knows how to install.
pub const MANIFEST_SCHEMA: &str = "synth-adapter.v1";
const MANIFEST_FILE: &str = "manifest.json";

/// Where a published adapter is fetched from.
///
/// Wasabi and MinIO differ by base URL alone, and a HuggingFace mirror slots
/// in as another base URL over the same verify-and-install path.
pub fn source_base_url() -> String {
    std::env::var("SYNTH_ADAPTERS_BASE_URL")
        .unwrap_or_else(|_| "https://adapters.usesynth.ai".to_string())
}

/// A published adapter this build is willing to install.
#[derive(Clone, Copy, Debug)]
pub struct AdapterSpec {
    pub model_id: &'static str,
    pub title: &'static str,
    /// Tree digest; also the object prefix and the catalog identity.
    pub digest: &'static str,
    /// The base weights revision this adapter was trained against.
    pub base_revision: &'static str,
    pub object_prefix: &'static str,
    pub download_bytes: u64,
}

/// One curated adapter. A menu is a product decision nothing here assumes.
pub const ADAPTER_CATALOG: [AdapterSpec; 1] = [AdapterSpec {
    model_id: FT_MODEL_ID,
    title: "Laguna XS 2.1 finetune",
    digest: "sha256:fd0f4bc06b421a925c32253d45c3a834ee6ed0e13e1b7f71ff2e5d9960aaee16",
    base_revision: "841778bda563a36104dd521e37d99218e46f4f25",
    object_prefix: "laguna-xs-2.1/ft",
    download_bytes: 6_570_553,
}];

pub fn adapter_spec(model_id: &str) -> Result<AdapterSpec> {
    ADAPTER_CATALOG
        .iter()
        .copied()
        .find(|spec| spec.model_id == model_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown adapter `{model_id}`"))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ManifestFile {
    pub path: String,
    pub sha256: String,
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ManifestBase {
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AdapterManifest {
    pub schema_version: String,
    pub digest: String,
    pub base: ManifestBase,
    #[serde(default)]
    pub files: Vec<ManifestFile>,
}

/// Where installed adapters live, matching the local LoRA catalog's root.
pub fn install_dir(digest: &str) -> PathBuf {
    let hex = digest.trim().trim_start_matches("sha256:");
    crate::optimizers::durable_lora_root().join(hex)
}

pub fn is_installed(spec: &AdapterSpec) -> bool {
    let root = install_dir(spec.digest);
    root.join("adapter_config.json").is_file() && root.join("adapters.safetensors").is_file()
}

fn digest_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Identity comes from the catalog's own definition, not a second one here.
pub fn digest_tree(root: &Path) -> Result<String> {
    crate::optimizers::digest_adapter_directory(root)
}

/// Refuse an adapter trained against different base weights.
///
/// A mismatched pair still produces fluent text, so this cannot be a warning:
/// nothing downstream would notice, and the user would attribute the damage to
/// the finetune itself.
pub fn check_base_revision(manifest: &AdapterManifest, installed_revision: &str) -> Result<()> {
    if manifest.base.revision.is_empty() {
        bail!("This adapter does not declare the base revision it was trained against.");
    }
    if manifest.base.revision != installed_revision {
        bail!(
            "This adapter was trained against Laguna XS revision {} but {} is installed. \
             Install the matching base weights before using it.",
            &manifest.base.revision[..manifest.base.revision.len().min(12)],
            &installed_revision[..installed_revision.len().min(12)]
        );
    }
    Ok(())
}

pub fn parse_manifest(body: &str) -> Result<AdapterManifest> {
    let manifest: AdapterManifest =
        serde_json::from_str(body).context("decode the adapter manifest")?;
    if manifest.schema_version != MANIFEST_SCHEMA {
        bail!(
            "This adapter uses manifest schema {} which this version of Workshop cannot install.",
            manifest.schema_version
        );
    }
    if manifest.files.is_empty() {
        bail!("The adapter manifest lists no files.");
    }
    Ok(manifest)
}

/// Verify a staged tree against its manifest, then against its own digest.
///
/// Per-file digests catch a corrupted object; the tree digest catches a
/// missing or extra file that individually intact digests would not.
pub fn verify_tree(root: &Path, manifest: &AdapterManifest) -> Result<()> {
    for file in &manifest.files {
        let path = root.join(&file.path);
        let actual = digest_file(&path)
            .with_context(|| format!("{} is missing from the download", file.path))?;
        if actual != file.sha256 {
            bail!(
                "{} failed verification: expected {}, got {}",
                file.path,
                file.sha256,
                actual
            );
        }
    }
    let recomputed = digest_tree(root)?;
    if recomputed != manifest.digest {
        bail!(
            "The downloaded adapter does not match its published identity: expected {}, got {}",
            manifest.digest,
            recomputed
        );
    }
    Ok(())
}

/// Fetch, verify, and stage a published adapter.
///
/// Returns the staged directory. Installation proper is the catalog's own
/// import path, so a downloaded adapter and a hand-imported one end up
/// identical rather than arriving through two subtly different routes.
pub async fn stage_download<F>(
    client: &reqwest::Client,
    spec: &AdapterSpec,
    installed_base_revision: &str,
    mut progress: F,
) -> Result<(PathBuf, AdapterManifest)>
where
    F: FnMut(&str, &str, u64, u64),
{
    let prefix = format!(
        "{}/{}/{}",
        source_base_url().trim_end_matches('/'),
        spec.object_prefix,
        spec.digest.replace(':', "-")
    );
    progress("preparing", "Reading the adapter manifest…", 0, spec.download_bytes);
    let body = client
        .get(format!("{prefix}/{MANIFEST_FILE}"))
        .send()
        .await
        .with_context(|| format!("adapter manifest is unreachable at {prefix}"))?
        .error_for_status()
        .context("adapter manifest could not be read")?
        .text()
        .await?;
    let manifest = parse_manifest(&body)?;
    // The build pins the digest, so a substituted manifest cannot redirect the
    // install to different bytes than the ones this version was shipped for.
    if manifest.digest != spec.digest {
        bail!(
            "The published adapter is {} but this version of Workshop installs {}.",
            manifest.digest,
            spec.digest
        );
    }
    check_base_revision(&manifest, installed_base_revision)?;

    let staging = crate::optimizers::durable_lora_root()
        .join(".staging")
        .join(spec.digest.trim_start_matches("sha256:"));
    if staging.exists() {
        fs::remove_dir_all(&staging).ok();
    }
    fs::create_dir_all(&staging)?;
    let total: u64 = manifest.files.iter().map(|file| file.bytes).sum();
    let mut done = 0u64;
    for file in &manifest.files {
        progress("downloading", &format!("Downloading {}…", file.path), done, total);
        let bytes = client
            .get(format!("{prefix}/{}", file.path))
            .send()
            .await
            .with_context(|| format!("{} is unreachable", file.path))?
            .error_for_status()
            .with_context(|| format!("{} could not be read", file.path))?
            .bytes()
            .await?;
        fs::write(staging.join(&file.path), &bytes)?;
        done += bytes.len() as u64;
    }
    progress("verifying", "Verifying the adapter…", done, total);
    if let Err(error) = verify_tree(&staging, &manifest) {
        // Never leave unverified bytes where an installer might later find
        // them and assume they were checked.
        fs::remove_dir_all(&staging).ok();
        return Err(error);
    }
    Ok((staging, manifest))
}

/// Record the manifest beside the installed tree, never inside it: a file in
/// the directory would change the digest the directory is identified by.
pub fn write_manifest_beside(install: &Path, manifest: &AdapterManifest) -> Result<()> {
    let mut path = install.as_os_str().to_owned();
    path.push(".manifest.json");
    fs::write(PathBuf::from(path), serde_json::to_vec_pretty(manifest)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn staged(root: &Path, body: &[u8]) -> AdapterManifest {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join("adapter_config.json"), br#"{"lora_parameters":{"rank":8}}"#).unwrap();
        fs::File::create(root.join("adapters.safetensors"))
            .unwrap()
            .write_all(body)
            .unwrap();
        AdapterManifest {
            schema_version: MANIFEST_SCHEMA.into(),
            digest: digest_tree(root).unwrap(),
            base: ManifestBase {
                model_id: "poolside/Laguna-XS-2.1-NVFP4-mlx".into(),
                revision: "841778bda563a36104dd521e37d99218e46f4f25".into(),
            },
            files: vec![
                ManifestFile {
                    path: "adapter_config.json".into(),
                    sha256: digest_file(&root.join("adapter_config.json")).unwrap(),
                    bytes: 0,
                },
                ManifestFile {
                    path: "adapters.safetensors".into(),
                    sha256: digest_file(&root.join("adapters.safetensors")).unwrap(),
                    bytes: body.len() as u64,
                },
            ],
        }
    }

    /// Ignored by default: needs an object store. Run the MinIO profile and
    /// publish a fixture, then:
    ///   SYNTH_ADAPTERS_BASE_URL=http://127.0.0.1:9000/synth-adapters \
    ///     cargo test --lib downloads_and_verifies -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "requires a published adapter in an object store"]
    async fn downloads_and_verifies_a_published_adapter() {
        let spec = ADAPTER_CATALOG[0];
        let client = reqwest::Client::new();
        let (staged, manifest) =
            stage_download(&client, &spec, spec.base_revision, |phase, detail, done, total| {
                println!("{phase}: {detail} ({done}/{total})");
            })
            .await
            .expect("download and verify");
        assert_eq!(manifest.digest, spec.digest);
        assert_eq!(digest_tree(&staged).unwrap(), spec.digest);
        fs::remove_dir_all(&staged).ok();
    }

    #[test]
    fn the_tree_digest_matches_the_publisher() {
        // Pinned against `tools/adapters/synth_adapters.py::digest_tree` for
        // this exact tree. If either side changes its framing, a published
        // adapter stops matching the id it was published under and this fails
        // instead of shipping an adapter nobody can install.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("adapter");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("adapter_config.json"), br#"{"lora_parameters":{"rank":8}}"#).unwrap();
        fs::write(root.join("adapters.safetensors"), b"weights").unwrap();
        assert_eq!(
            digest_tree(&root).unwrap(),
            "sha256:a15969e7c3f250d0118b7f9bd9152559043d8cece49295cefbcd1fa4eb3e842d"
        );
    }

    #[test]
    fn a_clean_tree_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("adapter");
        let manifest = staged(&root, b"weights");
        verify_tree(&root, &manifest).unwrap();
    }

    #[test]
    fn a_flipped_byte_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("adapter");
        let manifest = staged(&root, b"weights");
        fs::write(root.join("adapters.safetensors"), b"weightt").unwrap();
        let error = verify_tree(&root, &manifest).unwrap_err().to_string();
        assert!(error.contains("failed verification"), "{error}");
    }

    #[test]
    fn an_extra_file_breaks_the_tree_digest() {
        // Every listed file is individually intact here, so only the tree
        // digest can catch this.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("adapter");
        let manifest = staged(&root, b"weights");
        fs::write(root.join("smuggled.bin"), b"extra").unwrap();
        let error = verify_tree(&root, &manifest).unwrap_err().to_string();
        assert!(error.contains("published identity"), "{error}");
    }

    #[test]
    fn a_mismatched_base_revision_is_refused_not_warned() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("adapter");
        let manifest = staged(&root, b"weights");
        let error = check_base_revision(&manifest, "0000000000000000000000000000000000000000")
            .unwrap_err()
            .to_string();
        assert!(error.contains("was trained against"), "{error}");
        check_base_revision(&manifest, "841778bda563a36104dd521e37d99218e46f4f25").unwrap();
    }

    #[test]
    fn an_unknown_manifest_schema_is_refused() {
        let error = parse_manifest(r#"{"schema_version":"synth-adapter.v9","digest":"sha256:x","base":{},"files":[]}"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot install"), "{error}");
    }
}
