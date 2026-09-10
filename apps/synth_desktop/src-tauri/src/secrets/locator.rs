//! Durable credential locator metadata. This module never reads credential
//! values and never returns canonical workspace paths to adapters.

use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::lease::{
    CredentialError, CREDENTIAL_LOCATOR_LIMIT, CREDENTIAL_LOCATOR_NOT_REGULAR_FILE,
};
use super::path_gate::{self, GatedWorkspacePath};

pub const MAX_LIVE_LOCATORS: i64 = 64;
pub const MAX_PENDING_LOCATORS: i64 = 4;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CredentialLocatorKind {
    WorkspaceEnvFile,
    InstanceEnvFile,
    ProcessEnvironment,
    ExternalEnvFile,
}

impl CredentialLocatorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceEnvFile => "workspace_env_file",
            Self::InstanceEnvFile => "instance_env_file",
            Self::ProcessEnvironment => "process_environment",
            Self::ExternalEnvFile => "external_env_file",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "workspace_env_file" => Ok(Self::WorkspaceEnvFile),
            "instance_env_file" => Ok(Self::InstanceEnvFile),
            "process_environment" => Ok(Self::ProcessEnvironment),
            "external_env_file" => Ok(Self::ExternalEnvFile),
            _ => Err(anyhow!("unknown credential locator kind: {value}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CredentialLocatorState {
    Proposed,
    ApprovalPending,
    Observed,
    Missing,
    WorkspaceAuthorityRevoked,
    Superseded,
    Removed,
}

impl CredentialLocatorState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::ApprovalPending => "approval_pending",
            Self::Observed => "observed",
            Self::Missing => "missing",
            Self::WorkspaceAuthorityRevoked => "workspace_authority_revoked",
            Self::Superseded => "superseded",
            Self::Removed => "removed",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "proposed" => Ok(Self::Proposed),
            "approval_pending" => Ok(Self::ApprovalPending),
            "observed" => Ok(Self::Observed),
            "missing" => Ok(Self::Missing),
            "workspace_authority_revoked" => Ok(Self::WorkspaceAuthorityRevoked),
            "superseded" => Ok(Self::Superseded),
            "removed" => Ok(Self::Removed),
            _ => Err(anyhow!("unknown credential locator state: {value}")),
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        use CredentialLocatorState::*;
        matches!(
            (self, next),
            (Proposed, ApprovalPending | Removed)
                | (ApprovalPending, Observed | Removed)
                | (
                    Observed,
                    Missing | WorkspaceAuthorityRevoked | Superseded | Removed | ApprovalPending
                )
                | (
                    Missing,
                    Observed | WorkspaceAuthorityRevoked | Removed | Superseded
                )
                | (WorkspaceAuthorityRevoked, Removed)
                | (Superseded, Removed)
        )
    }
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialLocatorSummary {
    pub id: String,
    pub kind: CredentialLocatorKind,
    pub workspace_root_ref: Option<String>,
    pub relative_path: Option<String>,
    pub display_path: String,
    pub format: String,
    pub provider: String,
    pub variable: String,
    pub label: String,
    pub state: CredentialLocatorState,
    pub last_seen_at: Option<String>,
    pub source_id: Option<String>,
    pub registered: bool,
    pub preferred: bool,
    pub loaded: bool,
    pub source_state: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialBindingSummary {
    pub source_id: String,
    pub locator_id: Option<String>,
    pub provider: String,
    pub variable: String,
    pub label: String,
    pub preferred: bool,
    pub loaded: bool,
    pub source_state: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LocatorRecord {
    pub id: String,
    pub kind: CredentialLocatorKind,
    pub workspace_root_ref: Option<String>,
    pub workspace_canonical: Option<PathBuf>,
    pub relative_path: Option<String>,
    pub external_canonical: Option<PathBuf>,
    pub format: String,
    pub provider: String,
    pub variable: String,
    pub label: String,
    pub state: CredentialLocatorState,
    pub last_seen_at: Option<String>,
}

pub fn insert_workspace_pending(
    conn: &Connection,
    gated: &GatedWorkspacePath,
    provider: &str,
    variable: &str,
    label: &str,
) -> Result<LocatorRecord> {
    let record = insert(
        conn,
        CredentialLocatorKind::WorkspaceEnvFile,
        Some(&gated.workspace_root_ref),
        Some(&gated.root_canonical),
        Some(&gated.relative_path),
        None,
        provider,
        variable,
        label,
        CredentialLocatorState::ApprovalPending,
    )?;
    if record.state == CredentialLocatorState::Missing {
        set_observation_state(conn, &record.id, CredentialLocatorState::Observed)?;
        return get(conn, &record.id)?
            .ok_or_else(|| anyhow!("workspace locator missing after observation"));
    }
    Ok(record)
}

pub fn insert_external_observed(
    conn: &Connection,
    path: &Path,
    provider: &str,
    variable: &str,
    label: &str,
) -> Result<LocatorRecord> {
    let canonical = std::fs::canonicalize(path).map_err(|_| {
        CredentialError::new(
            CREDENTIAL_LOCATOR_NOT_REGULAR_FILE,
            "locator",
            false,
            "selected credential location is not a regular file",
        )
        .anyhow()
    })?;
    if !canonical.is_file() {
        return Err(CredentialError::new(
            CREDENTIAL_LOCATOR_NOT_REGULAR_FILE,
            "locator",
            false,
            "selected credential location is not a regular file",
        )
        .anyhow());
    }
    let record = insert(
        conn,
        CredentialLocatorKind::ExternalEnvFile,
        None,
        None,
        None,
        Some(&canonical),
        provider,
        variable,
        label,
        CredentialLocatorState::Observed,
    )?;
    if record.state == CredentialLocatorState::Missing {
        set_observation_state(conn, &record.id, CredentialLocatorState::Observed)?;
        return get(conn, &record.id)?
            .ok_or_else(|| anyhow!("external locator missing after observation"));
    }
    Ok(record)
}

pub fn upsert_instance(
    conn: &Connection,
    path: &Path,
    provider: &str,
    variable: &str,
    label: &str,
) -> Result<LocatorRecord> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let record = insert(
        conn,
        CredentialLocatorKind::InstanceEnvFile,
        None,
        None,
        None,
        Some(&canonical),
        provider,
        variable,
        label,
        if path.is_file() {
            CredentialLocatorState::Observed
        } else {
            CredentialLocatorState::Missing
        },
    )?;
    let state = if path.is_file() {
        CredentialLocatorState::Observed
    } else {
        CredentialLocatorState::Missing
    };
    conn.execute(
        "UPDATE credential_locators SET external_canonical=?1,state=?2,updated_at=?3,
         last_seen_at=CASE WHEN ?2='observed' THEN ?3 ELSE last_seen_at END WHERE id=?4",
        params![
            canonical.to_string_lossy().into_owned(),
            state.as_str(),
            Utc::now().to_rfc3339(),
            record.id
        ],
    )?;
    get(conn, &record.id)?.ok_or_else(|| anyhow!("instance locator missing after upsert"))
}

#[allow(clippy::too_many_arguments)]
fn insert(
    conn: &Connection,
    kind: CredentialLocatorKind,
    workspace_root_ref: Option<&str>,
    workspace_canonical: Option<&Path>,
    relative_path: Option<&str>,
    external_canonical: Option<&Path>,
    provider: &str,
    variable: &str,
    label: &str,
    state: CredentialLocatorState,
) -> Result<LocatorRecord> {
    let provider = provider.trim().to_ascii_lowercase();
    let variable = variable.trim();
    let location_key = workspace_root_ref
        .or_else(|| external_canonical.and_then(Path::to_str))
        .unwrap_or("process");
    let upsert_key = format!(
        "{}|{}|{}|{}|{}",
        kind.as_str(),
        location_key,
        relative_path.unwrap_or(""),
        variable,
        provider
    );
    if let Some(existing) = get_by_upsert_key(conn, &upsert_key)? {
        return Ok(existing);
    }
    enforce_caps(conn, state)?;
    let id = format!("loc_{}", Uuid::new_v4().simple());
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO credential_locators(
            id,kind,workspace_root_ref,workspace_canonical,relative_path,external_canonical,
            format,provider,variable,label,state,upsert_key,last_seen_at,created_at,updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,'dotenv',?7,?8,?9,?10,?11,?12,?13,?13)",
        params![
            id,
            kind.as_str(),
            workspace_root_ref,
            workspace_canonical.map(|path| path.to_string_lossy().into_owned()),
            relative_path,
            external_canonical.map(|path| path.to_string_lossy().into_owned()),
            provider,
            variable,
            label.trim(),
            state.as_str(),
            upsert_key,
            (state == CredentialLocatorState::Observed).then_some(now.clone()),
            now,
        ],
    )?;
    get(conn, &id)?.ok_or_else(|| anyhow!("credential locator missing after insert"))
}

