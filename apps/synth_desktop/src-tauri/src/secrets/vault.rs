//! SQLite metadata for secrets. Never stores the credential value.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::audit::{self, SecretAuditEvent};
use super::backend::{SecretBackend, SecretBytes};
use super::fingerprint::{self, display_suffix, fingerprint};

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SecretSummary {
    pub id: String,
    pub alias: String,
    pub provider: String,
    pub scope: String,
    pub status: String,
    pub backend: String,
    pub display_suffix: Option<String>,
    pub created_at: String,
    pub last_validated_at: Option<String>,
    pub allowed_recipes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SecretRecord {
    pub id: String,
    pub alias: String,
    pub provider: String,
    pub scope: String,
    pub backend: String,
    pub backend_ref: String,
    pub fingerprint: String,
    pub display_suffix: String,
    pub status: String,
    pub created_at: String,
    pub last_validated_at: Option<String>,
}

pub fn create(
    conn: &Connection,
    backend: &dyn SecretBackend,
    alias: &str,
    provider: &str,
    scope: &str,
    value: &SecretBytes,
    actor: &str,
) -> Result<SecretSummary> {
    let id = format!("sec_{}", Uuid::new_v4().simple());
    let backend_ref = backend.create(&id, value)?;
    let now = Utc::now().to_rfc3339();
    let suffix = display_suffix(value);
    let digest = fingerprint(value);
    conn.execute(
        "INSERT INTO secret_refs(
            id, alias, provider, scope, backend, backend_ref, fingerprint,
            display_suffix, status, created_at, updated_at, last_validated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'untested',?9,?9,NULL)",
        params![
            id,
            alias.trim(),
            provider.trim().to_ascii_lowercase(),
            scope.trim(),
            backend.backend_name(),
            backend_ref,
            digest,
            suffix,
            now
        ],
    )
    .context("persist secret metadata")?;
    let mut event = SecretAuditEvent::new("user", actor, "secret.create", "stored");
    event.secret_id = Some(id.clone());
    event.provider = Some(provider.to_ascii_lowercase());
    audit::append(conn, &event)?;
    get(conn, &id)?.ok_or_else(|| anyhow!("secret metadata missing after create"))
}

pub fn replace(
    conn: &Connection,
    backend: &dyn SecretBackend,
    id: &str,
    value: &SecretBytes,
    actor: &str,
) -> Result<SecretSummary> {
    let record = record(conn, id)?.ok_or_else(|| anyhow!("secret {id} was not found"))?;
    backend.replace(&record.backend_ref, value)?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE secret_refs SET fingerprint=?1, display_suffix=?2, status='untested',
         last_validated_at=NULL, updated_at=?3 WHERE id=?4",
        params![fingerprint(value), display_suffix(value), now, id],
    )?;
    let mut event = SecretAuditEvent::new("user", actor, "secret.replace", "rotated");
    event.secret_id = Some(id.into());
    event.provider = Some(record.provider);
    audit::append(conn, &event)?;
    get(conn, id)?.ok_or_else(|| anyhow!("secret metadata missing after replace"))
}

pub fn delete(conn: &Connection, backend: &dyn SecretBackend, id: &str, actor: &str) -> Result<()> {
    let record = record(conn, id)?.ok_or_else(|| anyhow!("secret {id} was not found"))?;
    backend.delete(&record.backend_ref)?;
    conn.execute("DELETE FROM secret_refs WHERE id=?1", [id])?;
    let mut event = SecretAuditEvent::new("user", actor, "secret.delete", "deleted");
    event.secret_id = Some(id.into());
    event.provider = Some(record.provider);
    audit::append(conn, &event)?;
    Ok(())
}

pub fn mark_status(conn: &Connection, id: &str, status: &str, actor: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let validated = if status == "valid" {
        Some(now.clone())
    } else {
        None
    };
    conn.execute(
        "UPDATE secret_refs SET status=?1, last_validated_at=COALESCE(?2, last_validated_at),
         updated_at=?3 WHERE id=?4",
        params![status, validated, now, id],
    )?;
    let mut event = SecretAuditEvent::new("user", actor, "secret.test", status);
    event.secret_id = Some(id.into());
    audit::append(conn, &event)?;
    Ok(())
}

