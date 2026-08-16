//! Built-in product-plugin registry.
//!
//! State is one JSON file per plugin, named for it. `optimizers` is the only
//! plugin with a catalog today, but the file layout, the registry handle, and
//! the catalog lookup are all keyed by plugin id, so a second one adds a
//! catalog arm rather than a second registry.

use super::types::{
    CatalogEntry, CatalogPayload, PluginActionReceipt, PluginStatus, DEV_RELEASE_CHANNEL,
    OFFICIAL_RELEASE_CHANNEL, OPTIMIZERS_PLUGIN_ID, PLUGIN_PUBLISHER,
};
use crate::contract::runtimes::OPTIMIZERS as CONTRACT;
use crate::optimizers::manager::{
    DEFAULT_SIDECAR_VERSION, DEV_SIDECAR_VERSION, OFFICIAL_SIDECAR_VERSION,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

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

impl RegistryState {
    fn empty(plugin_id: &str) -> Self {
        Self {
            plugin_id: plugin_id.to_owned(),
            enabled: true,
            release_channel: default_release_channel(),
            last_action_receipt_id: None,
            receipts: Vec::new(),
        }
    }
}

pub struct PluginRegistry {
    plugin_id: String,
    path: PathBuf,
}

impl PluginRegistry {
    /// One file per plugin, named for it. A shared file would make each
    /// plugin's write a chance to clobber another's enabled flag.
    pub fn for_plugin(plugin_id: &str) -> Self {
        Self {
            plugin_id: plugin_id.to_owned(),
            path: crate::storage::app_data_root().join(format!("plugins/{plugin_id}.json")),
        }
    }

    pub fn open_default() -> Self {
        Self::for_plugin(OPTIMIZERS_PLUGIN_ID)
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self::for_plugin_at(OPTIMIZERS_PLUGIN_ID, path)
    }

    pub fn for_plugin_at(plugin_id: &str, path: PathBuf) -> Self {
        Self {
            plugin_id: plugin_id.to_owned(),
            path,
        }
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn catalog_entry(plugin_id: &str, version: Option<&str>) -> Result<CatalogEntry> {
        match plugin_id {
            OPTIMIZERS_PLUGIN_ID => Self::optimizers_catalog_entry(version),
            other => bail!("no catalog is registered for plugin `{other}`"),
        }
    }

    fn optimizers_catalog_entry(version: Option<&str>) -> Result<CatalogEntry> {
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
            package: CONTRACT.package.into(),
            network_host: "pypi.org".into(),
            download_size_bytes: 12_000_000,
            workshop_compat: CONTRACT.workshop_compat.into(),
            payload: CatalogPayload::Optimizers {
                algorithms: owned(CONTRACT.algorithms),
                templates: owned(CONTRACT.templates),
                recipe_schema_version: CONTRACT.recipe_schema.into(),
                bounded_recipes: owned(CONTRACT.bounded_recipes),
            },
        })
    }

    pub fn selected_catalog_entry(&self, version: Option<&str>) -> Result<CatalogEntry> {
        let selected = self.release_channel();
        let version = version.unwrap_or_else(|| match selected.as_str() {
            DEV_RELEASE_CHANNEL => DEV_SIDECAR_VERSION,
            _ => OFFICIAL_SIDECAR_VERSION,
        });
        let entry = Self::catalog_entry(&self.plugin_id, Some(version))?;
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
            .and_then(|raw| serde_json::from_str::<RegistryState>(&raw).ok())
            // A file recording a different plugin is another plugin's state at
            // our path. Treat it as absent rather than adopting its enabled
            // flag and its receipts.
            .filter(|state| state.plugin_id == self.plugin_id)
            .unwrap_or_else(|| RegistryState::empty(&self.plugin_id))
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn plugins_do_not_share_an_enabled_flag() {
        let dir = tempdir().unwrap();
        let optimizers = PluginRegistry::for_plugin_at(
            OPTIMIZERS_PLUGIN_ID,
            dir.path().join("plugins/optimizers.json"),
        );
        let second =
            PluginRegistry::for_plugin_at("computer-use", dir.path().join("plugins/computer-use.json"));
        optimizers.set_enabled(false).unwrap();
        assert!(!optimizers.is_enabled());
        assert!(second.is_enabled());
    }

    /// The file name is a convention, not a guarantee. State that records a
    /// different plugin must not lend this one its enabled flag or receipts.
    #[test]
    fn state_recorded_for_another_plugin_is_not_adopted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins/shared.json");
        PluginRegistry::for_plugin_at(OPTIMIZERS_PLUGIN_ID, path.clone())
            .set_enabled(false)
            .unwrap();
        assert!(PluginRegistry::for_plugin_at("computer-use", path).is_enabled());
    }

    #[test]
    fn a_plugin_without_a_catalog_is_refused_by_name() {
        let error = PluginRegistry::catalog_entry("computer-use", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("computer-use"), "{error}");
    }

    #[test]
    fn the_optimizers_catalog_still_carries_its_own_payload() {
        let entry = PluginRegistry::catalog_entry(OPTIMIZERS_PLUGIN_ID, None).unwrap();
        assert_eq!(entry.plugin_id, OPTIMIZERS_PLUGIN_ID);
        let CatalogPayload::Optimizers { algorithms, .. } = entry.payload;
        assert!(!algorithms.is_empty());
    }
}
