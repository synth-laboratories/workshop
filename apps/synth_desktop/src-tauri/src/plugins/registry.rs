//! Built-in product-plugin registry. Only `optimizers` is registered in this cut.

use super::types::{
    CatalogEntry, PluginActionReceipt, PluginStatus, DEV_RELEASE_CHANNEL, OFFICIAL_RELEASE_CHANNEL,
    OPTIMIZERS_PLUGIN_ID, PLUGIN_PUBLISHER,
};
use crate::optimizers::manager::{
    DEFAULT_SIDECAR_VERSION, DEV_SIDECAR_VERSION, OFFICIAL_SIDECAR_VERSION,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RegistryState {
    plugin_id: String,
    enabled: bool,
    #[serde(default = "default_release_channel")]
    release_channel: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_action_receipt_id: Option<String>,
    #[serde(default)]
    receipts: Vec<PluginActionReceipt>,
}

impl Default for RegistryState {
    fn default() -> Self {
        Self {
            plugin_id: OPTIMIZERS_PLUGIN_ID.into(),
            enabled: true,
            release_channel: default_release_channel(),
            last_action_receipt_id: None,
            receipts: Vec::new(),
        }
    }
}

pub struct PluginRegistry {
    path: PathBuf,
}

impl PluginRegistry {
    pub fn open_default() -> Self {
        Self {
            path: crate::storage::app_data_root().join("plugins/optimizers.json"),
        }
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn catalog_entry(version: Option<&str>) -> Result<CatalogEntry> {
        let version = version.unwrap_or(DEFAULT_SIDECAR_VERSION);
        let release_channel = match version {
            OFFICIAL_SIDECAR_VERSION => OFFICIAL_RELEASE_CHANNEL,
            DEV_SIDECAR_VERSION => DEV_RELEASE_CHANNEL,
            _ => {
                bail!("unknown optimizer catalog version `{version}`");
            }
        };
        Ok(CatalogEntry {
            plugin_id: OPTIMIZERS_PLUGIN_ID.into(),
            release_channel: release_channel.into(),
            version: version.into(),
            publisher: PLUGIN_PUBLISHER.into(),
            package: "synth-optimizers".into(),
            network_host: "pypi.org".into(),
            download_size_bytes: 12_000_000,
            workshop_compat: "0.4.0".into(),
            algorithms: vec!["gepa".into()],
            templates: vec!["optimizer.gepa.live.v1".into(), "optimizer.run.v1".into()],
            recipe_schema_version: "gepa.recipe.v1".into(),
            bounded_recipes: vec![
                "gepa.banking77.smoke.v1".into(),
                "gepa.banking77.luna.v1".into(),
                "gepa.banking77.sol.v1".into(),
                "gepa.craftax.smoke.v1".into(),
            ],
        })
    }

    pub fn selected_catalog_entry(&self, version: Option<&str>) -> Result<CatalogEntry> {
        let selected = self.release_channel();
        let version = version.unwrap_or_else(|| match selected.as_str() {
            DEV_RELEASE_CHANNEL => DEV_SIDECAR_VERSION,
            _ => OFFICIAL_SIDECAR_VERSION,
        });
        let entry = Self::catalog_entry(Some(version))?;
        if version != entry.version {
            bail!("optimizer catalog selection is inconsistent");
        }
        Ok(entry)
    }

    pub fn algorithm_version() -> &'static str {
        crate::optimizers::manager::DEFAULT_ALGORITHM_VERSION
    }

    pub fn is_enabled(&self) -> bool {
        self.load().enabled
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<bool> {
        let mut state = self.load();
        state.enabled = enabled;
        self.store(&state)?;
        Ok(enabled)
    }

    pub fn release_channel(&self) -> String {
        normalize_release_channel(&self.load().release_channel).to_owned()
    }

    pub fn set_release_channel(&self, channel: &str) -> Result<String> {
        let channel = normalize_release_channel_strict(channel)?;
        let mut state = self.load();
        state.release_channel = channel.to_owned();
        self.store(&state)?;
        Ok(channel.to_owned())
    }

    pub fn last_action_receipt_id(&self) -> Option<String> {
        self.load().last_action_receipt_id
    }

    pub fn record_receipt(&self, receipt: PluginActionReceipt) -> Result<PluginActionReceipt> {
        let mut state = self.load();
        state.last_action_receipt_id = Some(receipt.receipt_id.clone());
        state.receipts.push(receipt.clone());
        if state.receipts.len() > 64 {
            let excess = state.receipts.len() - 64;
            state.receipts.drain(0..excess);
        }
        self.store(&state)?;
        Ok(receipt)
    }

    pub fn receipt(&self, receipt_id: &str) -> Option<PluginActionReceipt> {
        self.load()
            .receipts
            .into_iter()
            .find(|receipt| receipt.receipt_id == receipt_id)
    }

    pub fn apply_to_status(&self, mut status: PluginStatus) -> PluginStatus {
        let state = self.load();
        status.enabled = state.enabled;
        status.last_action_receipt_id = state.last_action_receipt_id;
        status
    }

    fn load(&self) -> RegistryState {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn store(&self, state: &RegistryState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("create plugin registry directory")?;
        }
        fs::write(
            &self.path,
            serde_json::to_vec_pretty(state).context("encode plugin registry")?,
        )
        .context("write plugin registry")
    }
}

fn default_release_channel() -> String {
    OFFICIAL_RELEASE_CHANNEL.into()
}

fn normalize_release_channel(channel: &str) -> &'static str {
    if channel == DEV_RELEASE_CHANNEL {
        DEV_RELEASE_CHANNEL
    } else {
        OFFICIAL_RELEASE_CHANNEL
    }
}

fn normalize_release_channel_strict(channel: &str) -> Result<&'static str> {
    match channel {
        OFFICIAL_RELEASE_CHANNEL => Ok(OFFICIAL_RELEASE_CHANNEL),
        DEV_RELEASE_CHANNEL => Ok(DEV_RELEASE_CHANNEL),
        _ => bail!("unknown optimizer release channel `{channel}`"),
    }
}

pub fn optimizers_plugin_enabled() -> bool {
    PluginRegistry::open_default().is_enabled()
}
