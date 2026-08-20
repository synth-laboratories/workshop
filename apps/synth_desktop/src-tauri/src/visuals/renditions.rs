//! Derived SVG/PNG renditions for Mermaid visuals. Canonical source stays in CAS blobs.

use super::charts;
use super::mermaid::{self, RenderedDiagram, Theme, MEDIA_TYPE_SVG, RENDERER_VERSION};
use super::systems;
use anyhow::{bail, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualRendition {
    pub visual_id: String,
    #[specta(type = specta_typescript::Unknown)]
    pub revision: i64,
    pub format: String,
    pub theme: String,
    pub size_class: String,
    pub content_digest: String,
    pub media_type: String,
    pub renderer_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = specta_typescript::Unknown)]
    pub width_px: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = specta_typescript::Unknown)]
    pub height_px: Option<i64>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualAsset {
    pub visual_id: String,
    #[specta(type = specta_typescript::Unknown)]
    pub revision: i64,
    pub format: String,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_class: Option<String>,
    pub digest: String,
    pub base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = specta_typescript::Unknown)]
    pub width_px: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = specta_typescript::Unknown)]
    pub height_px: Option<i64>,
}

pub fn insert_svg_rendition(
    conn: &Connection,
    visual_id: &str,
    revision: i64,
    digest: &str,
    rendered: &RenderedDiagram,
    theme: Theme,
    size_class: &str,
) -> Result<VisualRendition> {
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO visual_renditions(
            visual_id, revision, format, theme, size_class, content_digest,
            media_type, renderer_version, width_px, height_px, created_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            visual_id,
            revision,
            "svg",
            theme.as_str(),
            size_class,
            digest,
            MEDIA_TYPE_SVG,
            RENDERER_VERSION,
            rendered.width as i64,
            rendered.height as i64,
            created_at,
        ],
    )?;
    Ok(VisualRendition {
        visual_id: visual_id.to_string(),
        revision,
        format: "svg".into(),
        theme: theme.as_str().into(),
        size_class: size_class.into(),
        content_digest: digest.to_string(),
        media_type: MEDIA_TYPE_SVG.into(),
        renderer_version: RENDERER_VERSION.into(),
        width_px: Some(rendered.width as i64),
        height_px: Some(rendered.height as i64),
        created_at,
    })
}

pub fn insert_systems_svg_rendition(
    conn: &Connection,
    visual_id: &str,
    revision: i64,
    digest: &str,
    rendered: &systems::RenderedSystems,
    theme: &str,
    size_class: &str,
) -> Result<VisualRendition> {
    insert_svg_rendition_values(
        conn,
        visual_id,
        revision,
        digest,
        rendered.width,
        rendered.height,
        theme,
        size_class,
        systems::MEDIA_TYPE_SVG,
        systems::RENDERER_VERSION,
    )
}

pub fn insert_chart_svg_rendition(
    conn: &Connection,
    visual_id: &str,
    revision: i64,
    digest: &str,
    rendered: &charts::RenderedChart,
    theme: &str,
    size_class: &str,
) -> Result<VisualRendition> {
    insert_svg_rendition_values(
        conn,
        visual_id,
        revision,
        digest,
        rendered.width,
        rendered.height,
        theme,
        size_class,
        charts::MEDIA_TYPE_SVG,
        charts::RENDERER_VERSION,
    )
}

fn insert_svg_rendition_values(
    conn: &Connection,
    visual_id: &str,
    revision: i64,
    digest: &str,
    width: u32,
    height: u32,
    theme: &str,
    size_class: &str,
    media_type: &str,
    renderer_version: &str,
) -> Result<VisualRendition> {
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO visual_renditions(
            visual_id, revision, format, theme, size_class, content_digest,
            media_type, renderer_version, width_px, height_px, created_at
         ) VALUES (?1,?2,'svg',?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            visual_id,
            revision,
            theme,
            size_class,
            digest,
            media_type,
            renderer_version,
            width as i64,
            height as i64,
            created_at
        ],
    )?;
    Ok(VisualRendition {
        visual_id: visual_id.into(),
        revision,
        format: "svg".into(),
        theme: theme.into(),
        size_class: size_class.into(),
        content_digest: digest.into(),
        media_type: media_type.into(),
        renderer_version: renderer_version.into(),
        width_px: Some(width as i64),
        height_px: Some(height as i64),
        created_at,
    })
}

pub fn list_renditions(
    conn: &Connection,
    visual_id: &str,
    revision: i64,
) -> Result<Vec<VisualRendition>> {
    let mut stmt = conn.prepare(
        "SELECT visual_id, revision, format, theme, size_class, content_digest,
                media_type, renderer_version, width_px, height_px, created_at
         FROM visual_renditions
         WHERE visual_id = ?1 AND revision = ?2
         ORDER BY format, theme, size_class",
    )?;
    let rows = stmt.query_map(params![visual_id, revision], |row| {
        Ok(VisualRendition {
            visual_id: row.get(0)?,
            revision: row.get(1)?,
            format: row.get(2)?,
            theme: row.get(3)?,
            size_class: row.get(4)?,
            content_digest: row.get(5)?,
            media_type: row.get(6)?,
            renderer_version: row.get(7)?,
            width_px: row.get(8)?,
            height_px: row.get(9)?,
            created_at: row.get(10)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_rendition(
    conn: &Connection,
    visual_id: &str,
    revision: i64,
    format: &str,
    theme: &str,
    size_class: &str,
) -> Result<VisualRendition> {
    get_rendition_for_renderer(
        conn,
        visual_id,
        revision,
        format,
        theme,
        size_class,
        mermaid::RENDERER_VERSION,
    )
}

pub fn get_rendition_for_renderer(
    conn: &Connection,
    visual_id: &str,
    revision: i64,
    format: &str,
    theme: &str,
    size_class: &str,
    renderer_version: &str,
) -> Result<VisualRendition> {
    if !matches!(format, "svg" | "png") {
        bail!("unsupported rendition format {format}");
    }
    if !matches!(theme, "light" | "dark") {
        bail!("unsupported rendition theme {theme}");
    }
    if !matches!(size_class, "thumbnail" | "pane" | "export") {
        bail!("unsupported rendition size {size_class}");
    }
    conn.query_row(
        "SELECT visual_id, revision, format, theme, size_class, content_digest,
                media_type, renderer_version, width_px, height_px, created_at
         FROM visual_renditions
         WHERE visual_id = ?1 AND revision = ?2 AND format = ?3 AND theme = ?4 AND size_class = ?5
           AND renderer_version = ?6",
        params![
            visual_id,
            revision,
            format,
            theme,
            size_class,
            renderer_version
        ],
        |row| {
            Ok(VisualRendition {
                visual_id: row.get(0)?,
                revision: row.get(1)?,
                format: row.get(2)?,
                theme: row.get(3)?,
                size_class: row.get(4)?,
                content_digest: row.get(5)?,
                media_type: row.get(6)?,
                renderer_version: row.get(7)?,
                width_px: row.get(8)?,
                height_px: row.get(9)?,
                created_at: row.get(10)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow::anyhow!("rendition not found"))
}
