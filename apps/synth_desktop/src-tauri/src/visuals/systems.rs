//! Bounded explicit-layout systems scenes and deterministic SVG posters.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const STATIC_TEMPLATE_ID: &str = "diagram.systems.v1";
pub const DYNAMIC_TEMPLATE_ID: &str = "diagram.systems.dynamic.v1";
pub const MAX_SOURCE_BYTES: usize = 128 * 1024;
pub const MAX_AXIS_PX: f64 = 16_384.0;
pub const MAX_ITEMS: usize = 512;
pub const MAX_TIMELINE_ITEMS: usize = 2_000;
pub const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub const RENDERER_VERSION: &str = "workshop-systems-svg.1";
pub const MEDIA_TYPE_SOURCE: &str = "application/vnd.synth.systems+json";
pub const MEDIA_TYPE_SVG: &str = "image/svg+xml";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemsKind {
    Static,
    Dynamic,
}

impl SystemsKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Static => "systems",
            Self::Dynamic => "systems-dynamic",
        }
    }
}

pub fn template_kind(id: &str) -> Option<SystemsKind> {
    match id {
        STATIC_TEMPLATE_ID => Some(SystemsKind::Static),
        DYNAMIC_TEMPLATE_ID => Some(SystemsKind::Dynamic),
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scene {
    pub version: u8,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default = "default_theme")]
    pub theme: String,
    pub canvas: Canvas,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub poster_time_ms: Option<u64>,
    #[serde(default)]
    pub beats: Vec<Beat>,
    #[serde(default)]
    pub timeline: Vec<TimelineItem>,
    #[serde(default)]
    pub reduced_motion: Option<String>,
    #[serde(default)]
    pub design_rules: Option<DesignRules>,
}

fn default_theme() -> String {
    "light".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Canvas {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub label: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
}

fn default_true() -> bool {
    true
}
fn default_opacity() -> f64 {
    1.0
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Edge {
    #[serde(default)]
    pub id: Option<String>,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_route")]
    pub route: String,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default = "default_true")]
    pub directed: bool,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
}

