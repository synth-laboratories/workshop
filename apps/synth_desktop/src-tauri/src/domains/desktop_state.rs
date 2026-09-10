//! Durable desktop choices. The renderer is a cache of this instance-owned
//! state; compare-and-swap prevents a stale window from overwriting an agent.
use crate::core_runtime::CoreRuntime;
use anyhow::Result;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub value: Option<String>,
    pub revision: u32,
}
#[derive(Clone, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Write {
    pub key: String,
    pub value: Option<String>,
    pub expected_revision: u32,
}
#[derive(Clone, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
pub struct Snapshot {
    pub entries: BTreeMap<String, Entry>,
}

pub fn allowed(key: &str) -> bool {
    matches!(
        key,
        "synth.preferences.v1"
            | "synth.archivedContainerIds"
            | "synth.inferenceRailDefaultV2"
            | "synth.inferenceRailOpen"
            | "synth.workbenchSidePanelWidth"
            | "synth.workbenchZoomPercent"
            | "synth.accountChoiceMade"
            | "synth.training.lastRunId"
            | "synth.training.lastPlacement"
    ) || (key.starts_with("synth.models.")
        && key.len() < 160
        && key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)))
}
fn defaults() -> serde_json::Value {
    serde_json::from_str(include_str!("../contract/desktop_preferences.json"))
        .expect("generated desktop defaults")
}
pub fn read(core: &CoreRuntime) -> Result<Snapshot> {
    core.storage().database().with_conn(|conn| {
        let mut statement =
            conn.prepare("SELECT key, value, revision FROM desktop_state ORDER BY key")?;
        let mut entries = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    Entry {
                        value: row.get(1)?,
                        revision: row.get(2)?,
                    },
                ))
            })?
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
        entries
            .entry("synth.preferences.v1".into())
            .or_insert_with(|| Entry {
                value: Some(defaults().to_string()),
                revision: 0,
            });
        Ok(Snapshot { entries })
    })
}
pub fn write(core: &CoreRuntime, input: Write, agent: bool) -> Result<Entry> {
    anyhow::ensure!(allowed(&input.key), "unknown desktop state key");
    anyhow::ensure!(
        input
            .value
            .as_ref()
            .map_or(true, |value| value.len() <= 1024 * 1024),
        "desktop state value exceeds 1 MiB"
    );
    core.storage().database().transaction(|transaction| {
        let current: Option<Entry> = transaction.query_row("SELECT value, revision FROM desktop_state WHERE key = ?1", [&input.key],
            |row| Ok(Entry { value: row.get(0)?, revision: row.get(1)? })).optional()?;
        let revision = current.as_ref().map_or(0, |entry| entry.revision);
        anyhow::ensure!(revision == input.expected_revision, "desktop_state_conflict: read the current revision before retrying");
        if input.key == "synth.preferences.v1" {
            let next: serde_json::Value = serde_json::from_str(input.value.as_deref().ok_or_else(|| anyhow::anyhow!("preferences cannot be deleted"))?)?;
            anyhow::ensure!(next.is_object() && next["schemaVersion"] == defaults()["schemaVersion"], "preferences must use the current desktop schemaVersion 6");
            if agent {
                let previous: serde_json::Value = current.as_ref().and_then(|entry| entry.value.as_deref()).map(serde_json::from_str).transpose()?.unwrap_or_else(defaults);
                for key in ["approvalMode", "approvalPolicy", "sandboxMode"] {
                    anyhow::ensure!(next[key] == previous[key], "human_action_required: agents cannot change approval or sandbox preferences");
                }
            }
        }
        let entry = Entry { value: input.value, revision: revision.checked_add(1).ok_or_else(|| anyhow::anyhow!("desktop state revision overflow"))? };
        transaction.execute("INSERT INTO desktop_state(key, value, revision) VALUES (?1, ?2, ?3) ON CONFLICT(key) DO UPDATE SET value = excluded.value, revision = excluded.revision",
            rusqlite::params![input.key, entry.value, entry.revision])?;
        Ok(entry)
    })
}

pub struct Update;
impl crate::contract::capabilities::PublicOperation for Update {
    type Request = Write;
    type Response = Entry;
    const ID: &'static str = "desktop.state.update.v1";
    const MCP_NAME: &'static str = "desktop_state_update";
    const DESCRIPTION: &'static str = "Update one persisted desktop choice at an expected revision. Read desktop_state_get first. Values are strings as stored by the desktop: synth.preferences.v1 is the full JSON preferences object; synth.archivedContainerIds is a JSON array; synth.inferenceRailOpen is 0/1; synth.workbenchSidePanelWidth is pixels; synth.models.* holds model choices. Preserve unrelated fields. Approval/sandbox preferences require a human. Changes survive headless operation and are reflected by attached desktops.";
    const READ_ONLY: bool = false;
    const DESTRUCTIVE: bool = false;
    const IDEMPOTENT: bool = false;
}