fn enforce_caps(conn: &Connection, state: CredentialLocatorState) -> Result<()> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM credential_locators", [], |row| {
        row.get(0)
    })?;
    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM credential_locators WHERE state IN ('proposed','approval_pending')",
        [],
        |row| row.get(0),
    )?;
    if total >= MAX_LIVE_LOCATORS
        || (matches!(
            state,
            CredentialLocatorState::Proposed | CredentialLocatorState::ApprovalPending
        ) && pending >= MAX_PENDING_LOCATORS)
    {
        return Err(CredentialError::new(
            CREDENTIAL_LOCATOR_LIMIT,
            "locator",
            false,
            "credential locator registry limit reached",
        )
        .anyhow());
    }
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<LocatorRecord>> {
    conn.query_row(
        "SELECT id,kind,workspace_root_ref,workspace_canonical,relative_path,
                external_canonical,format,provider,variable,label,state,last_seen_at
         FROM credential_locators WHERE id=?1",
        [id],
        map_record,
    )
    .optional()
    .map_err(Into::into)
}

fn get_by_upsert_key(conn: &Connection, key: &str) -> Result<Option<LocatorRecord>> {
    conn.query_row(
        "SELECT id,kind,workspace_root_ref,workspace_canonical,relative_path,
                external_canonical,format,provider,variable,label,state,last_seen_at
         FROM credential_locators WHERE upsert_key=?1",
        [key],
        map_record,
    )
    .optional()
    .map_err(Into::into)
}