fn default_route() -> String {
    "orthogonal".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Note {
    #[serde(default)]
    pub id: Option<String>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub text: String,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Beat {
    pub id: String,
    pub at_ms: u64,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    pub caption: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineItem {
    pub at_ms: u64,
    #[serde(default = "default_transition_ms")]
    pub duration_ms: u64,
    #[serde(default = "default_easing")]
    pub easing: String,
    pub target: String,
    pub changes: Changes,
}

fn default_transition_ms() -> u64 {
    600
}
fn default_easing() -> String {
    "ease-in-out".into()
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Changes {
    #[serde(default)]
    pub visible: Option<bool>,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub opacity: Option<f64>,
    #[serde(default)]
    pub emphasis: Option<bool>,
    #[serde(default)]
    pub style: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignRules {
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(default)]
    pub spacing: Option<f64>,
    #[serde(default)]
    pub easing: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RenderedSystems {
    pub svg: String,
    pub width: u32,
    pub height: u32,
}

pub fn parse_and_validate(source: &str, kind: SystemsKind) -> Result<Scene> {
    if source.len() > MAX_SOURCE_BYTES {
        bail!("systems scene exceeds {MAX_SOURCE_BYTES} bytes");
    }
    if source.as_bytes().contains(&0) {
        bail!("systems scene must not contain NUL");
    }
    let scene: Scene =
        serde_json::from_str(source).context("systems scene must be valid bounded JSON")?;
    validate_scene(&scene, kind)?;
    Ok(scene)
}

pub fn validate_source(source: &str, kind: SystemsKind) -> Result<()> {
    parse_and_validate(source, kind).map(|_| ())
}

fn validate_scene(scene: &Scene, kind: SystemsKind) -> Result<()> {
    if scene.version != 1 {
        bail!("systems scene version must be 1");
    }
    rect(
        "canvas",
        0.0,
        0.0,
        scene.canvas.width,
        scene.canvas.height,
        &scene.canvas,
    )?;
    if scene.canvas.width * scene.canvas.height > 32_000_000.0 {
        bail!("systems canvas exceeds 32 megapixels");
    }
    if !matches!(scene.theme.as_str(), "light" | "technical-dark") {
        bail!("theme must be light or technical-dark");
    }
    if scene.groups.len() + scene.nodes.len() + scene.edges.len() + scene.notes.len() > MAX_ITEMS {
        bail!("systems scene exceeds {MAX_ITEMS} items");
    }
    if scene.timeline.len() > MAX_TIMELINE_ITEMS {
        bail!("systems timeline exceeds {MAX_TIMELINE_ITEMS} changes");
    }
    safe_text(scene.title.as_deref().unwrap_or_default())?;
    let mut ids = HashSet::new();
    for group in &scene.groups {
        valid_id(&group.id)?;
        unique(&mut ids, &group.id)?;
        rect(
            &group.id,
            group.x,
            group.y,
            group.width,
            group.height,
            &scene.canvas,
        )?;
        safe_text(group.label.as_deref().unwrap_or_default())?;
        valid_style(group.style.as_deref())?;
        opacity(group.opacity)?;
    }
    for node in &scene.nodes {
        valid_id(&node.id)?;
        unique(&mut ids, &node.id)?;
        rect(
            &node.id,
            node.x,
            node.y,
            node.width,
            node.height,
            &scene.canvas,
        )?;
        safe_text(&node.label)?;
        valid_style(node.style.as_deref())?;
        opacity(node.opacity)?;
        if let Some(group) = &node.group {
            let container = scene
                .groups
                .iter()
                .find(|item| &item.id == group)
                .ok_or_else(|| {
                    anyhow::anyhow!("node {} references missing group {group}", node.id)
                })?;
            if node.x < container.x
                || node.y < container.y
                || node.x + node.width > container.x + container.width
                || node.y + node.height > container.y + container.height
            {
                bail!("node {} falls outside group {group}", node.id);
            }
        }
    }
    for (index, a) in scene.nodes.iter().enumerate() {
        for b in scene.nodes.iter().skip(index + 1) {
            if overlaps(a, b) {
                bail!("nodes {} and {} overlap", a.id, b.id);
            }
        }
    }
    let node_ids: HashSet<_> = scene.nodes.iter().map(|node| node.id.as_str()).collect();
    for edge in &scene.edges {
        if !node_ids.contains(edge.from.as_str()) || !node_ids.contains(edge.to.as_str()) {
            bail!("edge references missing node: {} -> {}", edge.from, edge.to);
        }
        if edge.from == edge.to {
            bail!("self edges are not supported: {}", edge.from);
        }
        if let Some(id) = &edge.id {
            valid_id(id)?;
            unique(&mut ids, id)?;
        }
        safe_text(edge.label.as_deref().unwrap_or_default())?;
        valid_style(edge.style.as_deref())?;
        opacity(edge.opacity)?;
        if !matches!(edge.route.as_str(), "orthogonal" | "straight") {
            bail!("edge route must be orthogonal or straight");
        }
    }
    for note in &scene.notes {
        rect("note", note.x, note.y, note.width, 1.0, &scene.canvas)?;
        safe_text(&note.text)?;
        valid_style(note.style.as_deref())?;
        opacity(note.opacity)?;
        if let Some(id) = &note.id {
            valid_id(id)?;
            unique(&mut ids, id)?;
        }
    }
    match kind {
        SystemsKind::Static
            if scene.duration_ms.is_some()
                || !scene.beats.is_empty()
                || !scene.timeline.is_empty() =>
        {
            bail!("static systems scenes cannot contain a timeline")
        }
        SystemsKind::Static => {}
        SystemsKind::Dynamic => {
            let duration = scene
                .duration_ms
                .ok_or_else(|| anyhow::anyhow!("dynamic systems scene requires durationMs"))?;
            if !(100..=600_000).contains(&duration) {
                bail!("durationMs must be between 100 and 600000");
            }
            let poster = scene
                .poster_time_ms
                .ok_or_else(|| anyhow::anyhow!("dynamic systems scene requires posterTimeMs"))?;
            if poster > duration {
                bail!("posterTimeMs exceeds durationMs");
            }
            if scene.beats.is_empty() {
                bail!("dynamic systems scene requires at least one beat");
            }
            let mut beat_ids = HashSet::new();
            let mut last = 0;
            for (index, beat) in scene.beats.iter().enumerate() {
                valid_id(&beat.id)?;
                unique(&mut beat_ids, &beat.id)?;
                safe_text(&beat.caption)?;
                safe_text(beat.description.as_deref().unwrap_or_default())?;
                if beat.at_ms > duration || (index > 0 && beat.at_ms < last) {
                    bail!("beats must be ordered within durationMs");
                }
                last = beat.at_ms;
            }
            if !matches!(scene.reduced_motion.as_deref(), Some("poster" | "final")) {
                bail!("dynamic systems scene requires reducedMotion poster or final");
            }
            let mut last_time = 0;
            for (index, item) in scene.timeline.iter().enumerate() {
                if item.at_ms > duration || (index > 0 && item.at_ms < last_time) {
                    bail!("timeline must be ordered within durationMs");
                }
                last_time = item.at_ms;
                if item.duration_ms > 60_000
                    || item.at_ms.saturating_add(item.duration_ms) > duration
                {
                    bail!("timeline transition must end within durationMs and be at most 60000ms");
                }
                if !matches!(
                    item.easing.as_str(),
                    "linear" | "ease-in" | "ease-out" | "ease-in-out" | "step-start" | "step-end"
                ) {
                    bail!("unsupported timeline easing '{}'", item.easing);
                }
                if !ids.contains(&item.target) {
                    bail!("timeline references missing target {}", item.target);
                }
                if let Some(value) = item.changes.opacity {
                    opacity(value)?;
                }
                if let Some(value) = item.changes.x {
                    finite_axis("timeline x", value)?;
                }
                if let Some(value) = item.changes.y {
                    finite_axis("timeline y", value)?;
                }
                valid_style(item.changes.style.as_deref())?;
                validate_timeline_geometry(scene, item)?;
            }
            if let Some(rules) = &scene.design_rules {
                safe_text(rules.font_family.as_deref().unwrap_or_default())?;
                safe_text(rules.easing.as_deref().unwrap_or_default())?;
                if let Some(v) = rules.spacing {
                    finite_axis("spacing", v)?;
                }
            }
        }
    }
    Ok(())
}

fn rect(label: &str, x: f64, y: f64, width: f64, height: f64, canvas: &Canvas) -> Result<()> {
    for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
        finite_axis(name, value)?;
    }
    if x < 0.0
        || y < 0.0
        || width <= 0.0
        || height <= 0.0
        || x + width > canvas.width
        || y + height > canvas.height
    {
        bail!("{label} rectangle falls outside canvas");
    }
    Ok(())
}
fn finite_axis(name: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value.abs() > MAX_AXIS_PX {
        bail!("{name} must be finite and within {MAX_AXIS_PX}");
    }
    Ok(())
}
fn opacity(value: f64) -> Result<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        bail!("opacity must be between 0 and 1");
    }
    Ok(())
}
fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        bail!("invalid systems item id '{id}'");
    }
    Ok(())
}
fn unique(ids: &mut HashSet<String>, id: &str) -> Result<()> {
    if !ids.insert(id.to_string()) {
        bail!("duplicate systems item id '{id}'");
    }
    Ok(())
}
fn safe_text(value: &str) -> Result<()> {
    let lower = value.to_ascii_lowercase();
    if value.len() > 4096
        || value.contains(['<', '>', '\0'])
        || [
            "javascript:",
            "http://",
            "https://",
            "data:",
            "file:",
            "@import",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        bail!("systems text contains unsafe markup, URL, or excessive content");
    }
    Ok(())
}
fn valid_style(style: Option<&str>) -> Result<()> {
    if let Some(style) = style {
        if !matches!(
            style,
            "solid"
                | "dashed"
                | "muted"
                | "warning"
                | "success"
                | "missing"
                | "unproven"
                | "accent"
        ) {
            bail!("unsupported systems style '{style}'");
        }
    }
    Ok(())
}
fn overlaps(a: &Node, b: &Node) -> bool {
    a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y
}

