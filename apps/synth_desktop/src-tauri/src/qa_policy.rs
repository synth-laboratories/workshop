//! Operator-installed, expiring policy for isolated QA builds. No agent API
//! installs this file, resolves human approvals, or changes credential consent.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QaPolicy {
    pub schema_version: u32,
    pub id: String,
    pub instance: String,
    pub expires_at: String,
    pub container_roots: Vec<PathBuf>,
    pub recipe_roots: Vec<PathBuf>,
    pub containers: Vec<String>,
    pub recipes: Vec<String>,
    #[serde(default)]
    pub inline_evaluation_digests: Vec<String>,
    pub providers: Vec<String>,
    pub max_request_usd_micros: u64,
    pub max_total_usd_micros: u64,
    pub max_rollouts: u64,
}

impl QaPolicy {
    fn validate(&self, instance: &str, now: chrono::DateTime<chrono::Utc>) -> Result<()> {
        anyhow::ensure!(
            self.schema_version == 1 && self.instance == instance,
            "qa_policy_identity_mismatch"
        );
        anyhow::ensure!(
            !self.id.is_empty() && self.id.len() <= 96,
            "qa_policy_invalid_id"
        );
        let expiry = chrono::DateTime::parse_from_rfc3339(&self.expires_at)?;
        anyhow::ensure!(expiry > now, "qa_policy_expired");
        anyhow::ensure!(
            self.max_request_usd_micros > 0
                && self.max_request_usd_micros <= self.max_total_usd_micros
                && self.max_total_usd_micros < 50_000_000
                && self.max_rollouts > 0,
            "qa_policy_invalid_bounds"
        );
        anyhow::ensure!(
            self.providers
                .iter()
                .all(|p| p == "openrouter" || p == "openai"),
            "qa_policy_invalid_provider"
        );
        for root in self.container_roots.iter().chain(&self.recipe_roots) {
            anyhow::ensure!(root.is_absolute(), "qa_policy_root_must_be_absolute");
            let canonical = crate::project_sources::canonical_project_root(
                root.to_str().context("qa_policy_invalid_path")?,
            )?;
            anyhow::ensure!(&canonical == root, "qa_policy_root_must_be_canonical");
        }
        Ok(())
    }

    pub fn require_container(&self, root: &std::path::Path, id: &str) -> Result<()> {
        anyhow::ensure!(
            self.container_roots.iter().any(|p| p == root)
                && self.containers.iter().any(|p| p == id),
            "qa_policy_container_out_of_scope"
        );
        Ok(())
    }

    pub fn require_compute(
        &self,
        recipe: Option<&str>,
        provider: &str,
        ceiling: u64,
        rollouts: Option<u64>,
    ) -> Result<()> {
        anyhow::ensure!(
            recipe.is_some_and(|r| self.recipes.iter().any(|p| p == r))
                && self.providers.iter().any(|p| p == provider),
            "qa_policy_compute_out_of_scope"
        );
        anyhow::ensure!(
            ceiling > 0
                && ceiling <= self.max_request_usd_micros
                && rollouts.is_some_and(|r| r > 0 && r <= self.max_rollouts),
            "qa_policy_request_exceeds_bounds"
        );
        Ok(())
    }

    pub fn require_recipe_source(&self, recipe_id: &str) -> Result<()> {
        let mut matches = Vec::new();
        for root in &self.recipe_roots {
            if let Ok(recipe) = crate::optimizers::workspace_recipe::find_recipe(root, recipe_id) {
                matches.push(recipe);
            }
        }
        anyhow::ensure!(
            matches.len() == 1,
            "qa_policy_recipe_source_ambiguous_or_missing"
        );
        anyhow::ensure!(
            self.containers.contains(&matches[0].container)
                && self.providers.contains(&matches[0].provider),
            "qa_policy_recipe_target_out_of_scope"
        );
        Ok(())
    }