fn map_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocatorRecord> {
    let kind: String = row.get(1)?;
    let state: String = row.get(10)?;
    Ok(LocatorRecord {
        id: row.get(0)?,
        kind: CredentialLocatorKind::parse(&kind).map_err(|_| rusqlite::Error::InvalidQuery)?,
        workspace_root_ref: row.get(2)?,
        workspace_canonical: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
        relative_path: row.get(4)?,
        external_canonical: row.get::<_, Option<String>>(5)?.map(PathBuf::from),
        format: row.get(6)?,
        provider: row.get(7)?,
        variable: row.get(8)?,
        label: row.get(9)?,
        state: CredentialLocatorState::parse(&state).map_err(|_| rusqlite::Error::InvalidQuery)?,
        last_seen_at: row.get(11)?,
    })
}

pub fn transition(
    conn: &Connection,
    id: &str,
    next: CredentialLocatorState,
) -> Result<LocatorRecord> {
    let current = get(conn, id)?.ok_or_else(|| anyhow!("credential locator {id} was not found"))?;
    if !current.state.can_transition_to(next) {
        return Err(anyhow!(
            "credential locator cannot transition from {} to {}",
            current.state.as_str(),
            next.as_str()
        ));
    }
    if next == CredentialLocatorState::Removed {
        return Err(anyhow!(
            "Removed is represented by deleting the locator row"
        ));
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE credential_locators SET state=?1,last_seen_at=CASE WHEN ?1='observed' THEN ?2 ELSE last_seen_at END,updated_at=?2 WHERE id=?3",
        params![next.as_str(), now, id],
    )?;
    get(conn, id)?.ok_or_else(|| anyhow!("credential locator missing after transition"))
}