pub fn render_svg(source: &str, kind: SystemsKind) -> Result<RenderedSystems> {
    let mut scene = parse_and_validate(source, kind)?;
    if kind == SystemsKind::Dynamic {
        apply_poster(&mut scene);
    }
    let dark = scene.theme == "technical-dark";
    let (bg, surface, group, stroke, text, muted, accent) = if dark {
        (
            "#090A0C", "#111318", "#0D0F13", "#737782", "#F4F4F5", "#A1A1AA", "#A78BFA",
        )
    } else {
        (
            "#FFFFFF", "#F8FAFC", "#F1F5F9", "#64748B", "#0F172A", "#64748B", "#7C3AED",
        )
    };
    let mut out=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" role=\"img\" aria-labelledby=\"title desc\"><title id=\"title\">{}</title><desc id=\"desc\">Explicit-layout systems map</desc><defs><marker id=\"arrow\" markerWidth=\"8\" markerHeight=\"8\" refX=\"7\" refY=\"4\" orient=\"auto\"><path d=\"M0,0 L8,4 L0,8 z\" fill=\"{}\"/></marker></defs><rect width=\"100%\" height=\"100%\" fill=\"{}\"/><g font-family=\"ui-monospace,SFMono-Regular,Menlo,monospace\" font-size=\"14\" fill=\"{}\">", n(scene.canvas.width),n(scene.canvas.height),n(scene.canvas.width),n(scene.canvas.height),escape(scene.title.as_deref().unwrap_or("Systems map")),stroke,bg,text);
    for item in &scene.groups {
        if !item.visible {
            continue;
        }
        out.push_str(&format!("<g opacity=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\" fill=\"{}\" stroke=\"{}\" stroke-dasharray=\"6 5\"/>",n(item.opacity),n(item.x),n(item.y),n(item.width),n(item.height),group,style_color(item.style.as_deref(),stroke,accent)));
        if let Some(label) = &item.label {
            out.push_str(&text_at(
                item.x + 12.0,
                item.y + 22.0,
                label,
                "start",
                muted,
            ));
        }
        out.push_str("</g>");
    }
    let nodes: HashMap<_, _> = scene.nodes.iter().map(|v| (v.id.as_str(), v)).collect();
    for edge in &scene.edges {
        if !edge.visible {
            continue;
        }
        let a = nodes[edge.from.as_str()];
        let b = nodes[edge.to.as_str()];
        let (x1, y1, x2, y2) = anchors(a, b);
        let color = style_color(edge.style.as_deref(), stroke, accent);
        let dash = if matches!(
            edge.style.as_deref(),
            Some("dashed" | "missing" | "unproven")
        ) {
            " stroke-dasharray=\"7 5\""
        } else {
            ""
        };
        let marker = if edge.directed {
            " marker-end=\"url(#arrow)\""
        } else {
            ""
        };
        let path = if edge.route == "straight" {
            format!("M{} {} L{} {}", n(x1), n(y1), n(x2), n(y2))
        } else {
            let mid = (x1 + x2) / 2.0;
            format!("M{} {} H{} V{} H{}", n(x1), n(y1), n(mid), n(y2), n(x2))
        };
        out.push_str(&format!(
            "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\"{}{} opacity=\"{}\"/>",
            path,
            color,
            dash,
            marker,
            n(edge.opacity)
        ));
        if let Some(label) = &edge.label {
            let lx = (x1 + x2) / 2.0;
            let ly = (y1 + y2) / 2.0 - 7.0;
            let width = (label.chars().count() as f64 * 7.8 + 14.0).min(360.0);
            out.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"22\" rx=\"5\" fill=\"{}\"/>",
                n(lx - width / 2.0),
                n(ly - 15.0),
                n(width),
                surface
            ));
            out.push_str(&text_at(lx, ly, label, "middle", text));
        }
    }
    for item in &scene.nodes {
        if !item.visible {
            continue;
        }
        let color = style_color(item.style.as_deref(), stroke, accent);
        out.push_str(&format!("<g opacity=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.5\"/>",n(item.opacity),n(item.x),n(item.y),n(item.width),n(item.height),surface,color));
        let lines: Vec<_> = item.label.lines().collect();
        let start = item.y + item.height / 2.0 - (lines.len().saturating_sub(1) as f64 * 9.0);
        for (i, line) in lines.iter().enumerate() {
            out.push_str(&text_at(
                item.x + item.width / 2.0,
                start + i as f64 * 18.0,
                line,
                "middle",
                text,
            ));
        }
        out.push_str("</g>");
    }
    for item in &scene.notes {
        if !item.visible {
            continue;
        }
        out.push_str(&format!("<g opacity=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"38\" rx=\"6\" fill=\"{}\" stroke=\"{}\"/>{}</g>",n(item.opacity),n(item.x),n(item.y),n(item.width),surface,style_color(item.style.as_deref(),stroke,accent),text_at(item.x+10.0,item.y+24.0,&item.text,"start",text)));
    }
    out.push_str("</g></svg>");
    if out.len() > MAX_OUTPUT_BYTES {
        bail!("systems SVG exceeds {MAX_OUTPUT_BYTES} bytes");
    }
    Ok(RenderedSystems {
        svg: out,
        width: scene.canvas.width.ceil() as u32,
        height: scene.canvas.height.ceil() as u32,
    })
}