    // Full ceilings are charged permanently, even on failure/cancel. Unknown
    // provider usage and concurrent conversations cannot replenish this budget.
    pub fn reserve(&self, conn: &rusqlite::Connection, approval: &str, ceiling: u64) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS qa_policy_reservations (
            approval_id TEXT PRIMARY KEY, policy_id TEXT NOT NULL,
            ceiling INTEGER NOT NULL CHECK(ceiling > 0), receipt TEXT NOT NULL)",
        )?;
        let used: i64 = conn.query_row(
            "SELECT COALESCE(SUM(ceiling),0) FROM qa_policy_reservations WHERE policy_id=?1",
            [&self.id],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            (used as u64)
                .checked_add(ceiling)
                .is_some_and(|n| n <= self.max_total_usd_micros),
            "qa_policy_budget_exhausted"
        );
        conn.execute(
            "INSERT INTO qa_policy_reservations VALUES (?1,?2,?3,?4)",
            rusqlite::params![
                approval,
                self.id,
                ceiling as i64,
                serde_json::to_string(self)?
            ],
        )?;
        Ok(())
    }
}

pub(crate) fn active() -> Result<Option<QaPolicy>> {
    let path = crate::instance::data_root().join("qa-policy.json");
    if !path.exists() {
        return Ok(None);
    }
    if !cfg!(feature = "eval-driver") {
        bail!("qa_policy_requires_qa_build");
    }
    let instance = crate::instance::name().context("qa_policy_requires_named_instance")?;
    anyhow::ensure!(
        crate::instance::bundle_id().is_some_and(|id| id.contains(".dev.")),
        "qa_policy_requires_dev_bundle"
    );
    let policy: QaPolicy = serde_json::from_slice(&std::fs::read(path)?)?;
    policy.validate(&instance, chrono::Utc::now())?;
    Ok(Some(policy))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> QaPolicy {
        QaPolicy {
            schema_version: 1,
            id: "qa-one".into(),
            instance: "test".into(),
            expires_at: "2026-09-15T00:00:00Z".into(),
            container_roots: vec![],
            recipe_roots: vec![],
            containers: vec!["craftax".into()],
            recipes: vec!["qa.gepa".into()],
            inline_evaluation_digests: vec![],
            providers: vec!["openrouter".into()],
            max_request_usd_micros: 2_450_000,
            max_total_usd_micros: 4_900_000,
            max_rollouts: 12,
        }
    }
    #[test]
    fn scoped_and_expiring() {
        let p = policy();
        let now = "2026-09-14T00:00:00Z".parse().unwrap();
        assert!(p.validate("test", now).is_ok());
        assert!(p.validate("other", now).is_err());
        assert!(p
            .validate("test", "2026-09-16T00:00:00Z".parse().unwrap())
            .is_err());
        assert!(p
            .require_compute(Some("qa.gepa"), "openrouter", 2_450_000, Some(12))
            .is_ok());
        assert!(p.require_compute(None, "openrouter", 1, Some(1)).is_err());
        assert!(p
            .require_compute(Some("qa.gepa"), "openai", 1, Some(1))
            .is_err());
        assert!(p
            .require_compute(Some("qa.gepa"), "openrouter", 2_450_001, Some(1))
            .is_err());
        assert!(p
            .require_compute(Some("qa.gepa"), "openrouter", 1, Some(13))
            .is_err());
    }
    #[test]
    fn aggregate_ceilings_survive_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("budget.db");
        let p = policy();
        let mut db = rusqlite::Connection::open(&path).unwrap();
        let tx = db.transaction().unwrap();
        p.reserve(&tx, "conversation-one", 2_450_000).unwrap();
        tx.commit().unwrap();
        drop(db);
        let mut db = rusqlite::Connection::open(&path).unwrap();
        let tx = db.transaction().unwrap();
        p.reserve(&tx, "conversation-two", 2_450_000).unwrap();
        assert!(p.reserve(&tx, "retry", 1).is_err());
        tx.commit().unwrap();
    }
}