pub fn set_observation_state(
    conn: &Connection,
    id: &str,
    state: CredentialLocatorState,
) -> Result<()> {
    if !matches!(
        state,
        CredentialLocatorState::Observed
            | CredentialLocatorState::Missing
            | CredentialLocatorState::WorkspaceAuthorityRevoked
    ) {
        return Err(anyhow!("invalid observation state"));
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE credential_locators SET state=?1,last_seen_at=CASE WHEN ?1='observed' THEN ?2 ELSE last_seen_at END,updated_at=?2 WHERE id=?3",
        params![state.as_str(), now, id],
    )?;
    Ok(())
}

pub fn resolve_path(record: &LocatorRecord, allowed_roots: &[PathBuf]) -> Result<PathBuf> {
    match record.kind {
        CredentialLocatorKind::WorkspaceEnvFile => {
            let reference = record.workspace_root_ref.as_deref().ok_or_else(|| {
                anyhow!("workspace credential locator has no workspace root reference")
            })?;
            let relative = record
                .relative_path
                .as_deref()
                .ok_or_else(|| anyhow!("workspace credential locator has no relative path"))?;
            path_gate::gate_workspace_file(allowed_roots, reference, relative)
                .map(|gated| gated.file_canonical)
        }
        CredentialLocatorKind::InstanceEnvFile | CredentialLocatorKind::ExternalEnvFile => {
            let path = record
                .external_canonical
                .clone()
                .ok_or_else(|| anyhow!("credential locator has no internal file path"))?;
            let canonical = std::fs::canonicalize(&path).map_err(|_| {
                CredentialError::new(
                    CREDENTIAL_LOCATOR_NOT_REGULAR_FILE,
                    "locator",
                    false,
                    "credential locator does not identify a regular file",
                )
                .anyhow()
            })?;
            if canonical != path || !canonical.is_file() {
                return Err(CredentialError::new(
                    CREDENTIAL_LOCATOR_NOT_REGULAR_FILE,
                    "locator",
                    false,
                    "credential locator path changed or is not a regular file",
                )
                .anyhow());
            }
            Ok(canonical)
        }
        CredentialLocatorKind::ProcessEnvironment => Err(anyhow!(
            "process environment locators do not resolve to files"
        )),
    }
}