fn apply_poster(scene: &mut Scene) {
    let at = scene.poster_time_ms.unwrap_or(0);
    for item in scene.timeline.clone().into_iter().filter(|v| v.at_ms <= at) {
        let progress = transition_progress(at, item.at_ms, item.duration_ms, &item.easing);
        let discrete = item.easing != "step-end" || progress >= 1.0;
        if let Some(node) = scene.nodes.iter_mut().find(|v| v.id == item.target) {
            apply_node(node, &item.changes, progress, discrete)
        } else if let Some(group) = scene.groups.iter_mut().find(|v| v.id == item.target) {
            if discrete {
                if let Some(v) = item.changes.visible {
                    group.visible = v
                }
            }
            if let Some(v) = item.changes.opacity {
                group.opacity += (v - group.opacity) * progress
            }
            if let Some(v) = item.changes.x {
                group.x += (v - group.x) * progress
            }
            if let Some(v) = item.changes.y {
                group.y += (v - group.y) * progress
            }
            if discrete && item.changes.emphasis == Some(true) {
                group.style = Some("accent".into())
            }
            if discrete {
                if let Some(v) = item.changes.style {
                    group.style = Some(v)
                }
            }
        } else if let Some(edge) = scene
            .edges
            .iter_mut()
            .find(|v| v.id.as_deref() == Some(&item.target))
        {
            if discrete {
                if let Some(v) = item.changes.visible {
                    edge.visible = v
                }
            }
            if let Some(v) = item.changes.opacity {
                edge.opacity += (v - edge.opacity) * progress
            }
            if discrete && item.changes.emphasis == Some(true) {
                edge.style = Some("accent".into())
            }
            if discrete {
                if let Some(v) = item.changes.style {
                    edge.style = Some(v)
                }
            }
        } else if let Some(note) = scene
            .notes
            .iter_mut()
            .find(|v| v.id.as_deref() == Some(&item.target))
        {
            if discrete {
                if let Some(v) = item.changes.visible {
                    note.visible = v
                }
            }
            if let Some(v) = item.changes.opacity {
                note.opacity += (v - note.opacity) * progress
            }
            if let Some(v) = item.changes.x {
                note.x += (v - note.x) * progress
            }
            if let Some(v) = item.changes.y {
                note.y += (v - note.y) * progress
            }
            if discrete {
                if let Some(v) = item.changes.style {
                    note.style = Some(v)
                }
            }
        }
    }
}
fn apply_node(node: &mut Node, c: &Changes, progress: f64, discrete: bool) {
    if discrete {
        if let Some(v) = c.visible {
            node.visible = v
        }
    }
    if let Some(v) = c.x {
        node.x += (v - node.x) * progress
    }
    if let Some(v) = c.y {
        node.y += (v - node.y) * progress
    }
    if let Some(v) = c.opacity {
        node.opacity += (v - node.opacity) * progress
    }
    if discrete && c.emphasis == Some(true) {
        node.style = Some("accent".into())
    }
    if discrete {
        if let Some(v) = &c.style {
            node.style = Some(v.clone())
        }
    }
}

