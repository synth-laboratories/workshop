//! Capability negotiation at the optimizer/container boundary.
//!
//! The producer declares facts in `metadata.liveEval`; Workshop policy admits
//! only registered templates and safe artifact media; the optimizer consumer
//! requires a primary and trace presentation. The persisted result is the
//! intersection of those three inputs, not a family-specific host guess.

use std::collections::BTreeSet;
use std::fmt;

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::models::{
    EffectiveContract, EffectiveVisualAttachment, EffectiveVisualState,
    EFFECTIVE_CONTRACT_SCHEMA_VERSION,
};
use crate::visuals::TemplateMeta;

const PRIMARY_FALLBACK: &str = "experiment.overview.v1";
pub(super) const TRACE_FALLBACK: &str = "trace.workbench.v1";
const POLICY_MEDIA_TYPES: &[&str] = &["application/json", "image/png", "video/mp4"];

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContractRefusalCode {
    DeclarationContradiction,
    DeclaredTemplateUnavailable,
    AmbiguousFamilyTemplate,
}

#[derive(Debug)]
pub(super) struct ContractRefusal {
    pub code: ContractRefusalCode,
    pub field: &'static str,
    pub detail: String,
}

impl fmt::Display for ContractRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "effective_contract_refused code={} field={}: {}",
            serde_json::to_value(self.code)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_else(|| "unknown".into()),
            self.field,
            self.detail
        )
    }
}

impl std::error::Error for ContractRefusal {}

fn refusal(
    code: ContractRefusalCode,
    field: &'static str,
    detail: impl Into<String>,
) -> anyhow::Error {
    ContractRefusal {
        code,
        field,
        detail: detail.into(),
    }
    .into()
}

fn live_eval_declaration(metadata: &Value) -> Value {
    metadata
        .get("liveEval")
        .or_else(|| metadata.get("live_eval"))
        .or_else(|| metadata.pointer("/metadata/liveEval"))
        .or_else(|| metadata.pointer("/metadata/live_eval"))
        .cloned()
        .unwrap_or_else(|| json!({}))
}

fn declaration_string<'a>(declaration: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        declaration
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn registered<'a>(templates: &'a [TemplateMeta], id: &str) -> Option<&'a TemplateMeta> {
    templates.iter().find(|template| template.id == id)
}

fn declared_template(
    role: &'static str,
    id: &str,
    templates: &[TemplateMeta],
) -> Result<EffectiveVisualAttachment> {
    if registered(templates, id).is_none() {
        return Err(refusal(
            ContractRefusalCode::DeclaredTemplateUnavailable,
            if role == "primary" {
                "liveEval.templateId"
            } else {
                "liveEval.traceTemplateId"
            },
            format!("container declared unregistered template {id:?}"),
        ));
    }
    Ok(EffectiveVisualAttachment {
        role: role.into(),
        state: EffectiveVisualState::Declared,
        template_id: Some(id.into()),
        reason: "container declaration names a registered Workshop template".into(),
    })
}

