//! Supported Tinker student ids. Source of truth is
//! `docs/sft_tinker_base_models.toml` (not a Rust constant).

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const CATALOG_TOML: &str = include_str!("../../../../../docs/sft_tinker_base_models.toml");

#[derive(Debug, Deserialize)]
struct CatalogFile {
    default: String,
    model: Vec<CatalogModel>,
}

#[derive(Clone, Debug, Deserialize)]
struct CatalogModel {
    id: String,
}

#[derive(Clone, Debug)]
pub struct TinkerBaseModelCatalog {
    pub default_id: String,
    pub ids: Vec<String>,
}

impl TinkerBaseModelCatalog {
    pub fn load() -> Result<Self> {
        Self::from_toml(CATALOG_TOML)
    }

    fn from_toml(text: &str) -> Result<Self> {
        let parsed: CatalogFile =
            toml::from_str(text).context("parse sft_tinker_base_models.toml")?;
        if parsed.model.is_empty() {
            bail!("sft_tinker_base_models.toml has no [[model]] entries");
        }
        let ids: Vec<String> = parsed.model.into_iter().map(|model| model.id).collect();
        if !ids.iter().any(|id| id == &parsed.default) {
            bail!(
                "sft_tinker_base_models.toml default {} is not in [[model]]",
                parsed.default
            );
        }
        Ok(Self {
            default_id: parsed.default,
            ids,
        })
    }

    pub fn resolve(&self, requested: Option<&str>) -> Result<String> {
        let trimmed = requested.map(str::trim).filter(|value| !value.is_empty());
        let id = trimmed.unwrap_or(self.default_id.as_str());
        if id == "UNPINNED" {
            bail!(
                "base_model UNPINNED is refused; pick an id from docs/sft_tinker_base_models.toml"
            );
        }
        if !self.ids.iter().any(|allowed| allowed == id) {
            bail!(
                "base_model {id} is not in docs/sft_tinker_base_models.toml (default {})",
                self.default_id
            );
        }
        Ok(id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_default_is_nemotron_35_lightning() {
        let catalog = TinkerBaseModelCatalog::load().unwrap();
        assert_eq!(
            catalog.default_id,
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16"
        );
        assert_eq!(
            catalog.resolve(None).unwrap(),
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16"
        );
        assert_eq!(
            catalog
                .resolve(Some("nvidia/NVIDIA-Nemotron-3-Nano-30B-A3B-BF16"))
                .unwrap(),
            "nvidia/NVIDIA-Nemotron-3-Nano-30B-A3B-BF16"
        );
        assert!(catalog
            .resolve(Some("nvidia/nemotron-3-nano-30b-a3b"))
            .is_err());
        assert!(catalog.resolve(Some("UNPINNED")).is_err());
        assert!(catalog.resolve(Some("")).unwrap().contains("Lightning"));
        assert_eq!(
            catalog.resolve(Some("openai/gpt-oss-20b")).unwrap(),
            "openai/gpt-oss-20b"
        );
    }
}