pub fn remove(conn: &Connection, id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM secret_refs WHERE locator_id=?1")?;
    let source_ids = stmt
        .query_map([id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    conn.execute("DELETE FROM secret_refs WHERE locator_id=?1", [id])?;
    let changed = conn.execute("DELETE FROM credential_locators WHERE id=?1", [id])?;
    if changed == 0 {
        return Err(anyhow!("credential locator {id} was not found"));
    }
    Ok(source_ids)
}

pub fn source_id(conn: &Connection, locator_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM secret_refs WHERE locator_id=?1 ORDER BY updated_at DESC LIMIT 1",
        [locator_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

pub fn preferred_source(
    conn: &Connection,
    provider: &str,
    variable: &str,
) -> Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT sr.id,sr.locator_id FROM secret_refs sr
         JOIN credential_locators l ON l.id=sr.locator_id
         WHERE sr.provider=?1 AND l.variable=?2 AND sr.preferred=1 AND sr.locator_id IS NOT NULL
         ORDER BY sr.updated_at DESC LIMIT 1",
        params![provider, variable],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(Into::into)
}

pub fn mark_preferred(
    conn: &Connection,
    source_id: &str,
    provider: &str,
    variable: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE secret_refs SET preferred=0,
            source_state=CASE WHEN preferred=1 THEN 'unloaded' ELSE source_state END
         WHERE id IN (
            SELECT sr.id FROM secret_refs sr
            JOIN credential_locators l ON l.id=sr.locator_id
            WHERE sr.provider=?1 AND l.variable=?2 AND sr.preferred=1
         )",
        params![provider, variable],
    )?;
    conn.execute(
        "UPDATE secret_refs SET preferred=1,updated_at=?1 WHERE id=?2",
        params![Utc::now().to_rfc3339(), source_id],
    )?;
    Ok(())
}

pub fn preferred_instance_source(
    conn: &Connection,
    provider: &str,
    variable: &str,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT sr.id FROM secret_refs sr
         JOIN credential_locators l ON l.id=sr.locator_id
         WHERE sr.provider=?1 AND l.variable=?2 AND l.kind='instance_env_file'
         ORDER BY sr.updated_at DESC LIMIT 1",
        params![provider, variable],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

pub fn list(
    conn: &Connection,
    loaded_backend_refs: &[String],
    include_external: bool,
) -> Result<Vec<CredentialLocatorSummary>> {
    let mut sql = String::from(
        "SELECT l.id,l.kind,l.workspace_root_ref,l.relative_path,l.external_canonical,
                l.format,l.provider,l.variable,l.label,l.state,l.last_seen_at,
                sr.id,sr.backend_ref,COALESCE(sr.preferred,0),sr.source_state
         FROM credential_locators l
         LEFT JOIN secret_refs sr ON sr.locator_id=l.id",
    );
    if !include_external {
        sql.push_str(" WHERE l.kind <> 'external_env_file'");
    }
    sql.push_str(" ORDER BY l.provider,l.variable,l.created_at");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        let kind_text: String = row.get(1)?;
        let state_text: String = row.get(9)?;
        let kind =
            CredentialLocatorKind::parse(&kind_text).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let external = row.get::<_, Option<String>>(4)?;
        let display_path = match kind {
            CredentialLocatorKind::WorkspaceEnvFile => row
                .get::<_, Option<String>>(3)?
                .unwrap_or_else(|| ".env".into()),
            CredentialLocatorKind::InstanceEnvFile => "Workshop instance .env".into(),
            CredentialLocatorKind::ProcessEnvironment => "Process environment".into(),
            CredentialLocatorKind::ExternalEnvFile => external
                .as_deref()
                .map(home_relative)
                .unwrap_or_else(|| "External .env".into()),
        };
        let source_id: Option<String> = row.get(11)?;
        let backend_ref: Option<String> = row.get(12)?;
        let preferred = row.get::<_, i64>(13)? != 0;
        Ok(CredentialLocatorSummary {
            id: row.get(0)?,
            kind,
            workspace_root_ref: row.get(2)?,
            relative_path: row.get(3)?,
            display_path,
            format: row.get(5)?,
            provider: row.get(6)?,
            variable: row.get(7)?,
            label: row.get(8)?,
            state: CredentialLocatorState::parse(&state_text)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            last_seen_at: row.get(10)?,
            registered: source_id.is_some(),
            source_id,
            preferred,
            loaded: preferred
                && backend_ref.as_ref().is_some_and(|reference| {
                    loaded_backend_refs.contains(&super::lease::env_store_ref(reference))
                }),
            source_state: row.get(14)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn bindings(
    conn: &Connection,
    loaded_backend_refs: &[String],
) -> Result<Vec<CredentialBindingSummary>> {
    let mut stmt = conn.prepare(
        "SELECT sr.id,sr.locator_id,sr.provider,
                COALESCE(l.variable,''),COALESCE(NULLIF(l.label,''),sr.alias),
                sr.preferred,sr.backend_ref,sr.source_state
         FROM secret_refs sr
         LEFT JOIN credential_locators l ON l.id=sr.locator_id
         ORDER BY sr.provider,sr.alias",
    )?;
    let rows = stmt.query_map([], |row| {
        let backend_ref: String = row.get(6)?;
        let preferred = row.get::<_, i64>(5)? != 0;
        Ok(CredentialBindingSummary {
            source_id: row.get(0)?,
            locator_id: row.get(1)?,
            provider: row.get(2)?,
            variable: row.get(3)?,
            label: row.get(4)?,
            preferred,
            loaded: preferred
                && loaded_backend_refs.contains(&super::lease::env_store_ref(&backend_ref)),
            source_state: row.get(7)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn all_records(conn: &Connection) -> Result<Vec<LocatorRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id,kind,workspace_root_ref,workspace_canonical,relative_path,
                external_canonical,format,provider,variable,label,state,last_seen_at
         FROM credential_locators ORDER BY created_at,id",
    )?;
    let rows = stmt.query_map([], map_record)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn home_relative(path: &str) -> String {
    let Some(home) = dirs::home_dir() else {
        return Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("External .env")
            .to_owned();
    };
    let path = Path::new(path);
    path.strip_prefix(&home)
        .map(|relative| format!("~/{}", relative.display()))
        .unwrap_or_else(|_| {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("External .env")
                .to_owned()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    #[test]
    fn transition_graph_refuses_unknown_edges() {
        assert!(CredentialLocatorState::Proposed
            .can_transition_to(CredentialLocatorState::ApprovalPending));
        assert!(
            !CredentialLocatorState::Proposed.can_transition_to(CredentialLocatorState::Observed)
        );
        assert!(!CredentialLocatorState::WorkspaceAuthorityRevoked
            .can_transition_to(CredentialLocatorState::Observed));
    }

    #[test]
    fn external_display_never_returns_an_absolute_home_path() {
        let home = dirs::home_dir().expect("test host has a home directory");
        let canonical = home.join("projects/example/.env");
        let display = home_relative(&canonical.to_string_lossy());
        let home_text = home.to_string_lossy();
        assert_eq!(display, "~/projects/example/.env");
        assert!(!display.contains(home_text.as_ref()));
    }

    #[test]
    fn removed_is_a_deleted_row() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let env = dir.path().join(".env");
        std::fs::write(&env, "OPENAI_API_KEY=not-real\n").unwrap();
        let record = storage
            .database()
            .with_conn(|conn| {
                insert_external_observed(conn, &env, "openai", "OPENAI_API_KEY", "OpenAI")
            })
            .unwrap();
        storage
            .database()
            .with_conn(|conn| remove(conn, &record.id).map(|_| ()))
            .unwrap();
        assert!(storage
            .database()
            .with_conn(|conn| get(conn, &record.id))
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_new_observation_recovers_missing_but_never_revoked_workspace_authority() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("storage")).unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join(".env"), "OPENAI_API_KEY=not-real\n").unwrap();
        let reference = path_gate::workspace_root_ref(&workspace.canonicalize().unwrap());
        let gated =
            path_gate::gate_workspace_file(std::slice::from_ref(&workspace), &reference, ".env")
                .unwrap();
        let record = storage
            .database()
            .with_conn(|conn| {
                insert_workspace_pending(conn, &gated, "openai", "OPENAI_API_KEY", "OpenAI")
            })
            .unwrap();
        storage
            .database()
            .with_conn(|conn| {
                transition(conn, &record.id, CredentialLocatorState::Observed)?;
                set_observation_state(conn, &record.id, CredentialLocatorState::Missing)?;
                Ok(())
            })
            .unwrap();
        let recovered = storage
            .database()
            .with_conn(|conn| {
                insert_workspace_pending(conn, &gated, "openai", "OPENAI_API_KEY", "OpenAI")
            })
            .unwrap();
        assert_eq!(recovered.state, CredentialLocatorState::Observed);

        storage
            .database()
            .with_conn(|conn| {
                set_observation_state(
                    conn,
                    &record.id,
                    CredentialLocatorState::WorkspaceAuthorityRevoked,
                )
            })
            .unwrap();
        let still_revoked = storage
            .database()
            .with_conn(|conn| {
                insert_workspace_pending(conn, &gated, "openai", "OPENAI_API_KEY", "OpenAI")
            })
            .unwrap();
        assert_eq!(
            still_revoked.state,
            CredentialLocatorState::WorkspaceAuthorityRevoked
        );
    }
}