fn family_template(
    family: &str,
    templates: &[TemplateMeta],
) -> Result<Option<EffectiveVisualAttachment>> {
    let family = family.to_ascii_lowercase();
    let claimed: Vec<&TemplateMeta> = templates
        .iter()
        .filter(|template| template.genre.as_deref() == Some("live"))
        .filter(|template| {
            template
                .family
                .as_deref()
                .is_some_and(|claim| claim.eq_ignore_ascii_case(&family))
        })
        .collect();
    let matches: Vec<&TemplateMeta> = if claimed.is_empty() {
        templates
            .iter()
            .filter(|template| template.genre.as_deref() == Some("live"))
            .filter(|template| template.family.is_none())
            .filter(|template| {
                template
                    .tags
                    .iter()
                    .any(|tag| tag.eq_ignore_ascii_case(&family))
            })
            .collect()
    } else {
        claimed
    };
    match matches.as_slice() {
        [] => Ok(None),
        [template] => Ok(Some(EffectiveVisualAttachment {
            role: "primary".into(),
            state: EffectiveVisualState::FamilyMatched,
            template_id: Some(template.id.clone()),
            reason: format!("registered live template table declares family {family:?}"),
        })),
        _ => Err(refusal(
            ContractRefusalCode::AmbiguousFamilyTemplate,
            "liveEval.family",
            format!(
                "family {family:?} matches multiple registered live templates: {}",
                matches
                    .iter()
                    .map(|template| template.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

fn fallback(
    role: &str,
    template_id: &str,
    templates: &[TemplateMeta],
    reason: &str,
) -> EffectiveVisualAttachment {
    if registered(templates, template_id).is_some() {
        EffectiveVisualAttachment {
            role: role.into(),
            state: EffectiveVisualState::Fallback,
            template_id: Some(template_id.into()),
            reason: reason.into(),
        }
    } else {
        EffectiveVisualAttachment {
            role: role.into(),
            state: EffectiveVisualState::Empty,
            template_id: None,
            reason: format!("{reason}; fallback template {template_id:?} is not registered"),
        }
    }
}

fn declared_media_types(metadata: &Value, declaration: &Value) -> BTreeSet<String> {
    [
        declaration.get("artifactMediaTypes"),
        declaration.get("mediaTypes"),
        metadata.pointer("/capabilities/artifactMediaTypes"),
        metadata.pointer("/capabilities/artifact_media_types"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_array)
    .flatten()
    .filter_map(Value::as_str)
    .map(|value| value.trim().to_ascii_lowercase())
    .filter(|value| !value.is_empty())
    .collect()
}

pub(super) fn negotiate(
    optimizer_run_id: &str,
    container_id: &str,
    task_family: Option<&str>,
    metadata: &Value,
    templates: &[TemplateMeta],
) -> Result<EffectiveContract> {
    let declared = live_eval_declaration(metadata);
    let declared_family = declaration_string(&declared, &["family", "taskFamily"]);
    if let (Some(expected), Some(declared)) = (task_family, declared_family) {
        if !expected.eq_ignore_ascii_case(declared) {
            return Err(refusal(
                ContractRefusalCode::DeclarationContradiction,
                "liveEval.family",
                format!(
                    "container row family {expected:?} contradicts liveEval family {declared:?}"
                ),
            ));
        }
    }
    let family = declared_family.or(task_family).map(str::to_string);

    let primary_visual = if let Some(id) = declaration_string(
        &declared,
        &["templateId", "template_id", "primaryTemplateId"],
    ) {
        declared_template("primary", id, templates)?
    } else if let Some(family) = family.as_deref() {
        family_template(family, templates)?.unwrap_or_else(|| {
            fallback(
                "primary",
                PRIMARY_FALLBACK,
                templates,
                "the declaration named no primary template and the family matched none",
            )
        })
    } else {
        fallback(
            "primary",
            PRIMARY_FALLBACK,
            templates,
            "the declaration named neither a primary template nor a family",
        )
    };

    let trace_visual = if let Some(id) =
        declaration_string(&declared, &["traceTemplateId", "trace_template_id"])
    {
        declared_template("trace", id, templates)?
    } else {
        fallback(
            "trace",
            TRACE_FALLBACK,
            templates,
            "the declaration named no trace template; using the family-agnostic workstation",
        )
    };

    let declared_media = declared_media_types(metadata, &declared);
    let artifact_media_types = POLICY_MEDIA_TYPES
        .iter()
        .filter(|media_type| declared_media.contains(**media_type))
        .map(|value| (*value).to_string())
        .collect();

    Ok(EffectiveContract {
        schema_version: EFFECTIVE_CONTRACT_SCHEMA_VERSION.into(),
        optimizer_run_id: optimizer_run_id.into(),
        container_id: container_id.into(),
        family,
        primary_visual,
        trace_visual,
        artifact_media_types,
        declared,
        consumer_needs: json!({
            "primaryVisual": "registered_template_or_honest_generic_fallback",
            "traceVisual": TRACE_FALLBACK,
            "artifactAccess": "declared_media_intersected_with_host_policy",
        }),
        negotiated_at: Utc::now().to_rfc3339(),
    })
}

pub(super) fn upsert(conn: &Connection, contract: &EffectiveContract) -> Result<()> {
    if contract.schema_version != EFFECTIVE_CONTRACT_SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported effective contract schema {}",
            contract.schema_version
        );
    }
    conn.execute(
        "INSERT INTO optimizer_effective_contracts(
            optimizer_run_id, schema_version, contract_json, negotiated_at
         ) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(optimizer_run_id) DO UPDATE SET
            schema_version=excluded.schema_version,
            contract_json=excluded.contract_json,
            negotiated_at=excluded.negotiated_at",
        params![
            contract.optimizer_run_id,
            contract.schema_version,
            serde_json::to_string(contract)?,
            contract.negotiated_at,
        ],
    )?;
    Ok(())
}

pub(super) fn load(conn: &Connection, optimizer_run_id: &str) -> Result<Option<EffectiveContract>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT contract_json FROM optimizer_effective_contracts WHERE optimizer_run_id=?1",
            [optimizer_run_id],
            |row| row.get(0),
        )
        .optional()?;
    raw.map(|raw| {
        serde_json::from_str(&raw).context("decode persisted optimizer effective contract")
    })
    .transpose()
}