pub fn record(conn: &Connection, id: &str) -> Result<Option<SecretRecord>> {
    conn.query_row(
        "SELECT id, alias, provider, scope, backend, backend_ref, fingerprint,
                display_suffix, status, created_at, last_validated_at
         FROM secret_refs WHERE id=?1",
        [id],
        |row| {
            Ok(SecretRecord {
                id: row.get(0)?,
                alias: row.get(1)?,
                provider: row.get(2)?,
                scope: row.get(3)?,
                backend: row.get(4)?,
                backend_ref: row.get(5)?,
                fingerprint: row.get(6)?,
                display_suffix: row.get(7)?,
                status: row.get(8)?,
                created_at: row.get(9)?,
                last_validated_at: row.get(10)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn find_by_provider(conn: &Connection, provider: &str) -> Result<Option<SecretRecord>> {
    conn.query_row(
        "SELECT id, alias, provider, scope, backend, backend_ref, fingerprint,
                display_suffix, status, created_at, last_validated_at
         FROM secret_refs WHERE provider=?1 ORDER BY updated_at DESC LIMIT 1",
        [provider],
        |row| {
            Ok(SecretRecord {
                id: row.get(0)?,
                alias: row.get(1)?,
                provider: row.get(2)?,
                scope: row.get(3)?,
                backend: row.get(4)?,
                backend_ref: row.get(5)?,
                fingerprint: row.get(6)?,
                display_suffix: row.get(7)?,
                status: row.get(8)?,
                created_at: row.get(9)?,
                last_validated_at: row.get(10)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn list(
    conn: &Connection,
    provider: Option<&str>,
    scope: Option<&str>,
) -> Result<Vec<SecretSummary>> {
    let mut sql = String::from(
        "SELECT id, alias, provider, scope, backend, display_suffix, status, created_at, last_validated_at
         FROM secret_refs WHERE 1=1",
    );
    if provider.is_some() {
        sql.push_str(" AND provider=?1");
    }
    if scope.is_some() {
        sql.push_str(if provider.is_some() {
            " AND scope=?2"
        } else {
            " AND scope=?1"
        });
    }
    sql.push_str(" ORDER BY provider, alias");
    let mut stmt = conn.prepare(&sql)?;
    let bind_provider = provider.unwrap_or("");
    let bind_scope = scope.unwrap_or("");
    let rows = match (provider, scope) {
        (Some(_), Some(_)) => stmt
            .query_map(params![bind_provider, bind_scope], map_summary)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (Some(_), None) => stmt
            .query_map(params![bind_provider], map_summary)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (None, Some(_)) => stmt
            .query_map(params![bind_scope], map_summary)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (None, None) => stmt
            .query_map([], map_summary)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    let mut out = Vec::new();
    for mut summary in rows {
        summary.allowed_recipes = recipe_grants(conn, &summary.id)?;
        out.push(summary);
    }
    Ok(out)
}

fn map_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<SecretSummary> {
    let suffix: String = row.get(5)?;
    Ok(SecretSummary {
        id: row.get(0)?,
        alias: row.get(1)?,
        provider: row.get(2)?,
        scope: row.get(3)?,
        backend: row.get(4)?,
        display_suffix: if suffix.is_empty() {
            None
        } else {
            Some(fingerprint::mask_suffix(&suffix))
        },
        status: row.get(6)?,
        created_at: row.get(7)?,
        last_validated_at: row.get(8)?,
        allowed_recipes: Vec::new(),
    })
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<SecretSummary>> {
    Ok(list(conn, None, None)?
        .into_iter()
        .find(|summary| summary.id == id))
}

pub fn recipe_grants(conn: &Connection, secret_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT recipe_id FROM secret_recipe_grants WHERE secret_id=?1 ORDER BY recipe_id",
    )?;
    let rows = stmt.query_map([secret_id], |row| row.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn grant_recipe(conn: &Connection, secret_id: &str, recipe_id: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO secret_recipe_grants(secret_id, recipe_id, granted_at)
         VALUES (?1,?2,?3)",
        params![secret_id, recipe_id, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub fn has_recipe_grant(conn: &Connection, secret_id: &str, recipe_id: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM secret_recipe_grants WHERE secret_id=?1 AND recipe_id=?2",
        params![secret_id, recipe_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn find_configured_source(conn: &Connection, provider: &str) -> Result<Option<SecretRecord>> {
    conn.query_row(
        "SELECT id, alias, provider, scope, backend, backend_ref, fingerprint,
                display_suffix, status, created_at, last_validated_at
         FROM secret_refs WHERE provider=?1 AND backend='configured_env_file'
         ORDER BY updated_at DESC LIMIT 1",
        [provider],
        |row| {
            Ok(SecretRecord {
                id: row.get(0)?,
                alias: row.get(1)?,
                provider: row.get(2)?,
                scope: row.get(3)?,
                backend: row.get(4)?,
                backend_ref: row.get(5)?,
                fingerprint: row.get(6)?,
                display_suffix: row.get(7)?,
                status: row.get(8)?,
                created_at: row.get(9)?,
                last_validated_at: row.get(10)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Host-only resolution. Callers must be the provider proxy or a validation path.
pub fn resolve_for_proxy(
    conn: &Connection,
    backend: &dyn SecretBackend,
    env_sources: Option<&super::lease::EnvSourceStore>,
    id: &str,
) -> Result<SecretBytes> {
    let record = record(conn, id)?.ok_or_else(|| anyhow!("secret {id} was not found"))?;
    if record.backend == super::lease::BACKEND_CONFIGURED_ENV {
        let store = env_sources.ok_or_else(|| {
            super::lease::CredentialError::new(
                super::lease::CREDENTIAL_VALUE_UNLOADED,
                "source",
                false,
                "configured env credential store is not attached to the proxy",
            )
            .anyhow()
        })?;
        return super::lease::resolve_configured_env(store, &record.backend_ref);
    }
    backend.resolve(&record.backend_ref).map_err(|error| {
        let message = error.to_string();
        if message.contains("locked") {
            anyhow!("the OS credential store is locked")
        } else if message.contains("not stored") || message.contains("not found") {
            anyhow!("secret {id} is missing from the credential backend")
        } else {
            error
        }
    })
}
