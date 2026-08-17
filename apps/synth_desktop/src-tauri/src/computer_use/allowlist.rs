//! Which apps this agent may drive, and for how long. G5.
//!
//! The three scopes map onto the card in §6 — `[Once]`, `[This session]`,
//! `[Always]`. `Once` is never stored: it is consumed by the action that asked
//! for it, so storing it would turn a single yes into a standing one. The other
//! two are written to disk, because a grant that evaporates on restart trains
//! the operator to click through the card without reading it.
//!
//! An app denied by [`super::policy`] can never be recorded here. That check is
//! at the write, not only at the read: a denial that could be persisted around
//! by a stale file is not a denial.

use super::policy::{classify_app, DenialReason};
use crate::session::approval::ApprovalScope;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AllowlistEntry {
    /// Bundle identifier.
    pub app: String,
    /// `session` or `always`. `once` never appears on disk.
    pub scope: String,
    /// Set only for `session` grants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub granted_at: String,
    /// The approval this grant came from, so a grant can be traced to the card
    /// the operator actually saw.
    pub approval_receipt_id: String,
}

pub const SCOPE_SESSION: &str = "session";
pub const SCOPE_ALWAYS: &str = "always";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AllowlistFile {
    #[serde(default)]
    entries: Vec<AllowlistEntry>,
}

pub struct AppAllowlist {
    path: PathBuf,
    /// Serializes read-modify-write against concurrent sessions in one process.
    /// Cross-process safety comes from the single-writer rule: only Desktop
    /// writes this file.
    guard: Mutex<()>,
}