fn transition_progress(now: u64, start: u64, duration: u64, easing: &str) -> f64 {
    let raw = if duration == 0 {
        1.0
    } else {
        ((now.saturating_sub(start)) as f64 / duration as f64).clamp(0.0, 1.0)
    };
    match easing {
        "linear" => raw,
        "ease-in" => raw * raw,
        "ease-out" => 1.0 - (1.0 - raw) * (1.0 - raw),
        "step-start" => 1.0,
        "step-end" => {
            if raw >= 1.0 {
                1.0
            } else {
                0.0
            }
        }
        _ => raw * raw * (3.0 - 2.0 * raw),
    }
}

fn validate_timeline_geometry(scene: &Scene, item: &TimelineItem) -> Result<()> {
    let (Some(x), Some(y)) = (item.changes.x, item.changes.y) else {
        if item.changes.x.is_none() && item.changes.y.is_none() {
            return Ok(());
        }
        let x = item.changes.x;
        let y = item.changes.y;
        if let Some(node) = scene.nodes.iter().find(|v| v.id == item.target) {
            return rect(
                &item.target,
                x.unwrap_or(node.x),
                y.unwrap_or(node.y),
                node.width,
                node.height,
                &scene.canvas,
            );
        }
        if let Some(group) = scene.groups.iter().find(|v| v.id == item.target) {
            return rect(
                &item.target,
                x.unwrap_or(group.x),
                y.unwrap_or(group.y),
                group.width,
                group.height,
                &scene.canvas,
            );
        }
        if let Some(note) = scene
            .notes
            .iter()
            .find(|v| v.id.as_deref() == Some(&item.target))
        {
            return rect(
                &item.target,
                x.unwrap_or(note.x),
                y.unwrap_or(note.y),
                note.width,
                1.0,
                &scene.canvas,
            );
        }
        bail!(
            "timeline target {} does not support geometry changes",
            item.target
        )
    };
    if let Some(node) = scene.nodes.iter().find(|v| v.id == item.target) {
        rect(&item.target, x, y, node.width, node.height, &scene.canvas)
    } else if let Some(group) = scene.groups.iter().find(|v| v.id == item.target) {
        rect(&item.target, x, y, group.width, group.height, &scene.canvas)
    } else if let Some(note) = scene
        .notes
        .iter()
        .find(|v| v.id.as_deref() == Some(&item.target))
    {
        rect(&item.target, x, y, note.width, 1.0, &scene.canvas)
    } else {
        bail!(
            "timeline target {} does not support geometry changes",
            item.target
        )
    }
}
fn anchors(a: &Node, b: &Node) -> (f64, f64, f64, f64) {
    let ac = (a.x + a.width / 2.0, a.y + a.height / 2.0);
    let bc = (b.x + b.width / 2.0, b.y + b.height / 2.0);
    if (bc.0 - ac.0).abs() >= (bc.1 - ac.1).abs() {
        if bc.0 >= ac.0 {
            (a.x + a.width, ac.1, b.x, bc.1)
        } else {
            (a.x, ac.1, b.x + b.width, bc.1)
        }
    } else if bc.1 >= ac.1 {
        (ac.0, a.y + a.height, bc.0, b.y)
    } else {
        (ac.0, a.y, bc.0, b.y + b.height)
    }
}
fn style_color(style: Option<&str>, base: &str, accent: &str) -> String {
    match style {
        Some("warning") => "#F59E0B",
        Some("success") => "#22C55E",
        Some("missing" | "unproven") => "#EF4444",
        Some("accent") => accent,
        Some("muted") => "#71717A",
        _ => base,
    }
    .into()
}
fn text_at(x: f64, y: f64, value: &str, anchor: &str, color: &str) -> String {
    format!(
        "<text x=\"{}\" y=\"{}\" text-anchor=\"{}\" fill=\"{}\">{}</text>",
        n(x),
        n(y),
        anchor,
        color,
        escape(value)
    )
}
fn escape(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn n(v: f64) -> String {
    if v.fract().abs() < 0.000001 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    fn static_source() -> &'static str {
        r#"{"version":1,"title":"Query repair","theme":"technical-dark","canvas":{"width":800,"height":420},"groups":[{"id":"inputs","x":20,"y":20,"width":220,"height":360,"label":"Evidence"}],"nodes":[{"id":"mcp","x":50,"y":150,"width":160,"height":60,"label":"PlanetScale MCP","group":"inputs"},{"id":"agent","x":330,"y":150,"width":160,"height":60,"label":"AI agent"},{"id":"git","x":610,"y":150,"width":150,"height":60,"label":"GitHub"}],"edges":[{"id":"insight","from":"mcp","to":"agent","label":"insights"},{"from":"agent","to":"git","label":"pull request"}],"notes":[{"x":300,"y":280,"width":260,"text":"Evidence remains linked."}]}"#
    }
    #[test]
    fn deterministic_static_svg() {
        let a = render_svg(static_source(), SystemsKind::Static).unwrap();
        let b = render_svg(static_source(), SystemsKind::Static).unwrap();
        assert_eq!(
            Sha256::digest(a.svg.as_bytes()),
            Sha256::digest(b.svg.as_bytes())
        );
        assert!(a.svg.contains("PlanetScale MCP"));
    }
    #[test]
    fn rejects_dangling_and_unsafe() {
        assert!(parse_and_validate(r#"{"version":1,"canvas":{"width":100,"height":100},"nodes":[],"edges":[{"from":"x","to":"y"}]}"#,SystemsKind::Static).is_err());
        assert!(parse_and_validate(r#"{"version":1,"canvas":{"width":100,"height":100},"notes":[{"x":1,"y":1,"width":20,"text":"https://bad"}]}"#,SystemsKind::Static).is_err());
    }
    #[test]
    fn dynamic_poster_applies_timeline() {
        let source = r#"{"version":1,"canvas":{"width":400,"height":200},"nodes":[{"id":"a","x":20,"y":60,"width":100,"height":50,"label":"A"},{"id":"b","x":250,"y":60,"width":100,"height":50,"label":"B","visible":false}],"durationMs":3000,"posterTimeMs":1500,"reducedMotion":"poster","beats":[{"id":"one","atMs":0,"caption":"Start"}],"timeline":[{"atMs":1000,"target":"b","changes":{"visible":true,"emphasis":true}}]}"#;
        let out = render_svg(source, SystemsKind::Dynamic).unwrap();
        assert!(out.svg.contains(">B</text>"));
        assert!(out.svg.contains("#7C3AED"));
    }
    #[test]
    fn poster_samples_mid_transition_with_easing() {
        let source = r#"{"version":1,"canvas":{"width":400,"height":200},"nodes":[{"id":"a","x":20,"y":60,"width":100,"height":50,"label":"A"}],"durationMs":3000,"posterTimeMs":1500,"reducedMotion":"poster","beats":[{"id":"one","atMs":0,"caption":"Move"}],"timeline":[{"atMs":1000,"durationMs":1000,"easing":"linear","target":"a","changes":{"x":220,"opacity":0.5}}]}"#;
        let out = render_svg(source, SystemsKind::Dynamic).unwrap();
        assert!(out.svg.contains("<rect x=\"120\""));
        assert!(out.svg.contains("<g opacity=\"0.75\""));
    }
}