impl AppAllowlist {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            guard: Mutex::new(()),
        }
    }

    pub fn open_default() -> Self {
        Self::open(crate::storage::app_data_root().join("computer-use/allowlist.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// True when `app` may be driven by `session_id` without a fresh card.
    ///
    /// Denied apps answer `false` regardless of what is on disk, so a file
    /// edited by hand cannot re-open one.
    pub fn is_allowed(&self, app: &str, session_id: &str) -> bool {
        if classify_app(app).denial().is_some() {
            return false;
        }
        self.load().entries.iter().any(|entry| {
            entry.app.eq_ignore_ascii_case(app)
                && match entry.scope.as_str() {
                    SCOPE_ALWAYS => true,
                    SCOPE_SESSION => entry.session_id.as_deref() == Some(session_id),
                    _ => false,
                }
        })
    }

    /// Persist a grant. `Once` is accepted and deliberately does nothing.
    pub fn record(
        &self,
        app: &str,
        scope: &ApprovalScope,
        session_id: &str,
        approval_receipt_id: &str,
        granted_at: String,
    ) -> Result<Option<AllowlistEntry>> {
        if let Some(reason) = classify_app(app).denial() {
            bail!(
                "`{app}` cannot be allowlisted: {}",
                DenialReason::explain(&reason)
            );
        }
        let (scope_name, session) = match scope {
            ApprovalScope::Once => return Ok(None),
            ApprovalScope::Session => (SCOPE_SESSION, Some(session_id.to_owned())),
            ApprovalScope::Workspace => (SCOPE_ALWAYS, None),
        };
        let entry = AllowlistEntry {
            app: app.to_owned(),
            scope: scope_name.to_owned(),
            session_id: session,
            granted_at,
            approval_receipt_id: approval_receipt_id.to_owned(),
        };
        let _lock = self.guard.lock().unwrap_or_else(|error| error.into_inner());
        let mut file = self.load();
        // A broader grant supersedes a narrower one for the same app rather
        // than stacking, so revoking is one operation and not a hunt.
        file.entries.retain(|existing| {
            !(existing.app.eq_ignore_ascii_case(app)
                && (existing.scope == entry.scope
                    || (entry.scope == SCOPE_ALWAYS)
                    || existing.session_id == entry.session_id))
        });
        file.entries.push(entry.clone());
        self.store(&file)?;
        Ok(Some(entry))
    }

    /// Drop every grant for an app. Returns how many were removed so the caller
    /// can tell "revoked" from "there was nothing to revoke".
    pub fn revoke(&self, app: &str) -> Result<usize> {
        let _lock = self.guard.lock().unwrap_or_else(|error| error.into_inner());
        let mut file = self.load();
        let before = file.entries.len();
        file.entries
            .retain(|entry| !entry.app.eq_ignore_ascii_case(app));
        let removed = before - file.entries.len();
        if removed > 0 {
            self.store(&file)?;
        }
        Ok(removed)
    }

    /// Drop every session-scoped grant for one session. Called when a session
    /// ends: a session grant outliving its session is an always-grant nobody
    /// agreed to.
    pub fn clear_session(&self, session_id: &str) -> Result<usize> {
        let _lock = self.guard.lock().unwrap_or_else(|error| error.into_inner());
        let mut file = self.load();
        let before = file.entries.len();
        file.entries.retain(|entry| {
            !(entry.scope == SCOPE_SESSION && entry.session_id.as_deref() == Some(session_id))
        });
        let removed = before - file.entries.len();
        if removed > 0 {
            self.store(&file)?;
        }
        Ok(removed)
    }

    /// Everything currently granted, newest last. Feeds the app-scope chip.
    pub fn entries(&self) -> Vec<AllowlistEntry> {
        self.load().entries
    }

    /// Apps granted for this session or always, deduplicated — what the chip in
    /// §6 step 7 renders.
    pub fn apps_for(&self, session_id: &str) -> Vec<String> {
        let mut seen: BTreeMap<String, ()> = BTreeMap::new();
        for entry in self.load().entries {
            let visible = match entry.scope.as_str() {
                SCOPE_ALWAYS => true,
                SCOPE_SESSION => entry.session_id.as_deref() == Some(session_id),
                _ => false,
            };
            if visible {
                seen.insert(entry.app, ());
            }
        }
        seen.into_keys().collect()
    }

    fn load(&self) -> AllowlistFile {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|raw| serde_json::from_str::<AllowlistFile>(&raw).ok())
            .unwrap_or_default()
    }

    fn store(&self, file: &AllowlistFile) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("create computer-use state directory")?;
        }
        fs::write(
            &self.path,
            serde_json::to_vec_pretty(file).context("encode computer-use allowlist")?,
        )
        .context("write computer-use allowlist")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn allowlist(dir: &Path) -> AppAllowlist {
        AppAllowlist::open(dir.join("computer-use/allowlist.json"))
    }

    fn at() -> String {
        "2026-08-16T00:00:00Z".to_owned()
    }

    #[test]
    fn once_is_consumed_rather_than_stored() {
        let dir = tempdir().unwrap();
        let list = allowlist(dir.path());
        assert!(list
            .record(
                "com.apple.mail",
                &ApprovalScope::Once,
                "session-1",
                "approval-1",
                at()
            )
            .unwrap()
            .is_none());
        assert!(!list.is_allowed("com.apple.mail", "session-1"));
        assert!(list.entries().is_empty());
    }

    #[test]
    fn a_session_grant_does_not_leak_into_another_session() {
        let dir = tempdir().unwrap();
        let list = allowlist(dir.path());
        list.record(
            "com.apple.mail",
            &ApprovalScope::Session,
            "session-1",
            "approval-1",
            at(),
        )
        .unwrap();
        assert!(list.is_allowed("com.apple.mail", "session-1"));
        assert!(!list.is_allowed("com.apple.mail", "session-2"));
    }

    #[test]
    fn an_always_grant_applies_to_every_session() {
        let dir = tempdir().unwrap();
        let list = allowlist(dir.path());
        list.record(
            "com.apple.mail",
            &ApprovalScope::Workspace,
            "session-1",
            "approval-1",
            at(),
        )
        .unwrap();
        assert!(list.is_allowed("com.apple.mail", "session-2"));
    }

    /// G5 requires the scopes to survive a restart; re-opening the file is
    /// exactly what a restart does.
    #[test]
    fn grants_survive_reopening_the_file() {
        let dir = tempdir().unwrap();
        allowlist(dir.path())
            .record(
                "com.apple.mail",
                &ApprovalScope::Session,
                "session-1",
                "approval-1",
                at(),
            )
            .unwrap();
        let reopened = allowlist(dir.path());
        assert!(reopened.is_allowed("com.apple.mail", "session-1"));
        assert!(!reopened.is_allowed("com.apple.mail", "session-9"));
    }

    #[test]
    fn a_denied_app_can_neither_be_recorded_nor_read_back_as_allowed() {
        let dir = tempdir().unwrap();
        let list = allowlist(dir.path());
        let error = list
            .record(
                "com.apple.Terminal",
                &ApprovalScope::Workspace,
                "session-1",
                "approval-1",
                at(),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("Terminal-class"), "{error}");

        // Even a file edited by hand cannot re-open a denied app.
        let path = dir.path().join("computer-use/allowlist.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            serde_json::to_vec(&AllowlistFile {
                entries: vec![AllowlistEntry {
                    app: "com.apple.Terminal".into(),
                    scope: SCOPE_ALWAYS.into(),
                    session_id: None,
                    granted_at: at(),
                    approval_receipt_id: "forged".into(),
                }],
            })
            .unwrap(),
        )
        .unwrap();
        assert!(!allowlist(dir.path()).is_allowed("com.apple.Terminal", "session-1"));
    }

    #[test]
    fn a_broader_grant_replaces_a_narrower_one_instead_of_stacking() {
        let dir = tempdir().unwrap();
        let list = allowlist(dir.path());
        list.record(
            "com.apple.mail",
            &ApprovalScope::Session,
            "session-1",
            "approval-1",
            at(),
        )
        .unwrap();
        list.record(
            "com.apple.mail",
            &ApprovalScope::Workspace,
            "session-1",
            "approval-2",
            at(),
        )
        .unwrap();
        assert_eq!(list.entries().len(), 1);
        assert_eq!(list.entries()[0].scope, SCOPE_ALWAYS);
        assert_eq!(list.revoke("com.apple.mail").unwrap(), 1);
        assert!(!list.is_allowed("com.apple.mail", "session-1"));
        assert_eq!(list.revoke("com.apple.mail").unwrap(), 0);
    }

    #[test]
    fn ending_a_session_takes_its_grants_with_it() {
        let dir = tempdir().unwrap();
        let list = allowlist(dir.path());
        list.record(
            "com.apple.mail",
            &ApprovalScope::Session,
            "session-1",
            "approval-1",
            at(),
        )
        .unwrap();
        list.record(
            "com.figma.desktop",
            &ApprovalScope::Workspace,
            "session-1",
            "approval-2",
            at(),
        )
        .unwrap();
        assert_eq!(list.clear_session("session-1").unwrap(), 1);
        assert!(!list.is_allowed("com.apple.mail", "session-1"));
        // The always-grant is not a session grant and must survive.
        assert!(list.is_allowed("com.figma.desktop", "session-1"));
    }

    #[test]
    fn the_chip_shows_this_sessions_apps_only() {
        let dir = tempdir().unwrap();
        let list = allowlist(dir.path());
        list.record(
            "com.apple.mail",
            &ApprovalScope::Session,
            "session-1",
            "a",
            at(),
        )
        .unwrap();
        list.record(
            "com.other.app",
            &ApprovalScope::Session,
            "session-2",
            "b",
            at(),
        )
        .unwrap();
        list.record(
            "com.figma.desktop",
            &ApprovalScope::Workspace,
            "session-1",
            "c",
            at(),
        )
        .unwrap();
        assert_eq!(
            list.apps_for("session-1"),
            vec!["com.apple.mail".to_string(), "com.figma.desktop".to_string()]
        );
    }
}
